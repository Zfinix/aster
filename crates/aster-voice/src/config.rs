use serde::Deserialize;

/// The `voice:` block in `aster.yaml`. Every field is optional: with none set,
/// speech to text uses whichever key is in env and read aloud uses the system
/// voice.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VoiceConfig {
    pub stt: Option<SttProvider>,
    pub stt_model: Option<String>,
    /// Base URL of an OpenAI-compatible server, for `stt: openai-compatible`.
    pub stt_url: Option<String>,
    pub tts: Option<TtsProvider>,
    pub tts_model: Option<String>,
    pub tts_voice: Option<String>,
    /// Base URL of an OpenAI-compatible server, for `tts: openai-compatible`.
    pub tts_url: Option<String>,
    /// Spoken language as an ISO 639-1 code. Unset lets the provider detect it.
    pub language: Option<String>,
    pub read_aloud: Option<bool>,
}

impl VoiceConfig {
    /// The repo's file on top of the global one, field by field.
    pub fn overlaid_with(self, project: Self) -> Self {
        Self {
            stt: project.stt.or(self.stt),
            stt_model: project.stt_model.or(self.stt_model),
            stt_url: project.stt_url.or(self.stt_url),
            tts: project.tts.or(self.tts),
            tts_model: project.tts_model.or(self.tts_model),
            tts_voice: project.tts_voice.or(self.tts_voice),
            tts_url: project.tts_url.or(self.tts_url),
            language: project.language.or(self.language),
            read_aloud: project.read_aloud.or(self.read_aloud),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SttProvider {
    #[serde(rename = "elevenlabs")]
    ElevenLabs,
    #[serde(rename = "openai")]
    OpenAi,
    Groq,
    Deepgram,
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
}

impl SttProvider {
    pub const ALL: &[Self] = &[
        Self::ElevenLabs,
        Self::OpenAi,
        Self::Groq,
        Self::Deepgram,
        Self::OpenAiCompatible,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::ElevenLabs => "elevenlabs",
            Self::OpenAi => "openai",
            Self::Groq => "groq",
            Self::Deepgram => "deepgram",
            Self::OpenAiCompatible => "openai-compatible",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ElevenLabs => "ElevenLabs",
            Self::OpenAi => "OpenAI",
            Self::Groq => "Groq",
            Self::Deepgram => "Deepgram",
            Self::OpenAiCompatible => "Your own server",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TtsProvider {
    System,
    #[serde(rename = "elevenlabs")]
    ElevenLabs,
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
}

impl TtsProvider {
    pub const ALL: &[Self] = &[
        Self::System,
        Self::ElevenLabs,
        Self::OpenAi,
        Self::OpenAiCompatible,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::ElevenLabs => "elevenlabs",
            Self::OpenAi => "openai",
            Self::OpenAiCompatible => "openai-compatible",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System voice",
            Self::ElevenLabs => "ElevenLabs",
            Self::OpenAi => "OpenAI",
            Self::OpenAiCompatible => "Your own server",
        }
    }
}
