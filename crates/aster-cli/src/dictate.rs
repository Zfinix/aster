//! `aster dictate`: record until the speaker goes quiet or stdin gets a line,
//! then print the transcript. Front-ends use it for their mic button.

use std::io::BufRead;

use anyhow::Result;
use aster_voice::{Recording, Transcriber, VoiceError};
use serde_json::json;

const POLL: std::time::Duration = std::time::Duration::from_millis(50);

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
    let (line_tx, mut line) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let _ = line_tx.send(std::io::stdin().lock().read_line(&mut String::new()));
    });
    while !recording.ended() {
        tokio::select! {
            read = &mut line => {
                if let Ok(Err(e)) = read {
                    return Err(DictationFailure::interrupted(e.to_string()));
                }
                break;
            }
            () = tokio::time::sleep(POLL) => {}
        }
    }
    println!("{}", json!({ "type": "transcribing" }));
    let clip = tokio::task::spawn_blocking(|| recording.finish())
        .await
        .map_err(|e| DictationFailure::interrupted(e.to_string()))?;
    Ok(transcriber.transcribe(&clip).await?)
}
