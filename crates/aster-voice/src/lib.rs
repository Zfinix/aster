#![forbid(unsafe_code)]
//! Voice for Aster: record from the default microphone with [`Recording`], turn
//! the clip into text with a [`Transcriber`], and read replies aloud with a
//! [`Speaker`]. Providers come from the `voice:` block in `aster.yaml`.

mod clip;
mod config;
mod deepgram;
mod elevenlabs;
mod mic;
mod openai;
mod playback;
mod speech;

use std::time::Duration;

pub use clip::Clip;
pub use config::{SttProvider, TtsProvider, VoiceConfig};
pub use mic::Recording;
pub use speech::{Hush, MAX_SPOKEN_CHARS, Speaker, Speech, speakable};

pub const KEY_VARS: &[(&str, &str, &str)] = &[
    (
        "ElevenLabs",
        "ELEVENLABS_API_KEY",
        "dictation with Scribe, and read aloud",
    ),
    (
        "OpenAI",
        "OPENAI_API_KEY",
        "dictation, when ElevenLabs is not set, and read aloud",
    ),
    ("Groq", "GROQ_API_KEY", "dictation with hosted Whisper"),
    ("Deepgram", "DEEPGRAM_API_KEY", "dictation with Nova"),
    (
        "Speech server",
        "ASTER_VOICE_API_KEY",
        "an OpenAI-compatible voice server, if it wants a key",
    ),
];

/// Longest clip a recording keeps. Past this the microphone closes on its own,
/// so a forgotten recording cannot grow without bound.
pub const MAX_RECORDING: Duration = Duration::from_secs(300);

const MIN_RECORDING: Duration = Duration::from_millis(300);

const TIMEOUT: Duration = Duration::from_secs(120);

/// Order tried when `voice.stt` is unset: the first with a key wins.
const AUTO_STT: &[SttProvider] = &[
    SttProvider::ElevenLabs,
    SttProvider::OpenAi,
    SttProvider::Groq,
    SttProvider::Deepgram,
];

#[derive(Debug)]
pub enum VoiceError {
    Unsupported,
    NotSetUp(Missing),
    NoMicrophone(String),
    NoSpeaker(String),
    TooShort,
    Provider {
        provider: &'static str,
        detail: String,
    },
}

/// What a provider needs before it can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    AnyKey,
    Key(&'static str),
    Url(&'static str),
}

impl std::fmt::Display for VoiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => write!(f, "audio is not built for this platform"),
            Self::NotSetUp(Missing::AnyKey) => write!(f, "no speech key is set"),
            Self::NotSetUp(Missing::Key(var)) => write!(f, "{var} is not set"),
            Self::NotSetUp(Missing::Url(key)) => write!(f, "voice.{key} is not set"),
            Self::NoMicrophone(detail) => write!(f, "microphone unavailable: {detail}"),
            Self::NoSpeaker(detail) => write!(f, "speaker unavailable: {detail}"),
            Self::TooShort => write!(f, "recording too short to transcribe"),
            Self::Provider { provider, detail } => write!(f, "{provider}: {detail}"),
        }
    }
}

impl std::error::Error for VoiceError {}

/// A speech to text provider and the language it listens for.
#[derive(Clone)]
pub struct Transcriber {
    provider: SttProvider,
    engine: Engine,
    language: Option<String>,
}

#[derive(Clone)]
enum Engine {
    ElevenLabs(elevenlabs::Client),
    OpenAi(openai::Client),
    Deepgram(deepgram::Client),
}

impl Transcriber {
    /// The provider `voice.stt` names, or the first one with a key set.
    pub fn from_config(config: &VoiceConfig) -> Result<Self, VoiceError> {
        let provider = match config.stt {
            Some(provider) => provider,
            None if config.stt_url.is_some() => SttProvider::OpenAiCompatible,
            None => AUTO_STT
                .iter()
                .copied()
                .find(|p| stt_key_var(*p).and_then(key).is_some())
                .ok_or(VoiceError::NotSetUp(Missing::AnyKey))?,
        };
        let model = |default: &str| config.stt_model.clone().unwrap_or_else(|| default.into());
        let needed = || {
            let var = stt_key_var(provider).unwrap_or_default();
            key(var).ok_or(VoiceError::NotSetUp(Missing::Key(var)))
        };
        let engine = match provider {
            SttProvider::ElevenLabs => Engine::ElevenLabs(elevenlabs::Client::new(
                needed()?,
                model(elevenlabs::STT_MODEL),
            )),
            SttProvider::OpenAi => Engine::OpenAi(openai::Client::new(
                openai::OPENAI_URL,
                model("gpt-4o-transcribe"),
                Some(needed()?),
            )),
            SttProvider::Groq => Engine::OpenAi(openai::Client::new(
                openai::GROQ_URL,
                model("whisper-large-v3-turbo"),
                Some(needed()?),
            )),
            SttProvider::Deepgram => {
                Engine::Deepgram(deepgram::Client::new(needed()?, model(deepgram::MODEL)))
            }
            SttProvider::OpenAiCompatible => Engine::OpenAi(openai::Client::new(
                config
                    .stt_url
                    .as_deref()
                    .ok_or(VoiceError::NotSetUp(Missing::Url("stt_url")))?,
                model("whisper-1"),
                key("ASTER_VOICE_API_KEY"),
            )),
        };
        Ok(Self {
            provider,
            engine,
            language: config.language.clone(),
        })
    }

    pub fn name(&self) -> &'static str {
        self.provider.label()
    }

    pub fn model(&self) -> &str {
        match &self.engine {
            Engine::ElevenLabs(c) => c.model(),
            Engine::OpenAi(c) => c.model(),
            Engine::Deepgram(c) => c.model(),
        }
    }

    pub async fn transcribe(&self, clip: &Clip) -> Result<String, VoiceError> {
        if clip.duration() < MIN_RECORDING {
            return Err(VoiceError::TooShort);
        }
        let language = self.language.as_deref();
        let result = match &self.engine {
            Engine::ElevenLabs(c) => c.transcribe(clip.wav(), language).await,
            Engine::OpenAi(c) => c.transcribe(clip.wav(), language).await,
            Engine::Deepgram(c) => c.transcribe(clip.wav(), language).await,
        };
        result
            .map(|text| text.trim().to_string())
            .map_err(|err| VoiceError::Provider {
                provider: self.name(),
                detail: err.to_string(),
            })
    }
}

fn stt_key_var(provider: SttProvider) -> Option<&'static str> {
    match provider {
        SttProvider::ElevenLabs => Some("ELEVENLABS_API_KEY"),
        SttProvider::OpenAi => Some("OPENAI_API_KEY"),
        SttProvider::Groq => Some("GROQ_API_KEY"),
        SttProvider::Deepgram => Some("DEEPGRAM_API_KEY"),
        SttProvider::OpenAiCompatible => None,
    }
}

fn key(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.trim().is_empty())
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
    let body = String::from_utf8_lossy(&fetch_bytes(req).await?).into_owned();
    serde_json::from_str::<Transcript>(&body)
        .map(|t| t.text)
        .map_err(|e| format!("unreadable reply ({e}): {body}"))
}

async fn fetch_bytes(req: reqwest::RequestBuilder) -> Result<Vec<u8>, String> {
    let res = req.send().await.map_err(|e| e.to_string())?;
    let status = res.status();
    let body = res.bytes().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("{status}: {}", String::from_utf8_lossy(&body)));
    }
    Ok(body.to_vec())
}

fn wav_part(wav: Vec<u8>) -> reqwest::multipart::Part {
    reqwest::multipart::Part::bytes(wav)
        .file_name("dictation.wav")
        .mime_str("audio/wav")
        .expect("audio/wav is a valid mime type")
}
