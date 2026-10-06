//! Text to speech: [`Speaker`] turns a reply into [`Speech`], which plays on
//! the default output device until it ends or its [`Hush`] is set.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::config::{TtsProvider, VoiceConfig};
use crate::{Clip, Missing, VoiceError, elevenlabs, key, openai};

/// Longest text spoken from one reply. Read aloud costs money per character on
/// cloud voices and nobody listens to a wall of text, so the rest is dropped at
/// the last sentence that fits.
pub const MAX_SPOKEN_CHARS: usize = 2_000;

pub(crate) const POLL: Duration = Duration::from_millis(50);

/// Stops speech that is playing. Clones share one switch.
#[derive(Clone, Default)]
pub struct Hush(Arc<AtomicBool>);

impl Hush {
    pub fn now(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Clone)]
pub enum Speaker {
    System {
        voice: Option<String>,
    },
    ElevenLabs {
        client: elevenlabs::Client,
        voice: String,
    },
    OpenAi {
        client: openai::Client,
        voice: String,
    },
    Server {
        client: openai::Client,
        voice: String,
    },
}

impl Speaker {
    pub fn from_config(config: &VoiceConfig) -> Result<Self, VoiceError> {
        let model = |default: &str| config.tts_model.clone().unwrap_or_else(|| default.into());
        let voice = |default: &str| config.tts_voice.clone().unwrap_or_else(|| default.into());
        match config.tts.unwrap_or(TtsProvider::System) {
            TtsProvider::System => Ok(Self::System {
                voice: config.tts_voice.clone(),
            }),
            TtsProvider::ElevenLabs => Ok(Self::ElevenLabs {
                client: elevenlabs::Client::new(
                    key("ELEVENLABS_API_KEY")
                        .ok_or(VoiceError::NotSetUp(Missing::Key("ELEVENLABS_API_KEY")))?,
                    model(elevenlabs::TTS_MODEL),
                ),
                voice: voice(elevenlabs::TTS_VOICE),
            }),
            TtsProvider::OpenAi => Ok(Self::OpenAi {
                client: openai::Client::new(
                    openai::OPENAI_URL,
                    model("gpt-4o-mini-tts"),
                    Some(
                        key("OPENAI_API_KEY")
                            .ok_or(VoiceError::NotSetUp(Missing::Key("OPENAI_API_KEY")))?,
                    ),
                ),
                voice: voice("alloy"),
            }),
            TtsProvider::OpenAiCompatible => Ok(Self::Server {
                client: openai::Client::new(
                    config
                        .tts_url
                        .as_deref()
                        .ok_or(VoiceError::NotSetUp(Missing::Url("tts_url")))?,
                    model("kokoro"),
                    key("ASTER_VOICE_API_KEY"),
                ),
                voice: voice("af_heart"),
            }),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::System { .. } => TtsProvider::System.label(),
            Self::ElevenLabs { .. } => TtsProvider::ElevenLabs.label(),
            Self::OpenAi { .. } => TtsProvider::OpenAi.label(),
            Self::Server { .. } => TtsProvider::OpenAiCompatible.label(),
        }
    }

    /// Fetches the audio for `text`. The system voice has nothing to fetch and
    /// speaks when played.
    pub async fn render(&self, text: &str) -> Result<Speech, VoiceError> {
        let provider = |detail: String| VoiceError::Provider {
            provider: self.name(),
            detail,
        };
        match self {
            Self::System { voice } => Ok(Speech::System {
                text: text.to_string(),
                voice: voice.clone(),
            }),
            Self::ElevenLabs { client, voice } => client
                .speak(text, voice)
                .await
                .map(Speech::Clip)
                .map_err(provider),
            Self::OpenAi { client, voice } | Self::Server { client, voice } => client
                .speak(text, voice)
                .await
                .map(Speech::Clip)
                .map_err(provider),
        }
    }
}

pub enum Speech {
    Clip(Clip),
    System { text: String, voice: Option<String> },
}

impl Speech {
    /// Blocks until the speech ends or `hush` is set.
    pub fn play(self, hush: &Hush) -> Result<(), VoiceError> {
        match self {
            Self::Clip(clip) => crate::playback::play(&clip, hush),
            Self::System { text, voice } => say(&text, voice.as_deref(), hush),
        }
    }
}

fn say(text: &str, voice: Option<&str>, hush: &Hush) -> Result<(), VoiceError> {
    let mut cmd = system_voice(voice);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| VoiceError::NoSpeaker(format!("{e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| VoiceError::NoSpeaker(e.to_string()))?;
    }
    loop {
        if hush.is_set() {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(());
        }
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => std::thread::sleep(POLL),
            Err(e) => return Err(VoiceError::NoSpeaker(e.to_string())),
        }
    }
}

#[cfg(target_os = "macos")]
fn system_voice(voice: Option<&str>) -> Command {
    let mut cmd = Command::new("say");
    if let Some(voice) = voice {
        cmd.args(["-v", voice]);
    }
    cmd.args(["-f", "-"]);
    cmd
}

#[cfg(target_os = "windows")]
fn system_voice(voice: Option<&str>) -> Command {
    let select = voice
        .map(|v| format!("$s.SelectVoice('{}');", v.replace('\'', "''")))
        .unwrap_or_default();
    let mut cmd = Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-Command",
        &format!(
            "Add-Type -AssemblyName System.Speech; \
             $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; {select} \
             $s.Speak([Console]::In.ReadToEnd())"
        ),
    ]);
    cmd
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn system_voice(voice: Option<&str>) -> Command {
    let mut cmd = Command::new("espeak-ng");
    if let Some(voice) = voice {
        cmd.args(["-v", voice]);
    }
    cmd.arg("--stdin");
    cmd
}

/// The prose of a markdown reply, as it should sound: code blocks and tables
/// dropped, markup stripped, links read as their text, and capped at
/// [`MAX_SPOKEN_CHARS`].
pub fn speakable(markdown: &str) -> String {
    let mut out = String::new();
    let mut fenced = false;
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced || trimmed.starts_with('|') || trimmed.is_empty() {
            continue;
        }
        let body = trimmed
            .trim_start_matches('#')
            .trim_start_matches('>')
            .trim_start();
        let body = body
            .strip_prefix("- ")
            .or_else(|| body.strip_prefix("* "))
            .unwrap_or(body);
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&unlink(body).replace(['`', '*'], ""));
    }
    cap(out)
}

fn unlink(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](").map(|i| open + i) else {
            break;
        };
        let Some(end) = rest[close..].find(')').map(|i| close + i) else {
            break;
        };
        out.push_str(&rest[..open]);
        out.push_str(&rest[open + 1..close]);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn cap(text: String) -> String {
    if text.chars().count() <= MAX_SPOKEN_CHARS {
        return text;
    }
    let cut: String = text.chars().take(MAX_SPOKEN_CHARS).collect();
    match cut.rfind(['.', '!', '?']) {
        Some(end) => cut[..=end].to_string(),
        None => cut,
    }
}

#[cfg(test)]
#[path = "speech_tests.rs"]
mod tests;
