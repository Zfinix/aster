//! `aster dictate`: record until stdin gets a line or closes, then print the
//! transcript. Front-ends with no chat turn running use it for their mic button.

use std::io::BufRead;
use std::path::{Path, PathBuf};

use anyhow::Result;
use aster_voice::{Missing, Recording, Transcriber, VoiceConfig, VoiceError};
use serde_json::json;

/// What went wrong, in one plain sentence, with the raw detail kept apart.
#[derive(Clone, Debug)]
pub(crate) struct DictationFailure {
    pub(crate) message: String,
    pub(crate) detail: Option<String>,
}

impl DictationFailure {
    pub(crate) fn interrupted(detail: String) -> Self {
        Self {
            message: "Recording stopped unexpectedly. Try again.".into(),
            detail: Some(detail),
        }
    }
}

impl From<VoiceError> for DictationFailure {
    fn from(err: VoiceError) -> Self {
        let (message, detail): (&str, _) = match err {
            VoiceError::Unsupported => ("Voice isn't available on this system yet.", None),
            VoiceError::NotSetUp(missing) => {
                return Self {
                    message: not_set_up(missing),
                    detail: None,
                };
            }
            VoiceError::NoSpeaker(detail) => (
                "Can't play sound. Check your speakers or headphones, then try again.",
                Some(detail),
            ),
            VoiceError::NoMicrophone(detail) => (
                "Can't reach your microphone. Check that Aster has microphone access in System Settings, then try again.",
                Some(detail),
            ),
            VoiceError::TooShort => (
                "That was too short to hear. Speak for a moment before stopping.",
                None,
            ),
            VoiceError::Provider { provider, detail } => (
                "The voice service didn't answer. Check your voice key and connection, then try again.",
                Some(format!("{provider}: {detail}")),
            ),
        };
        Self {
            message: message.into(),
            detail,
        }
    }
}

fn not_set_up(missing: Missing) -> String {
    match missing {
        Missing::AnyKey => "Voice input needs a speech key. Run `aster key set GROQ_API_KEY` (or ELEVENLABS_API_KEY, OPENAI_API_KEY, DEEPGRAM_API_KEY), then try again.".into(),
        Missing::Key(var) => format!("Voice needs {var}. Run `aster key set {var}`, then try again."),
        Missing::Url(key) => format!("Voice needs the address of your speech server. Set voice.{key} in aster.yaml, then try again."),
    }
}

/// The voice block in effect here. A malformed aster.yaml is an error, so a
/// misspelled provider never quietly falls back to the default.
pub(crate) fn voice_config(repo_root: &Path) -> Result<VoiceConfig, DictationFailure> {
    crate::settings::Settings::load(Some(repo_root))
        .map(|s| s.voice)
        .map_err(|e| DictationFailure {
            message: "Your aster.yaml has a mistake in it. Fix it, then try again.".into(),
            detail: Some(format!("{e:#}")),
        })
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
    let transcriber = Transcriber::from_config(&voice_config(&cwd())?)?;
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

pub(crate) fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_default()
}
