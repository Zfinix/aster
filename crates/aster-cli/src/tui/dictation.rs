//! Ctrl+R dictation in the chat composer. One press runs `aster dictate`, the
//! same helper the editors use, so the words show in the footer as the speaker
//! talks. It stops by itself once the speaker goes quiet, or on a second press.

use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio;
use std::time::Instant;

use ratatui::prelude::*;
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;
use unicode_width::UnicodeWidthChar;

use super::chat::AppEvent;
use super::{theme, wrap};
use crate::dictate::DictationFailure;

const METER_WIDTH: usize = 6;
const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

#[derive(Default)]
pub(super) enum Dictation {
    #[default]
    Idle,
    Listening(Box<Session>),
    Transcribing(Box<Session>),
}

/// One line of `aster dictate` output.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Event {
    Listening,
    Level {
        level: f32,
    },
    Partial {
        text: String,
    },
    Transcribing,
    Transcript {
        text: String,
    },
    Error {
        message: String,
        detail: Option<String>,
    },
}

/// A running `aster dictate`. Dropping it kills the helper and discards the
/// recording.
pub(super) struct Session {
    _child: Child,
    stdin: Option<ChildStdin>,
    events: mpsc::UnboundedReceiver<Option<Event>>,
    started: Instant,
    levels: VecDeque<f32>,
    heard: String,
}

impl Session {
    fn start(repo_root: &Path) -> Result<Self, DictationFailure> {
        let exe =
            std::env::current_exe().map_err(|e| DictationFailure::interrupted(e.to_string()))?;
        let mut child = Command::new(exe)
            .arg("dictate")
            .current_dir(repo_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| DictationFailure::interrupted(e.to_string()))?;
        let (tx, events) = mpsc::unbounded_channel();
        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let Ok(event) = serde_json::from_str(&line) else {
                        continue;
                    };
                    if tx.send(Some(event)).is_err() {
                        return;
                    }
                }
                let _ = tx.send(None);
            });
        }
        Ok(Self {
            stdin: child.stdin.take(),
            _child: child,
            events,
            started: Instant::now(),
            levels: VecDeque::from(vec![0.0; METER_WIDTH]),
            heard: String::new(),
        })
    }
}

impl Dictation {
    pub(super) fn toggle(&mut self, repo_root: &Path) -> Result<(), DictationFailure> {
        match std::mem::take(self) {
            Self::Idle => *self = Self::Listening(Box::new(Session::start(repo_root)?)),
            Self::Listening(mut session) => {
                session.stdin = None;
                *self = Self::Transcribing(session);
            }
            transcribing @ Self::Transcribing(_) => *self = transcribing,
        }
        Ok(())
    }

    /// Takes in what the helper printed since the last frame and hands the
    /// result off once it has one. True while it runs, so frames keep coming.
    pub(super) fn tick(&mut self, tx: &mpsc::UnboundedSender<AppEvent>) -> bool {
        let (Self::Listening(session) | Self::Transcribing(session)) = self else {
            return false;
        };
        let mut quiet = false;
        let mut result = None;
        while let Ok(event) = session.events.try_recv() {
            match event {
                Some(Event::Listening) => {}
                Some(Event::Level { level }) => {
                    session.levels.pop_front();
                    session.levels.push_back(level);
                }
                Some(Event::Partial { text }) => session.heard = text,
                Some(Event::Transcribing) => quiet = true,
                Some(Event::Transcript { text }) => result = Some(Ok(text)),
                Some(Event::Error { message, detail }) => {
                    result = Some(Err(DictationFailure { message, detail }));
                }
                None => {
                    result.get_or_insert_with(|| {
                        Err(DictationFailure::interrupted("aster dictate exited".into()))
                    });
                }
            }
        }
        if let Some(result) = result {
            *self = Self::Idle;
            let _ = tx.send(AppEvent::Dictated(result));
            return false;
        }
        if quiet
            && matches!(self, Self::Listening(_))
            && let Self::Listening(session) = std::mem::take(self)
        {
            *self = Self::Transcribing(session);
        }
        true
    }

    /// Stands in for the chat footer while the mic is on: a timer and meter,
    /// then the words heard so far with the newest at the right edge.
    pub(super) fn footer(&self, width: u16) -> Option<Line<'static>> {
        let (session, listening) = match self {
            Self::Idle => return None,
            Self::Listening(session) => (session, true),
            Self::Transcribing(session) => (session, false),
        };
        let theme = theme::get();
        let secs = session.started.elapsed().as_secs();
        let lead = match listening {
            true => {
                let meter: String = session.levels.iter().map(|&level| bar(level)).collect();
                format!("  ● {}:{:02} {meter}  ", secs / 60, secs % 60)
            }
            false => "  ◌ ".to_string(),
        };
        let hint = match listening {
            true => "  ·  ctrl+r to stop · esc to discard",
            false => "  ·  finishing…",
        };
        let mut spans = vec![Span::styled(lead.clone(), theme.accent_style())];
        let room = (width as usize).saturating_sub(wrap::width(&lead) + wrap::width(hint));
        let heard = session.heard.trim();
        if heard.is_empty() {
            let waiting = match listening {
                true => "listening",
                false => "transcribing",
            };
            spans.push(Span::styled(waiting, theme.dim_style()));
        } else {
            let shown = tail(heard, room);
            let cut = shown.rfind(' ').map_or(0, |i| i + 1);
            spans.push(Span::styled(shown[..cut].to_string(), theme.text_style()));
            spans.push(Span::styled(shown[cut..].to_string(), theme.dim_style()));
        }
        spans.push(Span::styled(hint, theme.faint_style()));
        Some(Line::from(spans))
    }
}

/// The end of `text` that fits in `max` columns, with an ellipsis when the
/// start is cut.
fn tail(text: &str, max: usize) -> String {
    if wrap::width(text) <= max {
        return text.to_string();
    }
    let mut used = 1;
    let start = text
        .char_indices()
        .rev()
        .take_while(|&(_, c)| {
            used += c.width().unwrap_or(0);
            used <= max
        })
        .last()
        .map_or(text.len(), |(i, _)| i);
    format!("…{}", &text[start..])
}

/// Loudness on a log scale from room hum to loud speech, so silence sits flat
/// on the lowest bar and talk climbs the rest.
fn bar(level: f32) -> char {
    const QUIET: f32 = 0.004;
    const LOUD: f32 = 0.25;
    let scaled = ((level / QUIET).log10() / (LOUD / QUIET).log10()).clamp(0.0, 1.0);
    BARS[(scaled * (BARS.len() - 1) as f32).round() as usize]
}
