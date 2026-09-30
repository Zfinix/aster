//! Ctrl+R dictation in the chat composer. One press starts listening; it stops
//! by itself once the speaker goes quiet, or on a second press.

use std::collections::VecDeque;
use std::time::Instant;

use aster_voice::{Recording, Transcriber};
use tokio::sync::mpsc;

use super::chat::AppEvent;
use crate::dictate::DictationFailure;

const METER_WIDTH: usize = 6;
const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

#[derive(Default)]
pub(super) enum Dictation {
    #[default]
    Idle,
    Listening {
        recording: Recording,
        transcriber: Transcriber,
        started: Instant,
        levels: VecDeque<f32>,
    },
    Transcribing,
}

impl Dictation {
    pub(super) fn toggle(
        &mut self,
        tx: &mpsc::UnboundedSender<AppEvent>,
    ) -> Result<(), DictationFailure> {
        match std::mem::take(self) {
            Self::Idle => {
                let transcriber = Transcriber::from_env().ok_or_else(DictationFailure::no_key)?;
                *self = Self::Listening {
                    recording: Recording::start()?,
                    transcriber,
                    started: Instant::now(),
                    levels: VecDeque::from(vec![0.0; METER_WIDTH]),
                };
            }
            Self::Listening {
                recording,
                transcriber,
                ..
            } => self.transcribe(recording, transcriber, tx),
            Self::Transcribing => *self = Self::Transcribing,
        }
        Ok(())
    }

    /// Samples the meter before a frame and hands the clip off once the
    /// speaker has gone quiet. True while listening, so frames keep coming.
    pub(super) fn tick(&mut self, tx: &mpsc::UnboundedSender<AppEvent>) -> bool {
        let Self::Listening {
            recording, levels, ..
        } = self
        else {
            return false;
        };
        if recording.ended() {
            if let Self::Listening {
                recording,
                transcriber,
                ..
            } = std::mem::take(self)
            {
                self.transcribe(recording, transcriber, tx);
            }
            return false;
        }
        levels.pop_front();
        levels.push_back(recording.level());
        true
    }

    fn transcribe(
        &mut self,
        recording: Recording,
        transcriber: Transcriber,
        tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        *self = Self::Transcribing;
        let tx = tx.clone();
        tokio::spawn(async move {
            let result = match tokio::task::spawn_blocking(|| recording.finish()).await {
                Ok(clip) => transcriber
                    .transcribe(&clip)
                    .await
                    .map_err(DictationFailure::from),
                Err(err) => Err(DictationFailure::interrupted(err.to_string())),
            };
            let _ = tx.send(AppEvent::Dictated(result));
        });
    }

    pub(super) fn label(&self) -> Option<String> {
        match self {
            Self::Idle => None,
            Self::Listening {
                started, levels, ..
            } => {
                let secs = started.elapsed().as_secs();
                let meter: String = levels.iter().map(|&level| bar(level)).collect();
                Some(format!(
                    "● {}:{:02} {meter} · ctrl+r to stop",
                    secs / 60,
                    secs % 60
                ))
            }
            Self::Transcribing => Some("transcribing…".into()),
        }
    }
}

/// Loudness on a log scale from room hum to loud speech, so silence sits flat
/// on the lowest bar and talk climbs the rest.
fn bar(level: f32) -> char {
    const QUIET: f32 = 0.004;
    const LOUD: f32 = 0.25;
    let scaled = ((level / QUIET).log10() / (LOUD / QUIET).log10()).clamp(0.0, 1.0);
    BARS[(scaled * (BARS.len() - 1) as f32).round() as usize]
}
