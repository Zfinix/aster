use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use tokio::sync::mpsc;

use super::view::BottomPaneView;
use crate::tui::chat::AppEvent;
use crate::tui::render::{Inset, Insets, Renderable, wrapped};
use crate::tui::theme;

use super::{VISIBLE_ROWS, window_start};

pub(crate) struct ModelPickerView {
    models: Vec<String>,
    query: String,
    cursor: usize,
    selected: usize,
    complete: bool,
    tx: mpsc::UnboundedSender<AppEvent>,
}

impl ModelPickerView {
    pub(crate) fn new(
        current: &str,
        models: Vec<String>,
        tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Self {
        let selected = models.iter().position(|m| m == current).unwrap_or(0);
        Self {
            models,
            query: String::new(),
            cursor: 0,
            selected,
            complete: false,
            tx,
        }
    }

    fn filtered(&self) -> Vec<&str> {
        let q = self.query.to_lowercase();
        self.models
            .iter()
            .filter(|m| m.to_lowercase().contains(&q))
            .map(String::as_str)
            .collect()
    }

    fn accept(&mut self) {
        let filtered = self.filtered();
        let model = if filtered.len() == 1 {
            filtered[0].to_string()
        } else if !filtered.is_empty() {
            let idx = self.selected.min(filtered.len() - 1);
            filtered[idx].to_string()
        } else {
            self.query.trim().to_string()
        };
        if model.is_empty() {
            return;
        }
        let _ = self.tx.send(AppEvent::ModelChanged(model));
        self.complete = true;
    }

    fn move_by(&mut self, delta: isize) {
        let len = self.filtered().len();
        if len > 0 {
            let cur = self.selected.min(len - 1) as isize;
            self.selected = (cur + delta).rem_euclid(len as isize) as usize;
        }
    }

    fn lines(&self) -> Vec<Line<'static>> {
        let filtered = self.filtered();
        let mut out = vec![
            Line::from(Span::styled(
                "Switch model",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            self.query_line(),
            Line::from(""),
        ];
        if filtered.is_empty() {
            if !self.query.is_empty() {
                out.push(Line::from(Span::styled(
                    format!("  press enter to use \"{}\"", self.query.trim()),
                    theme::get().faint_style(),
                )));
            }
        } else {
            let sel = self.selected.min(filtered.len().saturating_sub(1));
            let start = window_start(sel, filtered.len());
            for (i, id) in filtered.iter().enumerate().skip(start).take(VISIBLE_ROWS) {
                let active = i == sel;
                let style = if active {
                    theme::get().selected_style()
                } else {
                    theme::get().dimmer_style()
                };
                out.push(Line::from(vec![Span::styled(
                    if active {
                        format!("▸ {id}")
                    } else {
                        format!("  {id}")
                    },
                    style,
                )]));
            }
            if filtered.len() > VISIBLE_ROWS {
                out.push(Line::from(Span::styled(
                    format!("  +{} more", filtered.len() - VISIBLE_ROWS),
                    theme::get().faint_style(),
                )));
            }
        }
        out.push(Line::from(""));
        out.push(Line::from(Span::styled(
            "enter confirm · esc cancel · type to filter",
            theme::get().faint_style(),
        )));
        out
    }

    fn query_line(&self) -> Line<'static> {
        if self.query.is_empty() {
            return Line::from(vec![
                Span::styled("> ", theme::get().dimmer_style()),
                Span::styled(
                    " ",
                    Style::default()
                        .add_modifier(Modifier::REVERSED)
                        .add_modifier(Modifier::SLOW_BLINK),
                ),
                Span::styled(" search models", theme::get().faint_style()),
            ]);
        }
        let pos = self.cursor.min(self.query.len());
        let prefix = self.query[..pos].to_string();
        let rest = &self.query[pos..];
        let cursor_char = if rest.is_empty() {
            " ".to_string()
        } else {
            rest[0..1].to_string()
        };
        let after = if rest.len() <= 1 {
            String::new()
        } else {
            rest[1..].to_string()
        };
        Line::from(vec![
            Span::styled("> ", theme::get().dimmer_style()),
            Span::raw(prefix),
            Span::styled(
                cursor_char,
                Style::default()
                    .add_modifier(Modifier::REVERSED)
                    .add_modifier(Modifier::SLOW_BLINK),
            ),
            Span::raw(after),
        ])
    }
}

impl Renderable for ModelPickerView {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        Inset::new(wrapped(self.lines()), Insets::tlbr(0, 0, 0, 0)).render(area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        Inset::new(wrapped(self.lines()), Insets::tlbr(0, 0, 0, 0)).desired_height(width)
    }
}

impl BottomPaneView<AppEvent> for ModelPickerView {
    fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.complete = true,
            KeyCode::Enter => self.accept(),
            KeyCode::Up => self.move_by(-1),
            KeyCode::Down => self.move_by(1),
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                self.cursor = (self.cursor + 1).min(self.query.len());
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.query.remove(self.cursor - 1);
                    self.cursor -= 1;
                    self.selected = 0;
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.query.len() {
                    self.query.remove(self.cursor);
                    self.selected = 0;
                }
            }
            KeyCode::Home => {
                self.cursor = 0;
            }
            KeyCode::End => {
                self.cursor = self.query.len();
            }
            KeyCode::Char(c) => {
                self.query.insert(self.cursor, c);
                self.cursor += 1;
                self.selected = 0;
            }
            _ => {}
        }
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    fn handle_click(&mut self, row: u16) -> bool {
        const FIRST_ROW: u16 = 4;
        let filtered = self.filtered();
        let Some(offset) = row.checked_sub(FIRST_ROW).map(|r| r as usize) else {
            return false;
        };
        if offset >= VISIBLE_ROWS || filtered.is_empty() {
            return false;
        }
        let sel = self.selected.min(filtered.len() - 1);
        let index = window_start(sel, filtered.len()) + offset;
        if index >= filtered.len() {
            return false;
        }
        self.selected = index;
        self.accept();
        true
    }

    fn handle_scroll(&mut self, delta: isize) {
        self.move_by(delta);
    }
}

#[cfg(test)]
#[path = "tests/model_picker_test.rs"]
mod tests;
