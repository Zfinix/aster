//! One panel for the session's knobs: thinking, mode, effort, model, provider. It
//! replaces the separate /mode, /effort and /provider pickers, so switching any of
//! them is one list instead of three.

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use tokio::sync::mpsc;

use super::view::BottomPaneView;
use super::window_start_of;
use crate::tui::render::{Inset, Insets, Renderable, wrapped};
use crate::tui::theme;

/// Item rows plus their headers; a taller panel would push the composer off a
/// short terminal.
const PANEL_ROWS: usize = 18;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnifiedSection {
    Options,
    Mode,
    Effort,
    Model,
    Provider,
}

impl UnifiedSection {
    fn title(self) -> &'static str {
        match self {
            Self::Options => "OPTIONS",
            Self::Mode => "MODE",
            Self::Effort => "EFFORT",
            Self::Model => "MODEL",
            Self::Provider => "PROVIDER",
        }
    }
}

pub(crate) struct UnifiedItem<E> {
    pub section: UnifiedSection,
    pub name: String,
    pub description: String,
    /// Ticked in the list: the mode in force, the effort in use, and so on.
    pub is_current: bool,
    /// Sent to the app when this row is accepted.
    pub event: E,
}

/// A rendered row: either a section header or one of `items`.
enum Row {
    Header(UnifiedSection),
    Item(usize),
}

pub(crate) struct UnifiedSelector<E> {
    items: Vec<UnifiedItem<E>>,
    /// Typed filter; rows whose name contains it are the ones shown.
    query: String,
    selected: usize,
    complete: bool,
    tx: mpsc::UnboundedSender<E>,
}

impl<E: Clone> UnifiedSelector<E> {
    pub(crate) fn new(items: Vec<UnifiedItem<E>>, tx: mpsc::UnboundedSender<E>) -> Self {
        let selected = items.iter().position(|i| i.is_current).unwrap_or(0);
        Self {
            items,
            query: String::new(),
            selected,
            complete: false,
            tx,
        }
    }

    fn matches(&self, item: &UnifiedItem<E>) -> bool {
        item.name
            .to_lowercase()
            .contains(&self.query.to_lowercase())
    }

    fn filtered(&self) -> Vec<usize> {
        (0..self.items.len())
            .filter(|i| self.matches(&self.items[*i]))
            .collect()
    }

    fn accept(&mut self, index: usize) {
        if let Some(item) = self.items.get(index) {
            let _ = self.tx.send(item.event.clone());
            self.complete = true;
        }
    }

    /// Keep the selection on a row the filter still shows.
    fn reselect(&mut self) {
        let stale = self
            .items
            .get(self.selected)
            .is_none_or(|i| !self.matches(i));
        if stale && let Some(first) = self.filtered().first() {
            self.selected = *first;
        }
    }

    fn move_by(&mut self, delta: isize) {
        let shown = self.filtered();
        if shown.is_empty() {
            return;
        }
        let at = shown.iter().position(|i| *i == self.selected).unwrap_or(0) as isize;
        let next = (at + delta).rem_euclid(shown.len() as isize) as usize;
        self.selected = shown[next];
    }

    /// Headers interleaved with the rows that survived the filter.
    fn rows(&self) -> Vec<Row> {
        let mut out = Vec::new();
        let mut section = None;
        for index in self.filtered() {
            let item = &self.items[index];
            if section != Some(item.section) {
                section = Some(item.section);
                out.push(Row::Header(item.section));
            }
            out.push(Row::Item(index));
        }
        out
    }

    /// The window of rows on screen, kept around the selection so a long list
    /// scrolls instead of growing the panel.
    fn window(&self) -> (Vec<Row>, usize) {
        let rows = self.rows();
        let at = rows
            .iter()
            .position(|r| matches!(r, Row::Item(i) if *i == self.selected))
            .unwrap_or(0);
        let start = window_start_of(at, rows.len(), PANEL_ROWS);
        let shown = rows.into_iter().skip(start).take(PANEL_ROWS).collect();
        (shown, start)
    }

    fn lines(&self) -> Vec<Line<'static>> {
        let mut out = vec![Line::from(vec![
            Span::styled("> ", theme::get().dimmer_style()),
            match self.query.is_empty() {
                true => Span::styled("search", theme::get().faint_style()),
                false => Span::raw(self.query.clone()),
            },
        ])];
        let (rows, _) = self.window();
        if rows.is_empty() {
            out.push(Line::from(Span::styled(
                "  no matches",
                theme::get().faint_style(),
            )));
        }
        for row in &rows {
            match row {
                Row::Header(section) => out.push(Line::from(Span::styled(
                    section.title(),
                    theme::get().faint_style().add_modifier(Modifier::BOLD),
                ))),
                Row::Item(index) => {
                    let item = &self.items[*index];
                    let active = *index == self.selected;
                    let style = match active {
                        true => theme::get().selected_style(),
                        false => theme::get().dimmer_style(),
                    };
                    out.push(Line::from(vec![
                        Span::styled(if active { "▸ " } else { "  " }, style),
                        Span::styled(item.name.clone(), style),
                        Span::styled(format!("  {}", item.description), theme::get().text_style()),
                        Span::styled(
                            if item.is_current { "  ✓" } else { "" },
                            theme::get().accent_style(),
                        ),
                    ]));
                }
            }
        }
        out.push(Line::from(""));
        out.push(Line::from(Span::styled(
            "enter confirm · esc cancel · type to filter",
            theme::get().faint_style(),
        )));
        out
    }

    fn renderable(&self) -> impl Renderable {
        Inset::new(wrapped(self.lines()), Insets::tlbr(0, 0, 0, 0))
    }

    /// Which item sits on `row` of the rendered panel. `lines` opens with the
    /// filter row, so the windowed rows start one below it.
    fn item_at(&self, row: u16) -> Option<usize> {
        let offset = row.checked_sub(1)? as usize;
        match self.window().0.get(offset)? {
            Row::Item(index) => Some(*index),
            Row::Header(_) => None,
        }
    }
}

impl<E: Clone> Renderable for UnifiedSelector<E> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.renderable().render(area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        self.renderable().desired_height(width)
    }
}

impl<E: Clone> BottomPaneView<E> for UnifiedSelector<E> {
    fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => self.move_by(-1),
            KeyCode::Down => self.move_by(1),
            KeyCode::Enter => self.accept(self.selected),
            KeyCode::Esc => self.complete = true,
            KeyCode::Backspace => {
                self.query.pop();
                self.reselect();
            }
            KeyCode::Char(c) => {
                self.query.push(c);
                self.reselect();
            }
            _ => {}
        }
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    /// One click picks the row outright, the way enter does.
    fn handle_click(&mut self, row: u16) -> bool {
        match self.item_at(row) {
            Some(index) => {
                self.selected = index;
                self.accept(index);
                true
            }
            None => false,
        }
    }

    fn handle_scroll(&mut self, delta: isize) {
        self.move_by(delta);
    }
}

#[cfg(test)]
#[path = "tests/unified_selector_test.rs"]
mod tests;
