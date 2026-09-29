//! `aster dictate`: record until stdin gets a line or closes, then print the
//! transcript. Front-ends with no chat turn running use it for their mic button.

use std::io::BufRead;

use anyhow::Result;
use aster_voice::{Recording, Transcriber, VoiceError};
use serde_json::json;

/// What went wrong, in one plain sentence, with the raw detail kept apart.
#[derive(Clone, Debug)]
pub(crate) struct DictationFailure {
    pub(crate) message: &'static str,
    pub(crate) detail: Option<String>,
}

impl DictationFailure {
    pub(crate) fn no_key() -> Self {
        Self {
            message: "Voice input needs a speech key. Run `aster key set ELEVENLABS_API_KEY` (or OPENAI_API_KEY), then try again.",
            detail: None,
        }
    }

    pub(crate) fn interrupted(detail: String) -> Self {
        Self {
            message: "Recording stopped unexpectedly. Try again.",
            detail: Some(detail),
        }
    }
}

impl From<VoiceError> for DictationFailure {
    fn from(err: VoiceError) -> Self {
        let (message, detail) = match err {
            VoiceError::Unsupported => ("Voice input isn't available on this system yet.", None),
            VoiceError::NoMicrophone(detail) => (
                "Can't reach your microphone. Check that Aster has microphone access in System Settings, then try again.",
                Some(detail),
            ),
            VoiceError::TooShort => (
                "That was too short to hear. Speak for a moment before stopping.",
                None,
            ),
            VoiceError::Provider { provider, detail } => (
                "Couldn't turn your recording into text. Check your speech key and connection, then try again.",
                Some(format!("{provider}: {detail}")),
            ),
        };
        Self { message, detail }
    }
}

pub(crate) async fn run() -> Result<()> {
    let result = dictate().await;
    let event = match result {
        Ok(text) => json!({ "type": "transcript", "text": text }),
        Err(failure) => json!({
            "type": "error",
            "message": failure.message,
            "detail": failure.detail,
        }),
    };
    println!("{event}");
    Ok(())
}

async fn dictate() -> Result<String, DictationFailure> {
    let transcriber = Transcriber::from_env().ok_or_else(DictationFailure::no_key)?;
    let recording = Recording::start()?;
    println!("{}", json!({ "type": "listening" }));
    tokio::task::spawn_blocking(|| std::io::stdin().lock().read_line(&mut String::new()))
        .await
        .map_err(|e| DictationFailure::interrupted(e.to_string()))?
        .map_err(|e| DictationFailure::interrupted(e.to_string()))?;
    println!("{}", json!({ "type": "transcribing" }));
    let clip = tokio::task::spawn_blocking(|| recording.finish())
        .await
        .map_err(|e| DictationFailure::interrupted(e.to_string()))?;
    Ok(transcriber.transcribe(&clip).await?)
}
