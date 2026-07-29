//! Generic borderless picker: title, numbered rows, dim footer hint.
//! Backs the /mode, /model and /effort menus.

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use tokio::sync::mpsc;

use super::view::BottomPaneView;
use crate::tui::render::{Inset, Insets, Renderable, wrapped};
use crate::tui::{ACCENT, theme};

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
        let name_w = self.items.iter().map(|i| i.name.len()).max().unwrap_or(0);
        for (i, item) in self.items.iter().enumerate() {
            let active = i == self.selected;
            let style = if active {
                Style::default().fg(ACCENT).bg(theme::SEL_BG)
            } else {
                theme::dimmer()
            };
            let current = if item.is_current { " (current)" } else { "" };
            out.push(Line::from(vec![
                Span::styled(if active { "▸ " } else { "  " }, style),
                Span::styled(format!("{}. ", i + 1), style),
                Span::styled(format!("{:<name_w$}{current}", item.name), style),
                Span::styled(format!("  {}", item.description), theme::faint()),
            ]));
        }
        out.push(Line::from(""));
        out.push(Line::from(Span::styled(
            "enter confirm · esc cancel",
            theme::faint(),
        )));
        out
    }

    fn renderable(&self) -> impl Renderable {
        Inset::new(wrapped(self.lines()), Insets::tlbr(0, 0, 0, 0))
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
