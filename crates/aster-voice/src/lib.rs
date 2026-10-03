#![forbid(unsafe_code)]
//! Voice for Aster: record from the default microphone with [`Recording`] and
//! turn the clip into text with a [`Transcriber`] picked from the keys in env.

mod clip;
mod elevenlabs;
mod mic;
mod openai;
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
mod vad;

use std::time::Duration;

pub use clip::Clip;
pub use mic::Recording;

pub const KEY_VARS: &[(&str, &str, &str)] = &[
    ("ElevenLabs", "ELEVENLABS_API_KEY", "dictation with Scribe"),
    (
        "OpenAI",
        "OPENAI_API_KEY",
        "dictation, when ElevenLabs is not set",
    ),
];

/// Longest clip a recording keeps. Past this the microphone closes on its own,
/// so a forgotten recording cannot grow without bound.
pub const MAX_RECORDING: Duration = Duration::from_secs(300);

const MIN_RECORDING: Duration = Duration::from_millis(300);

const TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug)]
pub enum VoiceError {
    Unsupported,
    NoMicrophone(String),
    TooShort,
    Provider {
        provider: &'static str,
        detail: String,
    },
}

impl std::fmt::Display for VoiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => write!(f, "microphone capture is not built for this platform"),
            Self::NoMicrophone(detail) => write!(f, "microphone unavailable: {detail}"),
            Self::TooShort => write!(f, "recording too short to transcribe"),
            Self::Provider { provider, detail } => write!(f, "{provider}: {detail}"),
        }
    }
}

impl std::error::Error for VoiceError {}

/// A speech to text provider, chosen from whichever key is set.
#[derive(Clone)]
pub enum Transcriber {
    ElevenLabs(elevenlabs::Client),
    OpenAi(openai::Client),
}

impl Transcriber {
    pub fn from_env() -> Option<Self> {
        let key = |var: &str| std::env::var(var).ok().filter(|v| !v.trim().is_empty());
        if let Some(k) = key("ELEVENLABS_API_KEY") {
            return Some(Self::ElevenLabs(elevenlabs::Client::new(k)));
        }
        key("OPENAI_API_KEY").map(|k| Self::OpenAi(openai::Client::new(k)))
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::ElevenLabs(_) => "ElevenLabs",
            Self::OpenAi(_) => "OpenAI",
        }
    }

    pub async fn transcribe(&self, clip: &Clip) -> Result<String, VoiceError> {
        if clip.duration() < MIN_RECORDING {
            return Err(VoiceError::TooShort);
        }
        let result = match self {
            Self::ElevenLabs(c) => c.transcribe(clip.wav()).await,
            Self::OpenAi(c) => c.transcribe(clip.wav()).await,
        };
        result
            .map(|text| text.trim().to_string())
            .map_err(|err| VoiceError::Provider {
                provider: self.name(),
                detail: err.to_string(),
            })
    }
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .expect("reqwest::Client always builds")
}

async fn send(req: reqwest::RequestBuilder) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct Transcript {
        text: String,
    }
    let res = req.send().await.map_err(|e| e.to_string())?;
    let status = res.status();
    let body = res.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("{status}: {body}"));
    }
    serde_json::from_str::<Transcript>(&body)
        .map(|t| t.text)
        .map_err(|e| format!("unreadable reply ({e}): {body}"))
}

fn wav_part(wav: Vec<u8>) -> reqwest::multipart::Part {
    reqwest::multipart::Part::bytes(wav)
        .file_name("dictation.wav")
        .mime_str("audio/wav")
        .expect("audio/wav is a valid mime type")
}
