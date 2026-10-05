//! Read aloud for finished replies, and `/voice` to see and change the voice
//! setup. Choices are saved to the `voice:` block of the global aster.yaml.

use std::path::Path;

use aster_voice::{Hush, Speaker, SttProvider, Transcriber, TtsProvider, VoiceConfig, speakable};
use ratatui::prelude::*;
use tokio::sync::mpsc;

use super::chat::AppEvent;
use super::theme;
use crate::dictate::{DictationFailure, voice_config};

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

/// Apply a `/voice` argument, then describe the setup that results.
pub(super) fn command(arg: Option<&str>, repo_root: &Path) -> Vec<Line<'static>> {
    let mut words = arg.unwrap_or_default().split_whitespace();
    let saved = match (words.next(), words.next()) {
        (None, _) => None,
        (Some("read"), Some(value @ ("on" | "off"))) => {
            Some(save(repo_root, "read_aloud", (value == "on").to_string()))
        }
        (Some("stt"), Some(id)) => Some(match SttProvider::ALL.iter().find(|p| p.id() == id) {
            Some(p) => save(repo_root, "stt", crate::settings::yaml_scalar(p.id())),
            None => Err(format!("no speech-to-text provider called {id}")),
        }),
        (Some("tts"), Some(id)) => Some(match TtsProvider::ALL.iter().find(|p| p.id() == id) {
            Some(p) => save(repo_root, "tts", crate::settings::yaml_scalar(p.id())),
            None => Err(format!("no voice called {id}")),
        }),
        (Some(other), _) => Some(Err(format!("/voice {other} is not a choice"))),
    };
    let mut lines = vec![Line::from("Voice".bold())];
    match saved {
        Some(Ok(path)) => lines.push(format!("saved to {path}").dim().into()),
        Some(Err(problem)) => lines.push(Line::from(Span::styled(
            problem,
            theme::get().accent_style(),
        ))),
        None => {}
    }
    lines.push("".into());
    match voice_config(repo_root) {
        Ok(config) => lines.extend(status(&config)),
        Err(failure) => {
            lines.push(failure.message.into());
            lines.extend(failure.detail.map(|d| d.dim().into()));
        }
    }
    lines.push("".into());
    let stt: Vec<&str> = SttProvider::ALL.iter().map(|p| p.id()).collect();
    let tts: Vec<&str> = TtsProvider::ALL.iter().map(|p| p.id()).collect();
    for (usage, what) in [
        ("/voice read on|off", "read replies aloud".to_string()),
        ("/voice stt <name>", stt.join(", ")),
        ("/voice tts <name>", tts.join(", ")),
        ("/voice stop", "stop reading (or esc)".to_string()),
    ] {
        lines.push(vec![format!("{usage:<20}").cyan(), what.dim()].into());
    }
    lines
}

fn status(config: &VoiceConfig) -> Vec<Line<'static>> {
    let row = |label: &str, value: String| -> Line<'static> {
        vec![format!("{label:<12}").cyan(), value.into()].into()
    };
    let dictation = match Transcriber::from_config(config) {
        Ok(t) => format!("{} · {} · ctrl+r", t.name(), t.model()),
        Err(err) => DictationFailure::from(err).message,
    };
    let voice = match Speaker::from_config(config) {
        Ok(s) => s.name().to_string(),
        Err(err) => DictationFailure::from(err).message,
    };
    let read = match config.read_aloud.unwrap_or(false) {
        true => "on",
        false => "off",
    };
    vec![
        row("dictation", dictation),
        row("voice", voice),
        row("read aloud", read.to_string()),
    ]
}

fn save(repo_root: &Path, key: &str, value: String) -> Result<String, String> {
    crate::settings::persist_voice(Some(repo_root), key, value)
        .map(|saved| saved.path.display().to_string())
        .map_err(|e| format!("couldn't save that: {e:#}"))
}
