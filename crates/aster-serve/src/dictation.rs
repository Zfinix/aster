//! The mic button in a browser tab. The server runs `aster dictate` for that
//! tab and relays each NDJSON line as a `dictation` event.

use std::process::Stdio;
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};

use crate::state::{AppState, Instance};

const STOPPED: &str = "Recording stopped unexpectedly. Try again.";

pub struct Dictation {
    id: ulid::Ulid,
    child: Child,
    stdin: Option<ChildStdin>,
}

pub async fn handle(state: &AppState, instance: &Arc<Instance>, action: &str) {
    match action {
        "start" => start(state, instance).await,
        "stop" => {
            let mut slot = instance.dictation.lock().await;
            if let Some(mut stdin) = slot.as_mut().and_then(|d| d.stdin.take()) {
                let _ = stdin.write_all(b"\n").await;
            }
        }
        "cancel" => {
            if let Some(mut dictation) = instance.dictation.lock().await.take() {
                let _ = dictation.child.start_kill();
            }
        }
        _ => {}
    }
}

async fn start(state: &AppState, instance: &Arc<Instance>) {
    let mut cmd = state.cli.command(&["dictate"]);
    cmd.stderr(Stdio::null());
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            post(
                instance,
                json!({
                    "type": "error",
                    "message": "Couldn't start Aster to listen. Try again.",
                    "detail": err.to_string(),
                }),
            );
            return;
        }
    };
    let id = ulid::Ulid::new();
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let previous = instance
        .dictation
        .lock()
        .await
        .replace(Dictation { id, child, stdin });
    if let Some(mut previous) = previous {
        let _ = previous.child.start_kill();
    }
    let Some(stdout) = stdout else {
        return;
    };

    let instance = Arc::clone(instance);
    tokio::spawn(async move {
        let mut settled = false;
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(event) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if !is_current(&instance, id).await {
                return;
            }
            settled |= matches!(event["type"].as_str(), Some("transcript" | "error"));
            post(&instance, event);
        }
        let mut slot = instance.dictation.lock().await;
        if slot.as_ref().is_some_and(|d| d.id == id) {
            *slot = None;
            if !settled {
                post(
                    &instance,
                    json!({ "type": "error", "message": STOPPED, "detail": null }),
                );
            }
        }
    });
}

async fn is_current(instance: &Instance, id: ulid::Ulid) -> bool {
    instance
        .dictation
        .lock()
        .await
        .as_ref()
        .is_some_and(|d| d.id == id)
}

fn post(instance: &Instance, event: Value) {
    instance.post(json!({ "type": "dictation", "event": event }));
}
