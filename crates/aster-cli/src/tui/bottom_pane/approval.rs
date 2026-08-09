//! Approval prompt: tinted patch preview plus selectable answers. Extra
//! requests queue behind the current one instead of stacking modals.

use std::collections::VecDeque;
use std::path::PathBuf;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use tokio::sync::mpsc;

use super::view::BottomPaneView;
use crate::chat::{Answer, ApprovalRequest};
use crate::tui::render::{Inset, Insets, Renderable, wrapped};
use crate::tui::{history, theme};

/// Longest preview shown; older lines elide so the composer stays reachable.
const MAX_PREVIEW_ROWS: usize = 12;

pub(crate) struct ApprovalView<E> {
    current: Option<ApprovalRequest>,
    queue: VecDeque<ApprovalRequest>,
    selected: usize,
    tx: mpsc::UnboundedSender<E>,
    /// Maps the decision to an app event for side effects (notices, promotion).
    on_decide: fn(Answer, Option<PathBuf>) -> E,
}

impl<E> ApprovalView<E> {
    pub(crate) fn new(
        req: ApprovalRequest,
        tx: mpsc::UnboundedSender<E>,
        on_decide: fn(Answer, Option<PathBuf>) -> E,
    ) -> Self {
        Self {
            current: Some(req),
            queue: VecDeque::new(),
            selected: 0,
            tx,
            on_decide,
        }
    }

    fn options(req: &ApprovalRequest) -> [(&'static str, Answer); 3] {
        match &req.scope {
            Some(_) => [
                ("Yes", Answer::Yes),
                ("Yes, always allow this directory", Answer::Always),
                ("No", Answer::No),
            ],
            None => [
                ("Yes", Answer::Yes),
                ("Yes to all edits this session", Answer::Always),
                ("No", Answer::No),
            ],
        }
    }

    fn decide(&mut self, answer: Answer) {
        let Some(req) = self.current.take() else {
            return;
        };
        let scope = req.scope.clone();
        let _ = req.respond.send(answer);
        let _ = self.tx.send((self.on_decide)(answer, scope));
        self.selected = 0;
        self.current = self.queue.pop_front();
    }

    fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let Some(req) = &self.current else {
            return Vec::new();
        };
        let dim = theme::get().dimmer_style();
        let (title, body) = req
            .preview
            .split_once('\n')
            .unwrap_or((req.preview.as_str(), ""));
        let title = match &req.scope {
            Some(dir) => format!(
                "Allow reads and writes to {}?",
                super::super::helpers::short_path(dir)
            ),
            None => format!("Approve: {}", title.trim_end_matches(':')),
        };

        let mut out = vec![Line::from(vec![
            Span::styled("? ", theme::get().amber_style()),
            Span::styled(title, theme::get().amber_style()),
        ])];
        let diff = history::diff_lines(body, (width as usize).saturating_sub(2).max(8));
        let hidden = diff.len().saturating_sub(MAX_PREVIEW_ROWS);
        if hidden > 0 {
            out.push(Line::from(Span::styled(
                format!("… +{hidden} earlier lines"),
                dim.add_modifier(Modifier::ITALIC),
            )));
        }
        out.extend(diff.into_iter().skip(hidden));
        out.push(Line::from(""));
        for (i, (label, _)) in Self::options(req).iter().enumerate() {
            let active = i == self.selected;
            let style = if active {
                theme::get().selected_style()
            } else {
                theme::get().dimmer_style()
            };
            out.push(Line::from(vec![
                Span::styled(if active { "▸ " } else { "  " }, style),
                Span::styled(format!("{}. {label}", i + 1), style),
            ]));
        }
        if !self.queue.is_empty() {
            out.push(Line::from(Span::styled(
                format!("{} more pending", self.queue.len()),
                dim,
            )));
        }
        out.push(Line::from(Span::styled(
            "  y yes · a always · n/esc no",
            dim,
        )));
        out
    }

    fn renderable(&self, width: u16) -> impl Renderable {
        Inset::new(wrapped(self.lines(width)), Insets::tlbr(0, 0, 0, 0))
    }
}

impl<E> Renderable for ApprovalView<E> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.renderable(area.width.saturating_sub(2))
            .render(area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        self.renderable(width.saturating_sub(2))
            .desired_height(width)
    }
}

impl<E> BottomPaneView<E> for ApprovalView<E> {
    fn handle_key(&mut self, key: KeyEvent) {
        let Some(req) = &self.current else {
            return;
        };
        let options = Self::options(req);
        match key.code {
            KeyCode::Char('y' | 'Y') => self.decide(Answer::Yes),
            KeyCode::Char('a' | 'A') => self.decide(Answer::Always),
            KeyCode::Char('n' | 'N') | KeyCode::Esc => self.decide(Answer::No),
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => self.selected = (self.selected + 1).min(options.len() - 1),
            KeyCode::Enter => self.decide(options[self.selected].1),
            KeyCode::Char(c) => {
                if let Some(n) = c.to_digit(10)
                    && n >= 1
                    && (n as usize) <= options.len()
                {
                    self.decide(options[n as usize - 1].1);
                }
            }
            _ => {}
        }
    }

    fn is_complete(&self) -> bool {
        self.current.is_none() && self.queue.is_empty()
    }

    fn try_consume_approval(&mut self, req: ApprovalRequest) -> Option<ApprovalRequest> {
        if self.current.is_none() {
            self.current = Some(req);
        } else {
            self.queue.push_back(req);
        }
        None
    }
}

#[cfg(test)]
#[path = "tests/approval_test.rs"]
mod tests;
