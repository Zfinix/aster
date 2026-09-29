//! Ctrl+R dictation in the chat composer. Terminals do not report key release,
//! so one press starts listening and the next sends the clip to be transcribed.

use aster_voice::{Recording, Transcriber, VoiceError};
use tokio::sync::mpsc;

use super::chat::AppEvent;

#[derive(Default)]
pub(super) enum Dictation {
    #[default]
    Idle,
    Listening(Recording, Transcriber),
    Transcribing,
}

/// What went wrong, in one plain sentence, with the raw detail kept apart.
#[derive(Clone)]
pub(super) struct DictationFailure {
    pub(super) message: &'static str,
    pub(super) detail: Option<String>,
}

impl Dictation {
    pub(super) fn toggle(
        &mut self,
        tx: &mpsc::UnboundedSender<AppEvent>,
    ) -> Result<(), DictationFailure> {
        match std::mem::take(self) {
            Self::Idle => {
                let Some(transcriber) = Transcriber::from_env() else {
                    return Err(DictationFailure {
                        message: "Voice input needs a speech key. Run `aster key set ELEVENLABS_API_KEY` (or OPENAI_API_KEY), then try again.",
                        detail: None,
                    });
                };
                let recording = Recording::start().map_err(failure)?;
                *self = Self::Listening(recording, transcriber);
            }
            Self::Listening(recording, transcriber) => {
                *self = Self::Transcribing;
                let tx = tx.clone();
                tokio::spawn(async move {
                    let result = match tokio::task::spawn_blocking(|| recording.finish()).await {
                        Ok(clip) => transcriber.transcribe(&clip).await.map_err(failure),
                        Err(err) => Err(DictationFailure {
                            message: "Recording stopped unexpectedly. Try again.",
                            detail: Some(err.to_string()),
                        }),
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

fn failure(err: VoiceError) -> DictationFailure {
    let (message, detail) = match err {
        VoiceError::Unsupported => ("Voice input isn't available on this system yet.", None),
        VoiceError::NoMicrophone(detail) => (
            "Can't reach your microphone. Check that your terminal has microphone access in System Settings, then try again.",
            Some(detail),
        ),
        VoiceError::TooShort => (
            "That was too short to hear. Speak for a moment before pressing ctrl+r again.",
            None,
        ),
        VoiceError::Provider { provider, detail } => (
            "Couldn't turn your recording into text. Check your speech key and connection, then try again.",
            Some(format!("{provider}: {detail}")),
        ),
    };
    DictationFailure { message, detail }
}
