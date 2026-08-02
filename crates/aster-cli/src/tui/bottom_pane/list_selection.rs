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
            selected,
            complete: false,
            tx,
        }
    }

    fn accept(&mut self, index: usize) {
        if let Some(item) = self.items.get(index) {
            let _ = self.tx.send(item.event.clone());
        }
        self.complete = true;
    }

    fn move_by(&mut self, delta: isize) {
        let len = self.items.len();
        if len > 0 {
            let cur = self.selected as isize;
            self.selected = (cur + delta).rem_euclid(len as isize) as usize;
        }
    }

    fn lines(&self) -> Vec<Line<'static>> {
        let mut out = vec![Line::from(Span::styled(
            self.title.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ))];
        out.push(Line::from(""));
        let start = window_start(self.selected, self.items.len());
        let shown = self.items.iter().enumerate().skip(start).take(VISIBLE_ROWS);
        let name_w = shown.clone().map(|(_, i)| i.name.len()).max().unwrap_or(0);
        for (i, item) in shown {
            let active = i == self.selected;
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
                Span::styled(
                    format!("  {}", item.description),
                    theme::get().faint_style(),
                ),
            ]));
        }
        if self.items.len() > VISIBLE_ROWS {
            out.push(Line::from(Span::styled(
                format!("  {} of {}", self.selected + 1, self.items.len()),
                theme::get().faint_style(),
            )));
        }
        out.push(Line::from(""));
        out.push(Line::from(Span::styled(
            "enter confirm · esc cancel",
            theme::get().faint_style(),
        )));
        out
    }

    fn renderable(&self) -> impl Renderable {
        Inset::new(wrapped(self.lines()), Insets::tlbr(0, 0, 0, 0))
    }

    /// Which item sits on `row` of the rendered view. `lines` puts the title on
    /// row 0 and a blank on row 1, so the window starts at row 2.
    fn item_at(&self, row: u16) -> Option<usize> {
        const FIRST_ROW: u16 = 2;
        let offset = row.checked_sub(FIRST_ROW)? as usize;
        if offset >= VISIBLE_ROWS {
            return None;
        }
        let index = window_start(self.selected, self.items.len()) + offset;
        (index < self.items.len()).then_some(index)
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
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::Enter => self.accept(self.selected),
            KeyCode::Esc => self.complete = true,
            KeyCode::Char(c) => {
                if let Some(n) = c.to_digit(10)
                    && n >= 1
                    && (n as usize) <= self.items.len()
                {
                    self.accept(n as usize - 1);
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
        assert!(long <= short + 7, "short {short}, long {long}");
        assert!(long < 20, "the pane would eat the screen: {long}");
    }

    #[test]
    fn the_window_follows_the_selection() {
        let mut v = long_view(400);
        v.selected = 300;
        let shown = v.lines();
        assert!(
            shown.iter().any(|l| l.to_string().contains("session-300")),
            "selection scrolled off: {shown:?}"
        );
        assert!(shown.iter().any(|l| l.to_string().contains("301 of 400")));
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
