//! `aster speak`: read text aloud with the voice from aster.yaml and print
//! NDJSON. Front-ends pass the text as an argument and stop it by sending a
//! line or closing stdin.

use std::io::{BufRead, Read};

use anyhow::Result;
use aster_voice::{Hush, Speaker, speakable};
use clap::Args;
use serde_json::json;

use crate::dictate::{DictationFailure, cwd, voice_config};

#[derive(Args, Debug)]
pub struct SpeakArgs {
    /// Text to read. Without it, the text is read from stdin until it closes.
    pub text: Option<String>,
}

pub(crate) async fn run(args: SpeakArgs) -> Result<()> {
    let event = match speak(args).await {
        Ok(()) => json!({ "type": "done" }),
        Err(failure) => json!({
            "type": "error",
            "message": failure.message,
            "detail": failure.detail,
        }),
    };
    println!("{event}");
    Ok(())
}

async fn speak(args: SpeakArgs) -> Result<(), DictationFailure> {
    let speaker = Speaker::from_config(&voice_config(&cwd())?)?;
    let from_stdin = args.text.is_none();
    let text = match args.text {
        Some(text) => text,
        None => {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| DictationFailure::interrupted(e.to_string()))?;
            text
        }
    };
    let speech = speaker.render(&speakable(&text)).await?;
    println!("{}", json!({ "type": "speaking" }));
    let hush = Hush::default();
    if !from_stdin {
        let stop = hush.clone();
        std::thread::spawn(move || {
            let _ = std::io::stdin().lock().read_line(&mut String::new());
            stop.now();
        });
    }
    tokio::task::spawn_blocking(move || speech.play(&hush))
        .await
        .map_err(|e| DictationFailure::interrupted(e.to_string()))??;
    Ok(())
}
