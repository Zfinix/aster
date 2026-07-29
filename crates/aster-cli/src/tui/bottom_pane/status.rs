//! The working indicator: spinner, current activity, elapsed time, esc hint.
//! Animates by rescheduling its own frame; hidden while a modal is up.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::tui::render::Renderable;
use crate::tui::terminal::FrameRequester;
use crate::tui::{ACCENT, SPINNER, theme};

const FRAME_EVERY: Duration = Duration::from_millis(100);

pub(crate) struct StatusWidget {
    detail: Option<String>,
    started: Instant,
    frames: FrameRequester,
}

impl StatusWidget {
    pub(crate) fn new(frames: FrameRequester) -> Self {
        Self {
            detail: None,
            started: Instant::now(),
            frames,
        }
    }

    /// Current activity label, e.g. the running tool ("reading chat.rs…").
    pub(crate) fn set_detail(&mut self, detail: Option<String>) {
        self.detail = detail;
        self.frames.schedule_now();
    }

    fn line(&self) -> Line<'static> {
        let elapsed = self.started.elapsed();
        let spinner = SPINNER[(elapsed.as_millis() / 100) as usize % SPINNER.len()];
        let label = self.detail.clone().unwrap_or_else(|| "thinking".into());
        Line::from(vec![
            Span::styled(format!("{spinner} "), Style::default().fg(ACCENT)),
            Span::styled(label, theme::dim()),
            Span::styled(
                format!(" · {}s · esc to interrupt", elapsed.as_secs()),
                theme::faint(),
            ),
        ])
    }
}

impl Renderable for StatusWidget {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.line().render(area, buf);
        // Keep the spinner turning without anyone else requesting frames.
        self.frames.schedule_in(FRAME_EVERY);
    }
    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}
