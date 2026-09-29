//! Ctrl+R dictation in the chat composer. Terminals do not report key release,
//! so one press starts listening and the next sends the clip to be transcribed.

use aster_voice::{Recording, Transcriber};
use tokio::sync::mpsc;

use super::chat::AppEvent;
use crate::dictate::DictationFailure;

#[derive(Default)]
pub(super) enum Dictation {
    #[default]
    Idle,
    Listening(Recording, Transcriber),
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
                let recording = Recording::start()?;
                *self = Self::Listening(recording, transcriber);
            }
            Self::Listening(recording, transcriber) => {
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
            Self::Transcribing => *self = Self::Transcribing,
        }
        Ok(())
    }

    pub(super) fn label(&self) -> Option<&'static str> {
        match self {
            Self::Idle => None,
            Self::Listening(..) => Some("● listening · ctrl+r to stop"),
            Self::Transcribing => Some("transcribing…"),
        }
    }
}
