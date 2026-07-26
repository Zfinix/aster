use std::sync;
use std::time::Duration;

use anyhow::Result;
use aster_ai::{AiClient, ChatMessage};
use aster_persist::{MessageEvent, Store};
use aster_policy::{Mode, PermissionsConfig, Policy};
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap};
use tokio::sync::mpsc;

use super::guard::TuiGuard;
use super::helpers::{dim, draw_input_box, human_count, mark_lines, short_path};
use super::{ACCENT, SPINNER};
use crate::chat::{ApprovalRequest, ApprovalSender, SessionCtx};
use crate::persist::Recorder;

type ChatTurn = tokio::task::JoinHandle<Result<(String, Vec<String>, Option<Vec<ChatMessage>>)>>;

/// Standalone conversational chat TUI, driven from `aster chat --tui`.
pub async fn run_chat(
    mut client: AiClient,
    repo_root: std::path::PathBuf,
    allow_edits: bool,
    perms: PermissionsConfig,
    seed: Option<String>,
) -> Result<()> {
    let guard = TuiGuard::install();
    let mut terminal = ratatui::init();
    // Depth 1: the agent awaits each approval before proposing the next.
    let (approval_tx, mut approval_rx) = mpsc::channel::<ApprovalRequest>(1);
    let endpoint = crate::init::provider_label(client.base_url());
    let cwd = short_path(&repo_root);

    let policy_for = |mode: Mode| {
        let mut c = perms.clone();
        c.mode = mode;
        sync::Arc::new(Policy::compile(&c).unwrap_or_else(|_| Policy::permissive()))
    };
    let edit_mode = if !allow_edits {
        EditMode::Off
    } else if perms.mode == Mode::Auto {
        EditMode::Auto
    } else {
        EditMode::Ask
    };
    let mut app = ChatApp::new(
        edit_mode,
        client.model.clone(),
        policy_for(Mode::Ask),
        policy_for(Mode::Auto),
        approval_tx,
        endpoint,
        cwd,
    );
    app.repo_root = repo_root.clone();
    if let Ok(store) = crate::persist::store() {
        match resume_or_new(&store, &repo_root, &client.model) {
            Ok((recorder, seeded)) => {
                app.recorder = Some(recorder);
                app.load_history(seeded);
            }
            Err(e) => tracing::warn!("could not open session store: {e:#}"),
        }
        app.store = Some(store);
    }

    let mut turn: Option<ChatTurn> = None;

    if let Some(seed) = seed.filter(|s| !s.trim().is_empty()) {
        turn = Some(app.submit(&seed, &client, &repo_root));
    }

    let outcome = loop {
        terminal.draw(|frame| app.draw(frame))?;

        if app.pending_approval.is_none()
            && let Ok(req) = approval_rx.try_recv()
        {
            app.begin_approval(req);
        }

        if event::poll(Duration::from_millis(80))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            let ctrl_c = key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('c'));

            // While an approval is pending, keys answer it instead of editing input.
            if app.pending_approval.is_some() {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => app.resolve_approval(true),
                    KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                        app.resolve_approval(false)
                    }
                    _ if ctrl_c => {
                        app.resolve_approval(false);
                        if let Some(t) = &turn {
                            t.abort();
                        }
                        break Ok(());
                    }
                    _ => {}
                }
                continue;
            }

            if ctrl_c || matches!(key.code, KeyCode::Esc) {
                if let Some(t) = &turn {
                    t.abort();
                }
                break Ok(());
            }
            let menu_open = !app.command_matches().is_empty();
            match key.code {
                KeyCode::Up if menu_open => app.menu_move(-1),
                KeyCode::Down if menu_open => app.menu_move(1),
                KeyCode::Tab if menu_open => app.complete_command(),
                KeyCode::Up => app.scroll = app.scroll.saturating_add(1),
                KeyCode::Down => app.scroll = app.scroll.saturating_sub(1),
                KeyCode::PageUp => app.scroll = app.scroll.saturating_add(10),
                KeyCode::PageDown => app.scroll = app.scroll.saturating_sub(10),
                KeyCode::Home => app.scroll = u16::MAX,
                KeyCode::End => app.scroll = 0,
                KeyCode::Enter if turn.is_none() && !app.input.trim().is_empty() => {
                    // A leading slash is a local command (e.g. /model), not a message.
                    if app.input.trim_start().starts_with('/') {
                        let cmd = app.command_to_run();
                        app.input.clear();
                        app.menu_sel = 0;
                        app.handle_command(&cmd, &mut client);
                        if app.should_quit {
                            break Ok(());
                        }
                    } else {
                        let text = std::mem::take(&mut app.input);
                        turn = Some(app.submit(&text, &client, &repo_root));
                        app.scroll = 0;
                    }
                }
                KeyCode::Backspace => {
                    app.input.pop();
                    app.menu_sel = 0;
                    app.flash = None;
                }
                KeyCode::Char(c) => {
                    app.input.push(c);
                    app.menu_sel = 0;
                    app.flash = None;
                }
                _ => {}
            }
        }

        app.usage = Some(client.usage_snapshot());
        if app.thinking {
            app.spinner = (app.spinner + 1) % SPINNER.len();
        }

        if turn.as_ref().is_some_and(|t| t.is_finished()) {
            match turn.take().expect("checked is_some").await {
                Ok(Ok((reply, edited, compacted))) => app.push_reply(&reply, &edited, compacted),
                Ok(Err(e)) => app.fail_turn(&format!("{e:#}")),
                Err(e) => app.fail_turn(&format!("chat failed: {e}")),
            }
            app.thinking = false;
        }
    };

    drop(guard);
    outcome
}

/// Continue the repo's most recent session, or start a fresh one. Returns the
/// live transcript handle and the prior user/assistant turns to seed the view.
fn resume_or_new(
    store: &Store,
    repo_root: &std::path::Path,
    model: &str,
) -> Result<(Recorder, Vec<ChatMessage>)> {
    if let Some(prev) = store.latest(repo_root)? {
        let messages = prev.to_chat_messages();
        let writer = store.resume_writer(repo_root, &prev.meta.id)?;
        Ok((sync::Arc::new(sync::Mutex::new(writer)), messages))
    } else {
        let writer = store.new_session(repo_root, repo_root, Some(model.to_string()))?;
        Ok((sync::Arc::new(sync::Mutex::new(writer)), Vec::new()))
    }
}

/// How file edits are gated in chat, cycled with `/edits`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EditMode {
    Off,
    Ask,
    Auto,
}

impl EditMode {
    fn short(self) -> &'static str {
        match self {
            EditMode::Off => "off",
            EditMode::Ask => "ask",
            EditMode::Auto => "auto",
        }
    }

    fn desc(self) -> &'static str {
        match self {
            EditMode::Off => "off · read & search only",
            EditMode::Ask => "ask · confirm each edit",
            EditMode::Auto => "auto · edit without asking",
        }
    }

    fn next(self) -> Self {
        match self {
            EditMode::Off => EditMode::Ask,
            EditMode::Ask => EditMode::Auto,
            EditMode::Auto => EditMode::Off,
        }
    }
}

/// A slash command in the chat menu; `takes_arg` decides whether Tab completes
/// to `/name ` or `/name`.
struct ChatCommand {
    name: &'static str,
    takes_arg: bool,
    desc: &'static str,
}

const CHAT_COMMANDS: &[ChatCommand] = &[
    ChatCommand {
        name: "model",
        takes_arg: true,
        desc: "Switch the active model, or show it with no argument",
    },
    ChatCommand {
        name: "edits",
        takes_arg: false,
        desc: "Cycle edit gating: off → ask → auto",
    },
    ChatCommand {
        name: "clear",
        takes_arg: false,
        desc: "Clear the conversation and start fresh",
    },
    ChatCommand {
        name: "help",
        takes_arg: false,
        desc: "List the available commands",
    },
    ChatCommand {
        name: "quit",
        takes_arg: false,
        desc: "Exit the chat",
    },
];

struct ChatApp {
    lines: Vec<Line<'static>>,
    input: String,
    thinking: bool,
    spinner: usize,
    usage: Option<aster_ai::UsageSnapshot>,
    edit_mode: EditMode,
    model: String,
    history: Vec<ChatMessage>,
    store: Option<Store>,
    recorder: Option<Recorder>,
    repo_root: std::path::PathBuf,
    ask_policy: sync::Arc<Policy>,
    auto_policy: sync::Arc<Policy>,
    approval_tx: ApprovalSender,
    pending_approval: Option<ApprovalRequest>,
    menu_sel: usize,
    should_quit: bool,
    endpoint: String,
    cwd: String,
    /// Transient footer status, cleared on the next keystroke.
    flash: Option<String>,
    /// Lines scrolled up from the bottom; `0` follows the latest output. Left
    /// untouched as output streams in so reading history isn't yanked down.
    scroll: u16,
}

impl ChatApp {
    fn new(
        edit_mode: EditMode,
        model: String,
        ask_policy: sync::Arc<Policy>,
        auto_policy: sync::Arc<Policy>,
        approval_tx: ApprovalSender,
        endpoint: String,
        cwd: String,
    ) -> Self {
        Self {
            lines: Vec::new(),
            input: String::new(),
            thinking: false,
            spinner: 0,
            usage: None,
            edit_mode,
            model,
            history: Vec::new(),
            store: None,
            recorder: None,
            repo_root: std::path::PathBuf::new(),
            ask_policy,
            auto_policy,
            approval_tx,
            pending_approval: None,
            menu_sel: 0,
            should_quit: false,
            endpoint,
            cwd,
            flash: None,
            scroll: 0,
        }
    }

    fn edits_enabled(&self) -> bool {
        self.edit_mode != EditMode::Off
    }

    /// `auto` applies writes directly; everything else prompts for approval.
    fn turn_policy(&self) -> sync::Arc<Policy> {
        match self.edit_mode {
            EditMode::Auto => self.auto_policy.clone(),
            _ => self.ask_policy.clone(),
        }
    }

    /// Slash commands matching the current input; empty when the menu shouldn't
    /// show (no leading `/`, or an argument is being typed).
    fn command_matches(&self) -> Vec<&'static ChatCommand> {
        let Some(rest) = self.input.strip_prefix('/') else {
            return Vec::new();
        };
        if rest.contains(char::is_whitespace) {
            return Vec::new();
        }
        CHAT_COMMANDS
            .iter()
            .filter(|c| c.name.starts_with(rest))
            .collect()
    }

    fn selected_command(&self) -> Option<&'static ChatCommand> {
        let matches = self.command_matches();
        matches
            .get(self.menu_sel)
            .or_else(|| matches.first())
            .copied()
    }

    fn menu_move(&mut self, delta: isize) {
        let len = self.command_matches().len();
        if len == 0 {
            return;
        }
        let cur = self.menu_sel.min(len - 1) as isize;
        self.menu_sel = (cur + delta).rem_euclid(len as isize) as usize;
    }

    /// Complete input to the highlighted command, with a trailing space when it
    /// takes an argument.
    fn complete_command(&mut self) {
        if let Some(cmd) = self.selected_command() {
            self.input = format!("/{}{}", cmd.name, if cmd.takes_arg { " " } else { "" });
        }
    }

    /// Command to run on Enter: a typed command with args runs as-is; a bare
    /// prefix runs the highlighted menu entry.
    fn command_to_run(&self) -> String {
        let rest = self.input.trim_start_matches('/').trim().to_string();
        if rest.contains(char::is_whitespace) {
            return rest;
        }
        self.selected_command()
            .map(|c| c.name.to_string())
            .unwrap_or(rest)
    }

    fn begin_approval(&mut self, req: ApprovalRequest) {
        for line in req.preview.lines() {
            self.push_system(line);
        }
        self.push_system("apply this edit? [y/n]");
        self.pending_approval = Some(req);
    }

    fn resolve_approval(&mut self, approved: bool) {
        if let Some(req) = self.pending_approval.take() {
            let _ = req.respond.send(approved);
            self.push_system(if approved {
                "edit approved"
            } else {
                "edit rejected"
            });
        }
    }

    /// A model change takes effect next turn, since each turn clones the client.
    fn handle_command(&mut self, cmd: &str, client: &mut AiClient) {
        let mut parts = cmd.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("");
        let arg = parts.next().map(str::trim).filter(|s| !s.is_empty());
        match name {
            "model" | "m" => match arg {
                Some(model) => {
                    client.model = model.to_string();
                    self.model = model.to_string();
                    self.push_system(&format!("model set to {model}"));
                }
                None => self.push_system(&format!("current model: {}", self.model)),
            },
            "edits" | "e" => {
                self.edit_mode = self.edit_mode.next();
                // A footer flash, not a scrollback line, so the view doesn't jump.
                self.flash = Some(format!("edits {}", self.edit_mode.desc()));
            }
            "clear" | "c" => {
                self.lines.clear();
                self.history.clear();
                self.start_new_session();
                self.push_system("conversation cleared");
            }
            "help" | "h" => {
                self.push_system("commands:");
                for c in CHAT_COMMANDS {
                    self.push_system(&format!("  /{:<7} {}", c.name, c.desc));
                }
            }
            "quit" | "q" | "exit" => self.should_quit = true,
            other => self.push_system(&format!("unknown command: /{other} (try /help)")),
        }
    }

    fn push_system(&mut self, msg: &str) {
        self.lines.push(Line::from(Span::styled(
            format!("  · {msg}"),
            Style::default().fg(ACCENT),
        )));
    }

    fn submit(&mut self, text: &str, client: &AiClient, repo_root: &std::path::Path) -> ChatTurn {
        self.push_user(text);
        self.history.push(ChatMessage {
            role: "user".into(),
            content: text.into(),
        });
        self.record_user(text);
        self.thinking = true;
        let client = client.clone();
        let repo_root = repo_root.to_path_buf();
        let history = self.history.clone();
        let allow_edits = self.edits_enabled();
        let policy = self.turn_policy();
        let approver = Some(self.approval_tx.clone());
        let ctx = SessionCtx {
            recorder: self.recorder.clone(),
            store: self.store.clone(),
            skills: crate::chat::discover_skills(&repo_root),
        };
        tokio::spawn(async move {
            crate::chat::agent_turn(
                client,
                repo_root,
                history,
                allow_edits,
                policy,
                approver,
                ctx,
            )
            .await
        })
    }

    fn record_user(&self, text: &str) {
        let Some(recorder) = &self.recorder else {
            return;
        };
        if let Ok(mut writer) = recorder.lock()
            && let Err(e) = writer.append_message(MessageEvent::user(text))
        {
            tracing::warn!("failed to record user turn: {e:#}");
        }
    }

    /// Replay a resumed transcript into the view and in-memory history. The
    /// assistant turns are already recorded on disk, so nothing is re-appended.
    fn load_history(&mut self, messages: Vec<ChatMessage>) {
        if messages.is_empty() {
            return;
        }
        let turns = messages.iter().filter(|m| m.role == "user").count();
        for m in &messages {
            match m.role.as_str() {
                "user" => self.push_user(&m.content),
                "assistant" => self.render_assistant(&m.content),
                _ => {}
            }
        }
        self.history = messages;
        self.push_system(&format!(
            "resumed {turns} previous turn(s) · /clear to start fresh"
        ));
    }

    fn start_new_session(&mut self) {
        let Some(store) = &self.store else {
            return;
        };
        match store.new_session(&self.repo_root, &self.repo_root, Some(self.model.clone())) {
            Ok(writer) => self.recorder = Some(sync::Arc::new(sync::Mutex::new(writer))),
            Err(e) => tracing::warn!("failed to start a new session: {e:#}"),
        }
    }

    fn push_user(&mut self, text: &str) {
        self.lines.push(Line::from(""));
        self.lines.push(Line::from(vec![
            Span::styled("❯ ", Style::default().fg(ACCENT)),
            Span::styled(
                text.to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    fn push_reply(&mut self, reply: &str, edited: &[String], compacted: Option<Vec<ChatMessage>>) {
        if let Some(compacted) = compacted {
            self.history = compacted;
            self.push_system("compacted earlier turns to save context");
        }
        self.history.push(ChatMessage {
            role: "assistant".into(),
            content: reply.into(),
        });
        self.render_assistant(reply);
        for path in edited {
            self.lines.push(Line::from(Span::styled(
                format!("  ✎ edited {path}"),
                Style::default().fg(ACCENT),
            )));
        }
    }

    fn render_assistant(&mut self, reply: &str) {
        self.lines.push(Line::from(""));
        for (i, l) in reply.lines().enumerate() {
            if i == 0 {
                self.lines.push(Line::from(vec![
                    Span::styled("✳ ", Style::default().fg(Color::Green)),
                    Span::raw(l.to_string()),
                ]));
            } else {
                self.lines.push(Line::from(Span::raw(l.to_string())));
            }
        }
    }

    fn push_error(&mut self, msg: &str) {
        self.lines.push(Line::from(Span::styled(
            format!("  ! {msg}"),
            Style::default().fg(Color::Red),
        )));
    }

    /// Drop the unanswered question from history so a retry resends it instead
    /// of stacking a duplicate user turn.
    fn fail_turn(&mut self, msg: &str) {
        if self.history.last().is_some_and(|m| m.role == "user") {
            self.history.pop();
        }
        self.push_error(msg);
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        let rows = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(area);
        let body = rows[0];
        let input = rows[1];
        let footer = rows[2];

        if self.lines.is_empty() {
            self.draw_welcome(frame, body);
        } else {
            let visible = body.height as usize;
            let mut content = self.lines.clone();
            if self.thinking {
                content.push(Line::from(""));
                content.push(Line::from(vec![
                    Span::styled(
                        format!("{} ", SPINNER[self.spinner]),
                        Style::default().fg(Color::Green),
                    ),
                    Span::styled("thinking…", Style::default().fg(Color::DarkGray)),
                ]));
            }
            let max_scroll = content.len().saturating_sub(visible) as u16;
            // Re-pin to the bottom as the range shrinks (e.g. after `/clear`).
            self.scroll = self.scroll.min(max_scroll);
            let scroll = max_scroll - self.scroll;
            frame.render_widget(
                Paragraph::new(content)
                    .block(Block::default().padding(Padding::horizontal(1)))
                    .wrap(Wrap { trim: false })
                    .scroll((scroll, 0)),
                body,
            );
        }

        draw_input_box(
            frame,
            input,
            &self.input,
            false,
            self.spinner,
            "Message Aster…",
        );

        // Drawn last so the menu wins over the input box below it.
        if !self.thinking && self.pending_approval.is_none() {
            self.draw_command_menu(frame, input);
        }

        let usage = self
            .usage
            .filter(|u| u.total_tokens > 0)
            .map(|u| {
                let approx = if u.estimated { "~" } else { "" };
                let cost = u
                    .estimated_cost_usd
                    .map(|c| format!("  ·  ~${c:.4}"))
                    .unwrap_or_default();
                format!(
                    "  ctx {approx}{} in / {approx}{} out{cost}",
                    human_count(u.prompt_tokens as usize),
                    human_count(u.completion_tokens as usize),
                )
            })
            .unwrap_or_default();
        let hint = if self.thinking {
            "esc to quit"
        } else {
            "enter to send  ·  esc to quit"
        };
        let dark = Style::default().fg(Color::DarkGray);
        let edit_color = match self.edit_mode {
            EditMode::Auto => Color::Green,
            EditMode::Ask => Color::Yellow,
            EditMode::Off => Color::DarkGray,
        };
        let mut spans = vec![Span::styled(
            format!("  ✎ edits {}", self.edit_mode.short()),
            Style::default().fg(edit_color),
        )];
        if !usage.is_empty() {
            spans.push(Span::styled(format!("  ·{usage}"), dark));
        }
        spans.push(Span::styled("  ·  ", dark));
        // A transient status (e.g. after /edits) replaces the hint until the next key.
        match &self.flash {
            Some(msg) => spans.push(Span::styled(msg.clone(), Style::default().fg(ACCENT))),
            None => spans.push(Span::styled(hint.to_string(), dark)),
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), footer);
    }

    fn welcome_lines(&self) -> Vec<Line<'static>> {
        let field = |k: &str, v: String| {
            Line::from(vec![
                Span::styled(format!("{k:<9}"), Style::default().fg(Color::DarkGray)),
                Span::raw(v),
            ])
        };

        let mut lines: Vec<Line<'static>> = mark_lines();
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                "Aster",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ·  AI code review  ·  v{}", env!("CARGO_PKG_VERSION")),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        lines.push(Line::from(""));
        lines.push(field("model", self.model.clone()));
        lines.push(field("provider", self.endpoint.clone()));
        lines.push(field("cwd", self.cwd.clone()));
        lines.push(field("mode", self.edit_mode.desc().into()));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Getting started",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));
        lines.push(dim("• Ask about a file, a diff, or anything in this repo"));
        lines.push(dim("• Type  /  to browse commands (model, edits, clear…)"));
        lines
    }

    fn draw_welcome(&self, frame: &mut Frame, body: Rect) {
        let lines = self.welcome_lines();
        let content_w = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
        let width = (content_w + 4).min(body.width);
        let height = (lines.len() as u16 + 2).min(body.height);
        let card = Rect {
            x: body.x,
            y: body.y,
            width,
            height,
        };
        frame.render_widget(Clear, card);
        frame.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray))
                    .padding(Padding::horizontal(1)),
            ),
            card,
        );
    }

    fn draw_command_menu(&self, frame: &mut Frame, input: Rect) {
        let matches = self.command_matches();
        if matches.is_empty() {
            return;
        }
        let sel = self.menu_sel.min(matches.len() - 1);

        let rows: Vec<Line> = matches
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let active = i == sel;
                let name_style = if active {
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                };
                Line::from(vec![
                    Span::styled(if active { " › " } else { "   " }, name_style),
                    Span::styled(format!("/{:<7}", c.name), name_style),
                    Span::styled(
                        format!("  {}", c.desc),
                        Style::default().fg(Color::DarkGray),
                    ),
                ])
            })
            .collect();

        let height = matches.len() as u16 + 2;
        let width = input.width;
        let y = input.y.saturating_sub(height);
        let popup = Rect {
            x: input.x,
            y,
            width,
            height,
        };

        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(rows).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(ACCENT))
                    .title(" commands ")
                    .title_style(Style::default().fg(Color::DarkGray))
                    .padding(Padding::horizontal(1)),
            ),
            popup,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat_app(model: String) -> ChatApp {
        let (tx, _rx) = mpsc::channel(1);
        ChatApp::new(
            EditMode::Off,
            model,
            sync::Arc::new(Policy::permissive()),
            sync::Arc::new(Policy::permissive()),
            tx,
            "OpenRouter".into(),
            "~/repo".into(),
        )
    }

    #[test]
    fn chat_command_model_switches_client_and_app() {
        let mut client = AiClient::new("http://localhost", "k", "openai/gpt-4o-mini");
        let mut app = chat_app(client.model.clone());
        app.handle_command("model anthropic/claude-sonnet-5", &mut client);
        assert_eq!(client.model, "anthropic/claude-sonnet-5");
        assert_eq!(app.model, "anthropic/claude-sonnet-5");
    }

    #[test]
    fn chat_command_model_without_arg_keeps_model() {
        let mut client = AiClient::new("http://localhost", "k", "m1");
        let mut app = chat_app(client.model.clone());
        app.handle_command("model", &mut client);
        assert_eq!(client.model, "m1");
        assert_eq!(app.model, "m1");
    }

    #[test]
    fn chat_command_unknown_is_reported() {
        let mut client = AiClient::new("http://localhost", "k", "m1");
        let mut app = chat_app(client.model.clone());
        let before = app.lines.len();
        app.handle_command("bogus", &mut client);
        assert!(app.lines.len() > before);
    }

    #[test]
    fn resume_seeds_history_from_prior_session() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::open(home.path()).unwrap();
        let repo = std::path::Path::new("/tmp/aster-resume-repo");
        {
            let mut w = store.new_session(repo, repo, Some("m".into())).unwrap();
            w.append_message(MessageEvent::user("hello")).unwrap();
            w.append_message(MessageEvent::assistant(Some("hi there".into()), vec![]))
                .unwrap();
        }

        let (recorder, messages) = resume_or_new(&store, repo, "m").unwrap();
        assert_eq!(messages.len(), 2);

        let mut app = chat_app("m".into());
        app.store = Some(store);
        app.repo_root = repo.to_path_buf();
        app.recorder = Some(recorder);
        app.load_history(messages);
        assert_eq!(app.history.len(), 2);
        assert!(!app.lines.is_empty());
    }

    #[test]
    fn record_user_persists_turn() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::open(home.path()).unwrap();
        let repo = std::path::Path::new("/tmp/aster-record-repo");
        let (recorder, _) = resume_or_new(&store, repo, "m").unwrap();

        let mut app = chat_app("m".into());
        app.store = Some(store.clone());
        app.repo_root = repo.to_path_buf();
        app.recorder = Some(recorder);
        app.record_user("remember me");

        let latest = store.latest(repo).unwrap().unwrap();
        let persisted = latest.events.iter().any(|e| {
            matches!(e, aster_persist::TranscriptEvent::Message(m)
                if m.role == "user" && m.content.as_deref() == Some("remember me"))
        });
        assert!(persisted);
    }
}
