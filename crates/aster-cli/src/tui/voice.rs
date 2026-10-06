//! Read aloud for finished replies, and the `/voice` panel that picks the
//! services. Choices are saved to the `voice:` block of the global aster.yaml.

use std::path::Path;

use aster_voice::{
    Hush, Missing, Speaker, SttProvider, Transcriber, TtsProvider, VoiceConfig, VoiceError,
    speakable,
};
use tokio::sync::mpsc;

use super::bottom_pane::SelectionItem;
use super::chat::AppEvent;
use crate::dictate::{DictationFailure, voice_config};
use crate::settings::yaml_scalar;

/// The reply being spoken, if any. Its [`Hush`] is set once speech ends, so a
/// set switch means nothing is playing.
#[derive(Default)]
pub(super) struct ReadAloud {
    playing: Option<Hush>,
}

impl ReadAloud {
    pub(super) fn start(
        &mut self,
        reply: &str,
        repo_root: &Path,
        tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        self.stop();
        let config = match voice_config(repo_root) {
            Ok(config) => config,
            Err(failure) => {
                let _ = tx.send(AppEvent::ReadAloudFailed(failure));
                return;
            }
        };
        let text = speakable(reply);
        if !config.read_aloud.unwrap_or(false) || text.is_empty() {
            return;
        }
        let speaker = match Speaker::from_config(&config) {
            Ok(speaker) => speaker,
            Err(err) => {
                let _ = tx.send(AppEvent::ReadAloudFailed(err.into()));
                return;
            }
        };
        let hush = Hush::default();
        self.playing = Some(hush.clone());
        let tx = tx.clone();
        tokio::spawn(async move {
            let ended = hush.clone();
            let result = async {
                let speech = speaker.render(&text).await?;
                tokio::task::spawn_blocking(move || speech.play(&hush))
                    .await
                    .map_err(|e| DictationFailure::interrupted(e.to_string()))??;
                Ok::<_, DictationFailure>(())
            }
            .await;
            let quiet = ended.is_set();
            ended.now();
            if let Err(failure) = result
                && !quiet
            {
                let _ = tx.send(AppEvent::ReadAloudFailed(failure));
            }
        });
    }

    /// Stops speech. True when something was playing, so the key that asked
    /// for it does nothing else.
    pub(super) fn stop(&mut self) -> bool {
        match self.playing.take() {
            Some(hush) if !hush.is_set() => {
                hush.now();
                true
            }
            _ => false,
        }
    }
}

/// What a row in the `/voice` panel does when picked.
#[derive(Clone, Copy)]
pub(super) enum VoiceAction {
    Open,
    ChooseStt,
    ChooseTts,
    ReadAloud(bool),
    Stt(Option<SttProvider>),
    Tts(Option<TtsProvider>),
}

pub(super) struct Panel {
    pub(super) title: &'static str,
    pub(super) items: Vec<SelectionItem<AppEvent>>,
    pub(super) back: Option<AppEvent>,
}

/// Apply `action`, then build the panel to show next. A failed save comes
/// back as the error, with the panel still built from what is on disk.
pub(super) fn handle(
    action: VoiceAction,
    repo_root: &Path,
) -> (Result<Panel, DictationFailure>, Option<String>) {
    let saved = match action {
        VoiceAction::Open | VoiceAction::ChooseStt | VoiceAction::ChooseTts => None,
        VoiceAction::ReadAloud(on) => Some(save(repo_root, "read_aloud", Some(on.to_string()))),
        VoiceAction::Stt(p) => Some(save(repo_root, "stt", p.map(|p| yaml_scalar(p.id())))),
        VoiceAction::Tts(p) => Some(save(repo_root, "tts", p.map(|p| yaml_scalar(p.id())))),
    };
    let problem = saved.and_then(Result::err);
    let panel = voice_config(repo_root).map(|config| match action {
        VoiceAction::ChooseStt => stt_panel(&config),
        VoiceAction::ChooseTts => tts_panel(&config),
        VoiceAction::Open
        | VoiceAction::ReadAloud(_)
        | VoiceAction::Stt(_)
        | VoiceAction::Tts(_) => main_panel(&config),
    });
    (panel, problem)
}

fn main_panel(config: &VoiceConfig) -> Panel {
    let read = config.read_aloud.unwrap_or(false);
    let voice = match Speaker::from_config(config) {
        Ok(s) => s.name().to_string(),
        Err(err) => needs(&err),
    };
    let dictation = match Transcriber::from_config(config) {
        Ok(t) => format!("{} · press ctrl+r in chat to talk", t.name()),
        Err(err) => needs(&err),
    };
    let item = |name: String, description: String, action| SelectionItem {
        name,
        description,
        is_current: false,
        event: AppEvent::Voice(action),
    };
    Panel {
        title: "Voice · enter changes a setting, esc closes",
        items: vec![
            item(
                format!("{} Read replies aloud", if read { "◼" } else { "◻" }),
                match read {
                    true => format!("on · {voice} · esc stops it"),
                    false => "off".to_string(),
                },
                VoiceAction::ReadAloud(!read),
            ),
            item("Reading voice".to_string(), voice, VoiceAction::ChooseTts),
            item("Dictation".to_string(), dictation, VoiceAction::ChooseStt),
        ],
        back: None,
    }
}

fn stt_panel(config: &VoiceConfig) -> Panel {
    let automatic = VoiceConfig {
        stt: None,
        ..config.clone()
    };
    let mut items = vec![SelectionItem {
        name: "Automatic".to_string(),
        description: match Transcriber::from_config(&automatic) {
            Ok(t) => format!("the first service with a key · now {}", t.name()),
            Err(err) => needs(&err),
        },
        is_current: config.stt.is_none(),
        event: AppEvent::Voice(VoiceAction::Stt(None)),
    }];
    items.extend(SttProvider::ALL.iter().map(|&p| {
        let trial = VoiceConfig {
            stt: Some(p),
            ..config.clone()
        };
        SelectionItem {
            name: p.label().to_string(),
            description: match Transcriber::from_config(&trial) {
                Ok(t) => format!("{} · ready", stt_blurb(p, t.model())),
                Err(err) => format!("{} · {}", stt_blurb(p, ""), needs(&err)),
            },
            is_current: config.stt == Some(p),
            event: AppEvent::Voice(VoiceAction::Stt(Some(p))),
        }
    }));
    Panel {
        title: "Dictation · what turns your voice into text",
        items,
        back: Some(AppEvent::Voice(VoiceAction::Open)),
    }
}

fn tts_panel(config: &VoiceConfig) -> Panel {
    let items = TtsProvider::ALL
        .iter()
        .map(|&p| {
            let trial = VoiceConfig {
                tts: Some(p),
                ..config.clone()
            };
            let blurb = match p {
                TtsProvider::System => "built into your computer, free",
                TtsProvider::ElevenLabs => "natural voices",
                TtsProvider::OpenAi => "natural voices",
                TtsProvider::OpenAiCompatible => "Kokoro or another engine you run",
            };
            SelectionItem {
                name: p.label().to_string(),
                description: match Speaker::from_config(&trial) {
                    Ok(_) => format!("{blurb} · ready"),
                    Err(err) => format!("{blurb} · {}", needs(&err)),
                },
                is_current: config.tts.unwrap_or(TtsProvider::System) == p,
                event: AppEvent::Voice(VoiceAction::Tts(Some(p))),
            }
        })
        .collect();
    Panel {
        title: "Reading voice · what reads replies aloud",
        items,
        back: Some(AppEvent::Voice(VoiceAction::Open)),
    }
}

fn stt_blurb(provider: SttProvider, model: &str) -> String {
    let what = match provider {
        SttProvider::ElevenLabs => "Scribe",
        SttProvider::OpenAi => "GPT-4o transcribe",
        SttProvider::Groq => "free tier, hosted Whisper",
        SttProvider::Deepgram => "Nova",
        SttProvider::OpenAiCompatible => "Parakeet or another engine you run",
    };
    match (provider, model) {
        (SttProvider::OpenAiCompatible, model) if !model.is_empty() => format!("{what} · {model}"),
        _ => what.to_string(),
    }
}

/// What is missing, short enough for one row.
fn needs(err: &VoiceError) -> String {
    match err {
        VoiceError::NotSetUp(Missing::AnyKey) => "needs a key · run aster key list".to_string(),
        VoiceError::NotSetUp(Missing::Key(var)) => format!("needs a key · aster key set {var}"),
        VoiceError::NotSetUp(Missing::Url(key)) => {
            format!("needs its address · set voice.{key}")
        }
        VoiceError::Unsupported
        | VoiceError::NoMicrophone(_)
        | VoiceError::NoSpeaker(_)
        | VoiceError::TooShort
        | VoiceError::Provider { .. } => err.to_string(),
    }
}

fn save(repo_root: &Path, key: &str, value: Option<String>) -> Result<(), String> {
    match value {
        Some(value) => crate::settings::persist_voice(Some(repo_root), key, value).map(drop),
        None => crate::settings::clear_voice(Some(repo_root), key),
    }
    .map_err(|e| format!("couldn't save that: {e:#}"))
}
