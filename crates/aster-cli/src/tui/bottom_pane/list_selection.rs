//! Generic borderless picker: title, numbered rows, dim footer hint.
//! Backs the /mode, /model and /effort menus.

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use tokio::sync::mpsc;

use super::view::BottomPaneView;
use super::{VISIBLE_ROWS, window_start};
use crate::tui::render::{Inset, Insets, Renderable, wrapped};
use crate::tui::theme;

pub(crate) struct SelectionItem<E> {
    pub name: String,
    pub description: String,
    pub is_current: bool,
    /// Sent to the app when this row is accepted.
    pub event: E,
}

pub(crate) struct ListSelectionView<E> {
    title: String,
    items: Vec<SelectionItem<E>>,
    /// Typed filter; rows whose name contains it are the ones shown.
    query: String,
    /// Index into the filtered rows, not `items`.
    selected: usize,
    complete: bool,
    tx: mpsc::UnboundedSender<E>,
    /// Sent on Esc when set; otherwise the view just closes silently.
    on_dismiss: Option<E>,
}

impl<E: Clone> ListSelectionView<E> {
    pub(crate) fn new(
        title: impl Into<String>,
        items: Vec<SelectionItem<E>>,
        tx: mpsc::UnboundedSender<E>,
        on_dismiss: Option<E>,
    ) -> Self {
        let selected = items.iter().position(|i| i.is_current).unwrap_or(0);
        Self {
            title: title.into(),
            items,
            query: String::new(),
            selected,
            complete: false,
            tx,
            on_dismiss,
        }
    }

    fn filtered(&self) -> Vec<&SelectionItem<E>> {
        let query = self.query.to_lowercase();
        self.items
            .iter()
            .filter(|i| i.name.to_lowercase().contains(&query))
            .collect()
    }

    fn accept(&mut self, index: usize) {
        if let Some(item) = self.filtered().get(index) {
            let _ = self.tx.send(item.event.clone());
            self.complete = true;
        }
    }

    fn move_by(&mut self, delta: isize) {
        let len = self.filtered().len();
        if len > 0 {
            let cur = (self.selected.min(len - 1)) as isize;
            self.selected = (cur + delta).rem_euclid(len as isize) as usize;
        }
    }

    fn push_query(&mut self, c: char) {
        self.query.push(c);
        self.selected = 0;
    }

    /// `y`/`n` answer a two-option yes/no list outright, the way a prompt
    /// expects; on any other list those letters just filter.
    fn yes_no_shortcut(&self, c: char) -> Option<usize> {
        if !self.query.is_empty() || self.items.len() != 2 {
            return None;
        }
        let starts = |i: usize, s: &str| self.items[i].name.to_lowercase().starts_with(s);
        let (yes, no) = if starts(0, "y") && starts(1, "n") {
            (0, 1)
        } else if starts(0, "n") && starts(1, "y") {
            (1, 0)
        } else {
            return None;
        };
        match c.to_ascii_lowercase() {
            'y' => Some(yes),
            'n' => Some(no),
            _ => None,
        }
    }

    fn lines(&self) -> Vec<Line<'static>> {
        let mut out = vec![Line::from(Span::styled(
            self.title.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ))];
        out.push(Line::from(""));
        if !self.query.is_empty() {
            out.push(Line::from(vec![
                Span::styled("> ", theme::get().dimmer_style()),
                Span::raw(self.query.clone()),
            ]));
        }
        let filtered = self.filtered();
        if filtered.is_empty() {
            out.push(Line::from(Span::styled(
                "  no matches",
                theme::get().faint_style(),
            )));
        }
        let sel = self.selected.min(filtered.len().saturating_sub(1));
        let start = window_start(sel, filtered.len());
        let shown = filtered.iter().enumerate().skip(start).take(VISIBLE_ROWS);
        let name_w = shown.clone().map(|(_, i)| i.name.len()).max().unwrap_or(0);
        for (i, item) in shown {
            let active = i == sel;
            let style = if active {
                theme::get().selected_style()
            } else {
                theme::get().dimmer_style()
            };
            let current = if item.is_current { " (current)" } else { "" };
            out.push(Line::from(vec![
                Span::styled(if active { "▸ " } else { "  " }, style),
                Span::styled(format!("{}. ", i + 1), style),
                Span::styled(format!("{:<name_w$}{current}", item.name), style),
                Span::styled(format!("  {}", item.description), theme::get().text_style()),
            ]));
        }
        if filtered.len() > VISIBLE_ROWS {
            out.push(Line::from(Span::styled(
                format!("  +{} more", filtered.len() - VISIBLE_ROWS),
                theme::get().faint_style(),
            )));
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

    /// Which item sits on `row` of the rendered view. `lines` puts the title
    /// on row 0 and a blank on row 1; a typed query adds one more row.
    fn item_at(&self, row: u16) -> Option<usize> {
        let first_row = 2 + u16::from(!self.query.is_empty());
        let offset = row.checked_sub(first_row)? as usize;
        if offset >= VISIBLE_ROWS {
            return None;
        }
        let len = self.filtered().len();
        let sel = self.selected.min(len.saturating_sub(1));
        let index = window_start(sel, len) + offset;
        (index < len).then_some(index)
    }
}

impl<E: Clone> Renderable for ListSelectionView<E> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.renderable().render(area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        self.renderable().desired_height(width)
    }
}

impl<E: Clone> BottomPaneView<E> for ListSelectionView<E> {
    fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => self.move_by(-1),
            KeyCode::Down => self.move_by(1),
            KeyCode::Enter => self.accept(self.selected),
            KeyCode::Esc => {
                if let Some(event) = self.on_dismiss.take() {
                    let _ = self.tx.send(event);
                }
                self.complete = true;
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.selected = 0;
            }
            // A digit on a clean query is a quick pick and `y`/`n` answer a
            // yes/no list; anything else typed filters the rows the way the
            // model picker does.
            KeyCode::Char(c) => {
                if let Some(index) = self.yes_no_shortcut(c) {
                    self.accept(index);
                } else if self.query.is_empty()
                    && let Some(n) = c.to_digit(10)
                    && n >= 1
                    && (n as usize) <= self.filtered().len()
                {
                    self.accept(n as usize - 1);
                } else {
                    self.push_query(c);
                }
            }
            _ => {}
        }
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    /// One click picks the row outright, the way the number keys do.
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
#[path = "tests/list_selection_test.rs"]
mod tests;
