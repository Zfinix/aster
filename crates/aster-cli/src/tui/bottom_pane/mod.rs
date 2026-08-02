//! The managed area under the transcript: composer, status row, slash menu,
//! and a stack of modal views. The top view gets the keys; the composer is
//! the fallback. Its height is wherever `desired_height` says — computed once.

mod approval;
mod list_selection;
mod model_picker;
mod status;
mod view;

pub(super) use approval::ApprovalView;
pub(super) use list_selection::{ListSelectionView, SelectionItem};
pub(super) use model_picker::ModelPickerView;
pub(super) use status::StatusWidget;
pub(super) use view::BottomPaneView;

use std::cell::Cell;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use tokio::sync::mpsc;

use super::composer::Composer;
use super::render::{Column, Inset, Insets, Probe, Renderable};
use super::terminal::FrameRequester;
use super::theme;
use crate::chat::{Answer, ApprovalRequest};

/// Rows a picker lists at once. The pane grows to whatever it renders, and
/// growing it scrolls history into scrollback that shrinking cannot pull back,
/// so an unbounded list leaves a blank band behind when the view closes.
const VISIBLE_ROWS: usize = 8;

/// The row a click landed on, counted from the top of `area`, or `None` when
/// it landed outside.
fn row_within(area: Rect, ev: MouseEvent) -> Option<u16> {
    let inside = ev.column >= area.x
        && ev.column < area.x + area.width
        && ev.row >= area.y
        && ev.row < area.y + area.height;
    inside.then(|| ev.row - area.y)
}

/// First row of a picker's visible window: keeps the selection centred until
/// it reaches either end of the list.
fn window_start(selected: usize, len: usize) -> usize {
    if len <= VISIBLE_ROWS {
        return 0;
    }
    selected
        .saturating_sub(VISIBLE_ROWS / 2)
        .min(len - VISIBLE_ROWS)
}

pub(super) struct CommandDesc {
    pub name: &'static str,
    pub takes_arg: bool,
    pub desc: &'static str,
}

/// What a keypress amounted to, once the pane has routed it.
pub(super) enum InputResult {
    None,
    /// A message to send, plus the `[@name]` → full-path references folded out
    /// of it so the caller can attach them as a resolvable block.
    Submitted {
        text: String,
        refs: Vec<(String, String)>,
    },
    /// A slash command to run (leading `/` stripped).
    Command(String),
    /// Enter on a draft while a turn is running; the draft is kept.
    Busy,
}

/// Cap the file index so huge repos stay cheap.
const MAX_FILE_INDEX: usize = 20_000;
/// Max mention suggestions shown at once.
const MAX_MENTION_MATCHES: usize = 8;

pub(super) struct BottomPane<E> {
    pub(super) composer: Composer,
    views: Vec<Box<dyn BottomPaneView<E>>>,
    status: Option<StatusWidget>,
    commands: &'static [CommandDesc],
    menu_sel: usize,
    placeholder: &'static str,
    frames: FrameRequester,
    tx: mpsc::UnboundedSender<E>,
    on_approval: fn(Answer, Option<PathBuf>) -> E,
    task_running: bool,
    file_index: Vec<String>,
    mention_sel: usize,
    /// Where the clickable things landed on the last draw. Rendering takes
    /// `&self`, so these are recorded rather than returned.
    view_area: Cell<Option<Rect>>,
    menu_area: Cell<Option<Rect>>,
    mention_area: Cell<Option<Rect>>,
}

impl<E: Clone + 'static> BottomPane<E> {
    pub(super) fn new(
        commands: &'static [CommandDesc],
        placeholder: &'static str,
        frames: FrameRequester,
        tx: mpsc::UnboundedSender<E>,
        on_approval: fn(Answer, Option<PathBuf>) -> E,
    ) -> Self {
        Self {
            composer: Composer::default(),
            views: Vec::new(),
            status: None,
            commands,
            menu_sel: 0,
            placeholder,
            frames,
            tx,
            on_approval,
            task_running: false,
            file_index: Vec::new(),
            mention_sel: 0,
            view_area: Cell::new(None),
            menu_area: Cell::new(None),
            mention_area: Cell::new(None),
        }
    }

    pub(super) fn has_active_view(&self) -> bool {
        !self.views.is_empty()
    }

    /// True while something on screen is worth clicking. The caller captures
    /// the mouse only then, so the transcript stays selectable the rest of the
    /// time.
    pub(super) fn wants_mouse(&self) -> bool {
        self.has_active_view()
            || !self.command_matches().is_empty()
            || self.mention_lines().is_some()
    }

    /// Click or scroll inside an open menu or picker. Anything outside one is
    /// ignored, so a stray click never disturbs the draft.
    pub(super) fn handle_mouse(&mut self, ev: MouseEvent) -> InputResult {
        let scroll = match ev.kind {
            MouseEventKind::ScrollUp => Some(-1),
            MouseEventKind::ScrollDown => Some(1),
            _ => None,
        };
        let click = matches!(ev.kind, MouseEventKind::Down(MouseButton::Left));
        if scroll.is_none() && !click {
            return InputResult::None;
        }
        self.frames.schedule_now();

        if let Some(top) = self.views.last_mut() {
            let area = self.view_area.get();
            if let Some(delta) = scroll {
                top.handle_scroll(delta);
                return InputResult::None;
            }
            if let Some(row) = area.and_then(|a| row_within(a, ev)) {
                top.handle_click(row);
                if top.is_complete() {
                    self.views.pop();
                }
            }
            return InputResult::None;
        }

        if !self.command_matches().is_empty()
            && let Some(area) = self.menu_area.get()
        {
            if let Some(delta) = scroll {
                self.menu_move(delta);
                return InputResult::None;
            }
            if let Some(row) = row_within(area, ev) {
                let len = self.command_matches().len();
                if (row as usize) < len {
                    self.menu_sel = row as usize;
                    let cmd = self.command_to_run();
                    self.composer.clear();
                    self.menu_sel = 0;
                    return InputResult::Command(cmd);
                }
            }
            return InputResult::None;
        }

        if let Some((start, _)) = self.composer.mention_context()
            && let Some(area) = self.mention_area.get()
        {
            if let Some(delta) = scroll {
                self.mention_move(delta);
                return InputResult::None;
            }
            if let Some(row) = row_within(area, ev) {
                self.mention_sel = row as usize;
                self.complete_selected_mention(start);
            }
        }
        InputResult::None
    }

    /// For views built outside the pane, which still route through its channel.
    pub(super) fn sender(&self) -> mpsc::UnboundedSender<E> {
        self.tx.clone()
    }

    pub(super) fn push_view(&mut self, view: Box<dyn BottomPaneView<E>>) {
        self.views.push(view);
        self.frames.schedule_now();
    }

    pub(super) fn push_picker(&mut self, title: &str, items: Vec<SelectionItem<E>>) {
        let view = ListSelectionView::new(title, items, self.tx.clone());
        self.push_view(Box::new(view));
    }

    /// Route an approval: the top view may absorb it; otherwise a new prompt
    /// opens.
    pub(super) fn push_approval(&mut self, req: ApprovalRequest) {
        let req = match self.views.last_mut() {
            Some(top) => match top.try_consume_approval(req) {
                None => {
                    self.frames.schedule_now();
                    return;
                }
                Some(req) => req,
            },
            None => req,
        };
        self.push_view(Box::new(ApprovalView::new(
            req,
            self.tx.clone(),
            self.on_approval,
        )));
    }

    pub(super) fn set_task_running(&mut self, running: bool) {
        self.task_running = running;
        self.status = running.then(|| StatusWidget::new(self.frames.clone()));
        self.frames.schedule_now();
    }

    pub(super) fn set_status_detail(&mut self, detail: Option<String>) {
        if let Some(status) = &mut self.status {
            status.set_detail(detail);
        }
    }

    pub(super) fn handle_paste(&mut self, text: String) {
        if let Some(top) = self.views.last_mut() {
            top.handle_paste(text);
        } else {
            self.composer.paste(&text);
        }
        self.frames.schedule_now();
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent, width: u16) -> InputResult {
        self.frames.schedule_now();
        if let Some(top) = self.views.last_mut() {
            top.handle_key(key);
            if top.is_complete() {
                self.views.pop();
            }
            return InputResult::None;
        }
        self.composer_key(key, width)
    }

    fn composer_key(&mut self, key: KeyEvent, width: u16) -> InputResult {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let menu_open = !self.command_matches().is_empty();
        let mention_open = self.composer.mention_context().is_some();

        // Mention menu owns these keys when open; slash menu is mutually exclusive.
        if mention_open
            && !menu_open
            && let Some((start, query)) = self.composer.mention_context()
        {
            let matches = self.mention_matches(query);
            if !matches.is_empty() {
                match key.code {
                    KeyCode::Up => {
                        self.mention_move(-1);
                        return InputResult::None;
                    }
                    KeyCode::Down => {
                        self.mention_move(1);
                        return InputResult::None;
                    }
                    KeyCode::Tab | KeyCode::Enter => {
                        self.complete_selected_mention(start);
                        return InputResult::None;
                    }
                    KeyCode::Esc => {
                        self.composer.cancel_mention(start);
                        self.mention_sel = 0;
                        return InputResult::None;
                    }
                    _ => {}
                }
            } else if key.code == KeyCode::Esc {
                self.composer.cancel_mention(start);
                self.mention_sel = 0;
                return InputResult::None;
            }
        }

        match key.code {
            KeyCode::Up if menu_open => self.menu_move(-1),
            KeyCode::Down if menu_open => self.menu_move(1),
            KeyCode::Tab if menu_open => self.complete_command(),

            KeyCode::Enter if alt || ctrl => self.composer.insert('\n'),
            KeyCode::Char('j') if ctrl => self.composer.insert('\n'),
            KeyCode::BackTab => self.composer.insert('\n'),
            // The menu is open and a row is highlighted, so enter picks that
            // row. Without this a bare `/` submits as a message, and arrowing
            // to a command does nothing, since the draft is still just `/`.
            KeyCode::Enter if menu_open => {
                let cmd = self.command_to_run();
                self.composer.clear();
                self.menu_sel = 0;
                return InputResult::Command(cmd);
            }
            KeyCode::Enter if !self.composer.text().trim().is_empty() => {
                if self.looks_like_command() {
                    let cmd = self.command_to_run();
                    self.composer.clear();
                    self.menu_sel = 0;
                    return InputResult::Command(cmd);
                }
                if !self.task_running {
                    let text = self.composer.take();
                    let refs = self.composer.take_refs();
                    return InputResult::Submitted { text, refs };
                }
                return InputResult::Busy;
            }

            KeyCode::Up => {
                if !self.composer.up(width) {
                    self.composer.recall_prev();
                }
            }
            KeyCode::Down => {
                if !self.composer.down(width) {
                    self.composer.recall_next();
                }
            }
            KeyCode::Left if ctrl || alt => self.composer.word_left(),
            KeyCode::Right if ctrl || alt => self.composer.word_right(),
            KeyCode::Left => self.composer.left(),
            KeyCode::Right => self.composer.right(),
            KeyCode::Home => self.composer.home(),
            KeyCode::End => self.composer.end(),
            KeyCode::Char('a') if ctrl => self.composer.home(),
            KeyCode::Char('e') if ctrl => self.composer.end(),
            KeyCode::Char('u') if ctrl => self.composer.kill_to_start(),
            KeyCode::Char('k') if ctrl => self.composer.kill_to_end(),
            KeyCode::Char('w') if ctrl => self.composer.delete_word_back(),
            KeyCode::Backspace if ctrl || alt => self.composer.delete_word_back(),
            KeyCode::Backspace => {
                self.composer.backspace();
                self.menu_sel = 0;
                self.mention_sel = 0;
            }
            KeyCode::Delete => self.composer.delete(),
            KeyCode::Char(c) if !ctrl => {
                self.composer.insert(c);
                self.menu_sel = 0;
                self.mention_sel = 0;
            }
            _ => {}
        }
        InputResult::None
    }

    fn command_matches(&self) -> Vec<&'static CommandDesc> {
        let Some(rest) = self.composer.text().strip_prefix('/') else {
            return Vec::new();
        };
        if rest.contains(char::is_whitespace) {
            return Vec::new();
        }
        self.commands
            .iter()
            .filter(|c| c.name.starts_with(rest))
            .collect()
    }

    fn selected_command(&self) -> Option<&'static CommandDesc> {
        let matches = self.command_matches();
        matches
            .get(self.menu_sel)
            .or_else(|| matches.first())
            .copied()
    }

    fn menu_move(&mut self, delta: isize) {
        let len = self.command_matches().len();
        if len > 0 {
            let cur = self.menu_sel.min(len - 1) as isize;
            self.menu_sel = (cur + delta).rem_euclid(len as isize) as usize;
        }
    }

    fn complete_command(&mut self) {
        if let Some(cmd) = self.selected_command() {
            let text = format!("/{}{}", cmd.name, if cmd.takes_arg { " " } else { "" });
            self.composer.clear();
            self.composer.insert_str(&text);
        }
    }

    /// True when the draft should run as a slash command rather than submit as
    /// a message. A leading `/` alone is not enough: a dragged-in absolute
    /// path (`/Users/chizi/…`) also starts with `/`. The first word must name
    /// or prefix a known command.
    fn looks_like_command(&self) -> bool {
        let text = self.composer.text().trim_start();
        let Some(rest) = text.strip_prefix('/') else {
            return false;
        };
        let first = rest.split(char::is_whitespace).next().unwrap_or("");
        // Command names are bare words; a second separator means a path.
        if first.is_empty() || first.contains(['/', '\\']) {
            return false;
        }
        self.commands.iter().any(|c| c.name.starts_with(first))
    }

    /// A typed command with args runs as-is; a bare prefix, or a bare `/`,
    /// runs whichever row the menu has highlighted.
    fn command_to_run(&self) -> String {
        let rest = self
            .composer
            .text()
            .trim_start()
            .trim_start_matches('/')
            .trim()
            .to_string();
        if rest.contains(char::is_whitespace) {
            return rest;
        }
        if self.commands.iter().any(|c| c.name == rest) {
            return rest;
        }
        self.selected_command()
            .map(|c| c.name.to_string())
            .unwrap_or(rest)
    }

    fn menu_lines(&self) -> Option<Vec<Line<'static>>> {
        let matches = self.command_matches();
        if matches.is_empty() {
            return None;
        }
        let sel = self.menu_sel.min(matches.len() - 1);
        Some(
            matches
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let style = if i == sel {
                        theme::get().selected_style()
                    } else {
                        theme::get().dimmer_style()
                    };
                    Line::from(vec![
                        Span::styled(if i == sel { "▸ " } else { "  " }, style),
                        Span::styled(format!("/{:<7}", c.name), style),
                        Span::styled(format!("  {}", c.desc), theme::get().faint_style()),
                    ])
                })
                .collect(),
        )
    }

    /// Walk the repo root, respecting `.gitignore`, and collect up to
    /// `MAX_FILE_INDEX` repo-relative paths.
    pub(super) fn build_file_index(&mut self, root: &Path) {
        let mut paths: Vec<String> = WalkBuilder::new(root)
            .git_ignore(true)
            .hidden(false)
            .build()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .filter_map(|e| {
                e.path()
                    .strip_prefix(root)
                    .ok()
                    .map(|p| p.to_string_lossy().into_owned())
            })
            .collect();
        // Sort before capping, so a big repo loses the deepest paths rather
        // than whatever the walk happened to reach last.
        paths.sort();
        paths.truncate(MAX_FILE_INDEX);
        self.file_index = paths;
    }

    /// Substring match against `file_index`, returning up to `MAX_MENTION_MATCHES`.
    fn mention_matches(&self, query: &str) -> Vec<&str> {
        if query.is_empty() {
            return self
                .file_index
                .iter()
                .take(MAX_MENTION_MATCHES)
                .map(String::as_str)
                .collect();
        }
        let lower = query.to_lowercase();
        self.file_index
            .iter()
            .filter(|p| p.to_lowercase().contains(&lower))
            .take(MAX_MENTION_MATCHES)
            .map(String::as_str)
            .collect()
    }

    fn mention_move(&mut self, delta: isize) {
        if let Some((_, query)) = self.composer.mention_context() {
            let len = self.mention_matches(query).len();
            if len > 0 {
                let cur = self.mention_sel.min(len - 1) as isize;
                self.mention_sel = (cur + delta).rem_euclid(len as isize) as usize;
            }
        }
    }

    fn complete_selected_mention(&mut self, start: usize) {
        if let Some((_, query)) = self.composer.mention_context() {
            let matches = self.mention_matches(query);
            let sel = self.mention_sel.min(matches.len().saturating_sub(1));
            if let Some(path) = matches.get(sel).map(|p| p.to_string()) {
                self.composer.complete_mention(start, &path);
            }
        }
        self.mention_sel = 0;
    }

    fn mention_lines(&self) -> Option<Vec<Line<'static>>> {
        let (_, query) = self.composer.mention_context()?;
        let matches = self.mention_matches(query);
        if matches.is_empty() {
            return None;
        }
        let sel = self.mention_sel.min(matches.len() - 1);
        Some(
            matches
                .iter()
                .enumerate()
                .map(|(i, path)| {
                    let style = if i == sel {
                        theme::get().selected_style()
                    } else {
                        theme::get().dimmer_style()
                    };
                    Line::from(vec![
                        Span::styled(if i == sel { "▸ " } else { "  " }, style),
                        Span::styled(path.to_string(), style),
                    ])
                })
                .collect(),
        )
    }

    fn as_column(&self) -> Column<'_> {
        let mut col = Column::new();
        col.push(Line::from(""));
        if let Some(view) = self.views.last() {
            col.push(Probe::new(
                Inset::new(ViewRef(view.as_ref()), Insets::tlbr(1, 1, 1, 1)),
                &self.view_area,
            ));
            return col;
        }
        if let Some(status) = &self.status {
            col.push(Inset::new(StatusRef(status), Insets::tlbr(0, 1, 1, 0)));
        }
        if let Some(menu) = self.menu_lines() {
            col.push(Probe::new(
                Inset::new(menu, Insets::tlbr(1, 1, 0, 1)),
                &self.menu_area,
            ));
        }
        if let Some(mention_menu) = self.mention_lines() {
            col.push(Probe::new(
                Inset::new(mention_menu, Insets::tlbr(1, 1, 0, 1)),
                &self.mention_area,
            ));
        }
        col.push(Inset::new(
            ComposerRef {
                composer: &self.composer,
                placeholder: self.placeholder,
                thinking: self.task_running,
            },
            Insets::vh(1, 1),
        ));
        col
    }

    /// Rows above the shaded band: the gap row and, when idle, the status row.
    fn unshaded_rows(&self, width: u16) -> u16 {
        let status = match (&self.status, self.views.is_empty()) {
            (Some(s), true) => s.desired_height(width) + 1,
            _ => 0,
        };
        1 + status
    }
}

impl<E: Clone + 'static> Renderable for BottomPane<E> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        // The gap and status rows sit on the terminal bg; everything under
        // them (views, menu, composer) sits on the shaded band, codex-style.
        let skip = self.unshaded_rows(area.width).min(area.height);
        let band = Rect {
            y: area.y + skip,
            height: area.height.saturating_sub(skip),
            ..area
        };
        buf.set_style(band, theme::get().pane_bg_style());
        self.as_column().render(area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        self.as_column().desired_height(width)
    }
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.as_column().cursor_pos(area)
    }
}

/// Borrowed adapters so `Column` can hold references without cloning.
struct ViewRef<'a>(&'a dyn Renderable);
impl Renderable for ViewRef<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.0.render(area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        self.0.desired_height(width)
    }
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.0.cursor_pos(area)
    }
}

struct StatusRef<'a>(&'a StatusWidget);
impl Renderable for StatusRef<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.0.render(area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        self.0.desired_height(width)
    }
}

struct ComposerRef<'a> {
    composer: &'a Composer,
    placeholder: &'a str,
    thinking: bool,
}

impl Renderable for ComposerRef<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let (lines, _) = self.composer.render(area.width, self.hint());
        Paragraph::new(lines).render(area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        self.composer.height(width)
    }
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        let (_, (row, col)) = self.composer.render(area.width, self.hint());
        Some((area.x + col, area.y + row))
    }
}

impl ComposerRef<'_> {
    fn hint(&self) -> &str {
        if self.thinking && self.composer.is_empty() {
            "…  (esc to interrupt)"
        } else {
            self.placeholder
        }
    }
}

#[cfg(test)]
#[path = "command_routing_tests.rs"]
mod tests;
