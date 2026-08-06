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
}

impl<E: Clone> ListSelectionView<E> {
    pub(crate) fn new(
        title: impl Into<String>,
        items: Vec<SelectionItem<E>>,
        tx: mpsc::UnboundedSender<E>,
    ) -> Self {
        let selected = items.iter().position(|i| i.is_current).unwrap_or(0);
        Self {
            title: title.into(),
            items,
            query: String::new(),
            selected,
            complete: false,
            tx,
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
            KeyCode::Esc => self.complete = true,
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
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn view(tx: mpsc::UnboundedSender<u8>) -> ListSelectionView<u8> {
        let items = vec![
            SelectionItem {
                name: "one".into(),
                description: "first".into(),
                is_current: false,
                event: 1,
            },
            SelectionItem {
                name: "two".into(),
                description: "second".into(),
                is_current: true,
                event: 2,
            },
            SelectionItem {
                name: "three".into(),
                description: "third".into(),
                is_current: false,
                event: 3,
            },
        ];
        ListSelectionView::new("Pick", items, tx)
    }

    fn long_view(count: usize) -> ListSelectionView<u8> {
        let (tx, _rx) = mpsc::unbounded_channel();
        let items = (0..count)
            .map(|i| SelectionItem {
                name: format!("session-{i}"),
                description: "a saved session".into(),
                is_current: false,
                event: 0,
            })
            .collect();
        ListSelectionView::new("Resume", items, tx)
    }

    #[test]
    fn height_does_not_grow_with_the_item_count() {
        let short = long_view(3).desired_height(80);
        let long = long_view(400).desired_height(80);
        assert!(
            long <= short + VISIBLE_ROWS as u16,
            "short {short}, long {long}"
        );
        assert!(long < 20, "the pane would eat the screen: {long}");
    }

    #[test]
    fn the_window_follows_the_selection_and_counts_the_rest() {
        let mut v = long_view(400);
        v.selected = 300;
        let shown = v.lines();
        assert!(
            shown.iter().any(|l| l.to_string().contains("session-300")),
            "selection scrolled off: {shown:?}"
        );
        assert!(
            shown.iter().any(|l| l.to_string().contains("+390 more")),
            "{shown:?}"
        );
    }

    #[test]
    fn opens_on_the_current_item_and_wraps() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut v = view(tx);
        assert_eq!(v.selected, 1);
        v.handle_key(key(KeyCode::Down));
        v.handle_key(key(KeyCode::Down));
        assert_eq!(v.selected, 0);
        v.handle_key(key(KeyCode::Enter));
        assert!(v.is_complete());
        assert_eq!(rx.try_recv().unwrap(), 1);
    }

    #[test]
    fn digits_select_and_accept_in_one_press() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut v = view(tx);
        v.handle_key(key(KeyCode::Char('3')));
        assert!(v.is_complete());
        assert_eq!(rx.try_recv().unwrap(), 3);
    }

    #[test]
    fn typing_filters_the_rows_and_enter_accepts_the_survivor() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut v = view(tx);
        v.handle_key(key(KeyCode::Char('t')));
        v.handle_key(key(KeyCode::Char('h')));
        v.handle_key(key(KeyCode::Enter));
        assert!(v.is_complete());
        assert_eq!(rx.try_recv().unwrap(), 3);
    }

    #[test]
    fn backspace_widens_the_filter_again() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut v = view(tx);
        v.handle_key(key(KeyCode::Char('t')));
        v.handle_key(key(KeyCode::Char('h')));
        assert_eq!(v.filtered().len(), 1);
        v.handle_key(key(KeyCode::Backspace));
        assert_eq!(v.filtered().len(), 2, "t matches two and three");
    }

    #[test]
    fn y_and_n_answer_a_yes_no_list_without_enter() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let items = vec![
            SelectionItem {
                name: "No, keep it".into(),
                description: String::new(),
                is_current: true,
                event: 0u8,
            },
            SelectionItem {
                name: "Yes, delete it".into(),
                description: String::new(),
                is_current: false,
                event: 1,
            },
        ];
        let mut v = ListSelectionView::new("Delete?", items, tx);
        v.handle_key(key(KeyCode::Char('y')));
        assert!(v.is_complete());
        assert_eq!(rx.try_recv().unwrap(), 1);
    }

    #[test]
    fn esc_cancels_without_sending() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut v = view(tx);
        v.handle_key(key(KeyCode::Esc));
        assert!(v.is_complete());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn out_of_range_digit_does_nothing() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut v = view(tx);
        v.handle_key(key(KeyCode::Char('9')));
        assert!(!v.is_complete());
        assert!(rx.try_recv().is_err());
    }
}
