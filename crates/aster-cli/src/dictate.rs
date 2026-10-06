//! `aster dictate`: record until the speaker goes quiet or stdin gets a line,
//! printing the loudness and the words heard so far as it goes, then the
//! transcript. Front-ends with no chat turn running use it for their mic
//! button. With no `voice.stt` set, words come from this machine's own
//! recognizer when it has one, and from a speech key otherwise.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use aster_voice::{Heard, Missing, Recording, Transcriber, VoiceConfig, VoiceError};
use serde_json::json;

const POLL: Duration = Duration::from_millis(50);
const SETTLE: Duration = Duration::from_secs(6);

/// What went wrong, in one plain sentence, with the raw detail kept apart.
#[derive(Clone, Debug)]
pub(crate) struct DictationFailure {
    pub(crate) message: String,
    pub(crate) detail: Option<String>,
}

impl DictationFailure {
    fn nothing_heard() -> Self {
        Self {
            message: "Didn't catch any words. Speak a little closer to the mic, then try again."
                .into(),
            detail: None,
        }
    }

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
            VoiceError::SpeechNotAllowed => (
                "Aster isn't allowed to recognize speech. Turn it on in System Settings > Privacy & Security > Speech Recognition, then try again.",
                None,
            ),
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
    if let Some(code) = aster_voice::relaunch_as_own_app()? {
        std::process::exit(code);
    }
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
    let config = voice_config(&cwd())?;
    let on_device = config.stt.is_none() && config.stt_url.is_none();
    let transcriber = Transcriber::from_config(&config);
    aster_voice::ask_for_microphone()?;
    let recording = Recording::start()?;
    let live = on_device
        .then(|| aster_voice::listen_on_device(recording.audio(), recording.sample_rate()));
    let (words, transcriber) = match (live, transcriber) {
        (Some(Ok(words)), transcriber) => (Some(words), transcriber.ok()),
        (Some(Err(err)), Ok(transcriber)) => {
            tracing::info!("on-device speech unavailable, using a speech key: {err}");
            (None, Some(transcriber))
        }
        (Some(Err(VoiceError::Unsupported)) | None, Err(err)) => return Err(err.into()),
        (Some(Err(err)), Err(_)) => return Err(err.into()),
        (None, Ok(transcriber)) => (None, Some(transcriber)),
    };
    println!(
        "{}",
        json!({
            "type": "listening",
            "service": transcriber.as_ref().map_or("on-device", Transcriber::name),
            "model": transcriber.as_ref().map(Transcriber::model),
        })
    );
    let mut heard = String::new();
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
            () = tokio::time::sleep(POLL) => {
                println!("{}", json!({ "type": "level", "level": recording.level() }));
                for Heard::Partial(text) | Heard::Final(text) in words.iter().flat_map(mpsc::Receiver::try_iter) {
                    if text != heard {
                        println!("{}", json!({ "type": "partial", "text": text }));
                        heard = text;
                    }
                }
            }
        }
    }
    println!("{}", json!({ "type": "transcribing" }));
    let clip = tokio::task::spawn_blocking(|| recording.finish())
        .await
        .map_err(|e| DictationFailure::interrupted(e.to_string()))?;
    if let Some(words) = words {
        let settled = tokio::task::spawn_blocking(move || settle(&words, heard))
            .await
            .map_err(|e| DictationFailure::interrupted(e.to_string()))?;
        if !settled.is_empty() {
            return Ok(settled);
        }
    }
    match transcriber {
        Some(transcriber) => Ok(transcriber.transcribe(&clip).await?),
        None => Err(DictationFailure::nothing_heard()),
    }
}

/// Lets the recognizer finish the audio it was handed. Its final pass can
/// drop words the speaker already saw, so the last partial wins when there is
/// one.
fn settle(words: &mpsc::Receiver<Heard>, mut heard: String) -> String {
    let deadline = std::time::Instant::now() + SETTLE;
    while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
        match words.recv_timeout(left) {
            Ok(Heard::Partial(text)) => heard = text,
            Ok(Heard::Final(text)) if heard.trim().is_empty() => heard = text,
            Ok(Heard::Final(_)) | Err(_) => break,
        }
    }
    heard.trim().to_string()
}

pub(crate) fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_default()
}
