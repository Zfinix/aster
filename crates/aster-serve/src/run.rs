//! The two kinds of run a browser can start: a chat turn and a review. Each
//! owns at most one child, streams its NDJSON stdout to every tab, and answers
//! prompts on the stdin it keeps open.

use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdout};
use tokio::sync::oneshot;

use crate::state::AppState;

/// A live child. Cancelling drops the sender, which the waiter reads as a kill.
pub struct Run {
    stdin: Option<tokio::process::ChildStdin>,
    cancel: Option<oneshot::Sender<()>>,
}

impl Run {
    pub async fn write(&mut self, line: &str) -> Result<(), String> {
        let stdin = self.stdin.as_mut().ok_or("this turn takes no answers")?;
        stdin
            .write_all(format!("{line}\n").as_bytes())
            .await
            .map_err(|e| format!("could not answer the prompt: {e}"))
    }
}

/// Start a chat turn. Returns once the child is up: the turn's own end arrives
/// as a `done` or `error` event, and a child that dies without one is reported
/// as a `chatError`.
pub async fn chat(state: &Arc<AppState>, id: String, message: &Value) -> Result<(), String> {
    let settings = state.settings.lock().await;
    let mode = message
        .get("permissionMode")
        .and_then(Value::as_str)
        .unwrap_or(&settings.permission_mode)
        .to_string();
    let provider = settings.provider.clone();
    drop(settings);

    let mut args: Vec<String> = vec![
        "chat".into(),
        "--messages-json".into(),
        "-".into(),
        "--stream".into(),
        "--permission-mode".into(),
        mode,
    ];
    // Only when the browser picked one: the flag outranks ASTER_EFFORT and
    // aster.yaml, so sending a default would silently override the repo's.
    for (flag, value) in [
        ("--effort", message.get("effort")),
        ("--model", message.get("model")),
        ("--session", message.get("session")),
    ] {
        if let Some(value) = value.and_then(Value::as_str).filter(|v| !v.is_empty()) {
            args.push(flag.into());
            args.push(value.into());
        }
    }

    // Claim the slot before spawning: a refused turn must not leave a child
    // behind, and two turns must never share a repo.
    let mut slot = state.chat.lock().await;
    if slot.is_some() {
        return Err("A turn is already running.".into());
    }
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut child = state
        .cli
        .command(&argv, provider.as_ref())
        .spawn()
        .map_err(|e| format!("could not launch aster: {e}"))?;

    // The messages go in as a single line; stdin then stays open, because that
    // is the channel the CLI reads approval replies from.
    let mut stdin = child.stdin.take().ok_or("aster chat has no stdin")?;
    let messages = message
        .get("messages")
        .cloned()
        .unwrap_or_else(|| json!([]));
    stdin
        .write_all(format!("{messages}\n").as_bytes())
        .await
        .map_err(|e| format!("could not send messages to aster: {e}"))?;

    log(child.stderr.take());
    let terminal = stream(state, child.stdout.take(), {
        let id = id.clone();
        move |event| {
            let terminal = matches!(
                event.get("type").and_then(Value::as_str),
                Some("done" | "error")
            );
            (
                json!({ "type": "chatEvent", "id": id, "event": event }),
                terminal,
            )
        }
    });

    let (cancel, cancelled) = oneshot::channel();
    *slot = Some(Run {
        stdin: Some(stdin),
        cancel: Some(cancel),
    });
    drop(slot);

    let state = state.clone();
    tokio::spawn(async move {
        let code = wait(child, cancelled).await;
        state.chat.lock().await.take();
        // A turn that ended on its own already said so. One that did not left
        // the UI waiting, so the exit code is the only explanation available.
        if let Some(code) = code
            && !terminal.load(std::sync::atomic::Ordering::SeqCst)
        {
            state.post(json!({
                "type": "chatError",
                "id": id,
                "message": format!("aster chat exited with code {code}. See the terminal running aster serve."),
            }));
        }
        state.post_run_state().await;
    });
    Ok(())
}

/// Start a review. Findings stream as they land; the run ends with `reviewDone`
/// or `reviewError`.
pub async fn review(state: &Arc<AppState>, id: String, source: &Value) -> Result<(), String> {
    let mut args: Vec<String> = vec!["review".into(), "--stream".into()];
    let kind = source
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("working");
    if let Some(value) = source.get("value").and_then(Value::as_str)
        && matches!(kind, "range" | "pr")
    {
        args.push(format!("--{kind}"));
        args.push(value.into());
    }

    let provider = state.settings.lock().await.provider.clone();
    let mut slot = state.review.lock().await;
    if slot.is_some() {
        return Err("A review is already running.".into());
    }
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut child = state
        .cli
        .command(&argv, provider.as_ref())
        .spawn()
        .map_err(|e| format!("could not launch aster: {e}"))?;

    // Nothing answers a review, so its stdin is closed at once.
    drop(child.stdin.take());
    log(child.stderr.take());
    stream(state, child.stdout.take(), {
        let id = id.clone();
        move |event| {
            (
                json!({ "type": "reviewEvent", "id": id, "event": event }),
                false,
            )
        }
    });

    let (cancel, cancelled) = oneshot::channel();
    *slot = Some(Run {
        stdin: None,
        cancel: Some(cancel),
    });
    drop(slot);

    let state = state.clone();
    tokio::spawn(async move {
        let code = wait(child, cancelled).await;
        state.review.lock().await.take();
        match code {
            // Cancelled: the browser asked, so it is not news.
            None => {}
            Some(0) => state.post(json!({ "type": "reviewDone", "id": id })),
            Some(code) => state.post(json!({
                "type": "reviewError",
                "id": id,
                "message": format!("aster exited with code {code}. See the terminal running aster serve."),
            })),
        }
        state.post_run_state().await;
    });
    Ok(())
}

/// Stop a run. The waiter frees the slot and reports what happened.
pub async fn cancel(run: &mut Option<Run>) {
    if let Some(run) = run.as_mut()
        && let Some(cancel) = run.cancel.take()
    {
        let _ = cancel.send(());
    }
}

/// Forward NDJSON lines as they arrive, wrapped by `wrap` into the message the
/// browser expects. The flag it returns rides back so a caller can tell whether
/// the stream ever reached its own ending.
fn stream<F>(
    state: &Arc<AppState>,
    stdout: Option<ChildStdout>,
    wrap: F,
) -> Arc<std::sync::atomic::AtomicBool>
where
    F: Fn(Value) -> (Value, bool) + Send + 'static,
{
    let terminal = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let Some(stdout) = stdout else {
        return terminal;
    };
    let (state, saw) = (state.clone(), terminal.clone());
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(line) {
                Ok(event) => {
                    let (message, ended) = wrap(event);
                    if ended {
                        saw.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    state.post(message);
                }
                // Not every line a child prints is an event; a stray one is
                // log material, not a reason to tear the stream down.
                Err(_) => tracing::debug!("{line}"),
            }
        }
    });
    terminal
}

/// The CLI's stderr is its log feed. The terminal running the server is where
/// it goes, which is the same place it would go without a browser attached.
fn log(stderr: Option<ChildStderr>) {
    let Some(stderr) = stderr else { return };
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if !line.trim().is_empty() {
                eprintln!("{line}");
            }
        }
    });
}

/// Wait for the child, or kill it when the browser cancels. `None` means it was
/// cancelled or died on a signal, which is nobody's error to report.
async fn wait(mut child: Child, cancelled: oneshot::Receiver<()>) -> Option<i32> {
    tokio::select! {
        status = child.wait() => status.ok().and_then(|status| status.code()),
        _ = cancelled => {
            let _ = child.kill().await;
            None
        }
    }
}
