//! The working indicator: spinner, current activity, elapsed time, esc hint.
//! Animates by rescheduling its own frame; hidden while a modal is up.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::tui::SPINNER;
use crate::tui::render::Renderable;
use crate::tui::terminal::FrameRequester;
use crate::tui::theme;

const FRAME_EVERY: Duration = Duration::from_millis(100);
const ROTATE_EVERY: Duration = Duration::from_millis(3200);

const WORDS: [&str; 35] = [
    "Astering",
    "Stargazing",
    "Orbiting",
    "Charting",
    "Triangulating",
    "Navigating",
    "Plotting",
    "Scanning",
    "Squinting",
    "Combing",
    "Sifting",
    "Untangling",
    "Unravelling",
    "Tracing",
    "Threading",
    "Poring",
    "Surveying",
    "Sounding",
    "Prospecting",
    "Excavating",
    "Decoding",
    "Whittling",
    "Winnowing",
    "Nitpicking",
    "Grokking",
    "Rummaging",
    "Foraging",
    "Circling",
    "Homing",
    "Aligning",
    "Calibrating",
    "Focusing",
    "Reckoning",
    "Deducing",
    "Sleuthing",
];

fn word(seed: u64, elapsed: Duration) -> &'static str {
    let bucket = (elapsed.as_millis() / ROTATE_EVERY.as_millis()) as u64;
    let mixed = seed
        .wrapping_add(bucket)
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    WORDS[(mixed >> 33) as usize % WORDS.len()]
}

pub(crate) struct StatusWidget {
    detail: Option<String>,
    started: Instant,
    seed: u64,
    frames: FrameRequester,
}

impl StatusWidget {
    pub(crate) fn new(frames: FrameRequester) -> Self {
        Self {
            detail: None,
            started: Instant::now(),
            seed: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos() as u64),
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
        let label = self
            .detail
            .clone()
            .unwrap_or_else(|| word(self.seed, elapsed).to_string());
        Line::from(vec![
            Span::styled(format!("{spinner} "), theme::get().accent_style()),
            Span::styled(label, theme::get().dim_style()),
            Span::styled(
                format!(
                    " · {} · esc to interrupt",
                    crate::tui::helpers::elapsed(elapsed.as_secs())
                ),
                theme::get().faint_style(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_holds_for_a_rotation_then_changes() {
        let held = word(7, Duration::from_millis(0));
        assert_eq!(held, word(7, ROTATE_EVERY - Duration::from_millis(1)));
        assert_ne!(held, word(7, ROTATE_EVERY));
    }

    #[test]
    fn different_seeds_open_on_different_words() {
        let opening: Vec<_> = (0..8).map(|s| word(s, Duration::ZERO)).collect();
        assert!(
            opening
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                > 1
        );
    }

    #[test]
    fn every_bucket_lands_on_a_word() {
        for bucket in 0..500 {
            let at = ROTATE_EVERY * bucket;
            assert!(WORDS.contains(&word(3, at)));
        }
    }
}
