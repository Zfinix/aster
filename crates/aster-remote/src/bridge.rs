//! Runs one agent turn by spawning `aster chat --stream` and translating its
//! NDJSON events into typed [`TurnEvent`]s, the same contract the desktop and
//! VS Code front-ends use.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};

/// How long a relayed prompt waits for a tap before it is denied.
const PROMPT_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// One `{"role","content"}` message on the `--messages-json` wire.
#[derive(Clone, Serialize, Deserialize)]
pub struct WireMessage {
    pub role: String,
    pub content: String,
}

impl WireMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
        }
    }
}

/// A reply to a relayed approval prompt.
#[derive(Clone, Copy)]
pub enum Answer {
    Allow,
    /// Allow and persist the request's scope as a grant.
    AlwaysAllow,
    Deny,
}

/// What the running turn needs from the channel adapter.
pub enum TurnEvent {
    /// The agent started a tool call; `arguments` is the raw JSON string.
    ToolCall { name: String, arguments: String },
    /// The agent needs a yes/no from the user before it can continue.
    ApprovalRequest {
        preview: String,
        scope: Option<String>,
        respond: oneshot::Sender<Answer>,
    },
    /// The agent asked a multiple-choice question.
    Question {
        header: String,
        question: String,
        options: Vec<String>,
        respond: oneshot::Sender<Option<String>>,
    },
}

/// The finished turn: the reply text and any files the agent edited.
pub struct TurnOutcome {
    pub reply: String,
    pub edits: Vec<String>,
}

/// Everything constant across one chat's turns.
#[derive(Clone)]
pub struct Turn {
    /// Path to the `aster` binary to spawn.
    pub bin: PathBuf,
    /// Repository the agent operates on.
    pub repo_root: PathBuf,
    /// Session id the turn records into, e.g. `telegram-12345`.
    pub session: String,
    /// Permission mode passed through to `--permission-mode`.
    pub mode: String,
    /// Model override passed through to `--model`.
    pub model: Option<String>,
    /// Reasoning budget passed through to `--effort`.
    pub effort: Option<String>,
    /// Extra environment for the child, e.g. chat context for MCP tools.
    pub extra_env: Vec<(String, String)>,
}

/// Run one turn: spawn the child, feed it the history, pump its events into
/// `events`, and answer its prompts from the oneshot replies.
pub async fn run_turn(
    turn: &Turn,
    messages: &[WireMessage],
    events: &mpsc::Sender<TurnEvent>,
) -> Result<TurnOutcome> {
    let mut command = Command::new(&turn.bin);
    command
        .current_dir(&turn.repo_root)
        .args(["chat", "--stream", "--messages-json", "-"])
        .args(["--session", &turn.session])
        .args(["--permission-mode", &turn.mode]);
    if let Some(model) = &turn.model {
        command.args(["--model", model]);
    }
    if let Some(effort) = &turn.effort {
        command.args(["--effort", effort]);
    }
    for (key, value) in &turn.extra_env {
        command.env(key, value);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("spawning {}", turn.bin.display()))?;

    let mut stdin = child.stdin.take().context("child stdin missing")?;
    let stdout = child.stdout.take().context("child stdout missing")?;
    let stderr = child.stderr.take().context("child stderr missing")?;
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!("aster: {line}");
        }
    });

    // The whole history goes as one line; stdin then stays open for replies.
    let mut wire = serde_json::to_string(messages)?;
    wire.push('\n');
    stdin.write_all(wire.as_bytes()).await?;
    stdin.flush().await?;

    let mut lines = BufReader::new(stdout).lines();
    let mut outcome = None;
    let mut error = None;
    while let Some(line) = lines.next_line().await? {
        let Ok(event) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("tool_call") => {
                let _ = events
                    .send(TurnEvent::ToolCall {
                        name: str_field(&event, "name"),
                        arguments: str_field(&event, "arguments"),
                    })
                    .await;
            }
            Some("approval_request") => {
                let (tx, rx) = oneshot::channel();
                let sent = events
                    .send(TurnEvent::ApprovalRequest {
                        preview: str_field(&event, "preview"),
                        scope: event
                            .get("scope")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        respond: tx,
                    })
                    .await;
                let answer = match sent {
                    Ok(()) => await_or(rx, Answer::Deny).await,
                    Err(_) => Answer::Deny,
                };
                let reply = match answer {
                    Answer::Allow => json!({"allow": true}),
                    Answer::AlwaysAllow => json!({"allow": true, "always": true}),
                    Answer::Deny => json!({"allow": false}),
                };
                write_line(&mut stdin, &reply).await?;
            }
            Some("question") => {
                let options = event
                    .get("options")
                    .and_then(Value::as_array)
                    .map(|opts| {
                        opts.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                let (tx, rx) = oneshot::channel();
                let sent = events
                    .send(TurnEvent::Question {
                        header: str_field(&event, "header"),
                        question: str_field(&event, "question"),
                        options,
                        respond: tx,
                    })
                    .await;
                let choice = match sent {
                    Ok(()) => await_or(rx, None).await,
                    Err(_) => None,
                };
                write_line(&mut stdin, &json!({ "choice": choice })).await?;
            }
            Some("done") => {
                let edits = event
                    .get("edits")
                    .and_then(Value::as_array)
                    .map(|paths| {
                        paths
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                outcome = Some(TurnOutcome {
                    reply: str_field(&event, "reply"),
                    edits,
                });
            }
            Some("error") => error = Some(str_field(&event, "message")),
            _ => {}
        }
    }

    let status = child.wait().await?;
    if let Some(message) = error {
        bail!("agent turn failed: {message}");
    }
    match outcome {
        Some(outcome) => Ok(outcome),
        None => bail!("agent exited ({status}) without a done event"),
    }
}

/// Await a prompt reply, falling back when the adapter drops it or the user
/// never answers within [`PROMPT_TIMEOUT`].
async fn await_or<T>(rx: oneshot::Receiver<T>, fallback: T) -> T {
    match tokio::time::timeout(PROMPT_TIMEOUT, rx).await {
        Ok(Ok(answer)) => answer,
        _ => fallback,
    }
}

async fn write_line(stdin: &mut (impl AsyncWriteExt + Unpin), value: &Value) -> Result<()> {
    let mut line = value.to_string();
    line.push('\n');
    stdin.write_all(line.as_bytes()).await?;
    stdin.flush().await?;
    Ok(())
}

fn str_field(event: &Value, key: &str) -> String {
    event
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}
