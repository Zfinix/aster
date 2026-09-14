//! Two ways to run an agent turn, both translated into typed [`TurnEvent`]s:
//! [`Agent`] keeps one `aster acp` process per chat and drives it as an ACP
//! client, the way Zed and the VS Code extension do, so a message never pays
//! for a cold start; [`run_turn`] spawns `aster chat --stream` per turn.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, oneshot};

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

#[derive(Clone, Copy)]
pub enum Answer {
    Allow,
    AlwaysAllow,
    Deny,
}

/// What the running turn needs from the channel adapter.
pub enum TurnEvent {
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    ToolResult {
        id: String,
        error: bool,
        /// The start of what the tool returned, so a card can show the work.
        output: String,
    },
    /// A piece of what the agent is saying, as it says it.
    Text { content: String },
    /// A piece of the agent's reasoning, as it thinks it.
    Thought { content: String },
    ApprovalRequest {
        preview: String,
        scope: Option<String>,
        respond: oneshot::Sender<Answer>,
    },
    Question {
        header: String,
        question: String,
        options: Vec<String>,
        respond: oneshot::Sender<Option<String>>,
    },
}

pub struct TurnOutcome {
    pub reply: String,
    pub edits: Vec<String>,
}

/// Everything constant across one chat's turns.
#[derive(Clone)]
pub struct Turn {
    pub bin: PathBuf,
    pub repo_root: PathBuf,
    pub session: String,
    pub mode: String,
    pub model: Option<String>,
    pub effort: Option<String>,
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
                        id: str_field(&event, "id"),
                        name: str_field(&event, "name"),
                        arguments: str_field(&event, "arguments"),
                    })
                    .await;
            }
            Some("tool_result") => {
                let _ = events
                    .send(TurnEvent::ToolResult {
                        id: str_field(&event, "id"),
                        error: event.get("error").and_then(Value::as_bool).unwrap_or(false),
                        output: excerpt(&str_field(&event, "result")),
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

/// Ask the model one question with no tools and no session, returning its
/// plain-text answer. For side work like writing a commit message, where a
/// full agent turn would cost tool rounds and approval prompts.
pub async fn ask_once(bin: &Path, repo_root: &Path, prompt: &str) -> Result<String> {
    let output = Command::new(bin)
        .current_dir(repo_root)
        .args(["chat", "--print", "--no-tools", prompt])
        .output()
        .await
        .with_context(|| format!("spawning {}", bin.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

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

/// How much of a tool's output rides along in its event.
const OUTPUT_EXCERPT: usize = 700;

fn excerpt(text: &str) -> String {
    let mut cut = text.trim().chars().take(OUTPUT_EXCERPT).collect::<String>();
    if text.trim().chars().count() > OUTPUT_EXCERPT {
        cut.push('…');
    }
    cut
}

type Waiters = HashMap<u64, oneshot::Sender<Result<Value, String>>>;

enum Incoming {
    Update(Value),
    Request {
        id: Value,
        method: String,
        params: Value,
    },
}

#[derive(Clone, Default, PartialEq)]
struct Applied {
    model: Option<String>,
    effort: Option<String>,
    mode: Option<String>,
}

/// One long-lived `aster acp` process, driven as an ACP client over stdio. The
/// session keeps its own history, so each message sends only itself.
pub struct Agent {
    stdin: tokio::sync::Mutex<ChildStdin>,
    next_id: AtomicU64,
    pending: Arc<Mutex<Waiters>>,
    inbox: Arc<Mutex<Option<mpsc::Sender<Incoming>>>>,
    alive: Arc<AtomicBool>,
    session: Mutex<Option<String>>,
    primed: AtomicBool,
    applied: Mutex<Applied>,
    reader: Mutex<Option<tokio::task::JoinHandle<()>>>,
    child: Mutex<Option<tokio::process::Child>>,
}

impl Agent {
    pub async fn spawn(turn: &Turn) -> Result<Arc<Self>> {
        let mut command = Command::new(&turn.bin);
        command
            .current_dir(&turn.repo_root)
            .arg("acp")
            .args(["--permission-mode", &turn.mode]);
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
        let stdin = child.stdin.take().context("agent stdin missing")?;
        let stdout = child.stdout.take().context("agent stdout missing")?;
        let stderr = child.stderr.take().context("agent stderr missing")?;
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!("aster acp: {line}");
            }
        });

        let agent = Arc::new(Self {
            stdin: tokio::sync::Mutex::new(stdin),
            next_id: AtomicU64::new(1),
            pending: Arc::default(),
            inbox: Arc::default(),
            alive: Arc::new(AtomicBool::new(true)),
            session: Mutex::new(None),
            primed: AtomicBool::new(false),
            applied: Mutex::new(Applied::default()),
            reader: Mutex::new(None),
            child: Mutex::new(Some(child)),
        });

        let reader = Arc::downgrade(&agent);
        let handle = tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(message) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let Some(reader) = reader.upgrade() else {
                    break;
                };
                reader.route(message).await;
            }
            if let Some(reader) = reader.upgrade() {
                // stdout closed, so the process is gone or going; every waiter
                // should know rather than hang on a reply that will not come.
                reader.alive.store(false, Ordering::Relaxed);
                for (_, waiter) in reader.pending.lock().expect("pending lock").drain() {
                    let _ = waiter.send(Err("the agent went away".into()));
                }
            }
        });
        *agent.reader.lock().expect("reader lock") = Some(handle);

        agent
            .call(
                "initialize",
                json!({
                    "protocolVersion": 1,
                    "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false } },
                    "clientInfo": { "name": "aster-remote", "title": "Aster", "version": env!("CARGO_PKG_VERSION") },
                }),
            )
            .await?;
        Ok(agent)
    }

    /// Responses settle their waiter; everything the agent asks or announces
    /// goes to the turn in progress, or is answered as cancelled when none is.
    async fn route(&self, message: Value) {
        let id = message.get("id").cloned();
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_string);
        match (id, method) {
            (Some(id), Some(method)) => {
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                let inbox = self.inbox.lock().expect("inbox lock").clone();
                let delivered = match inbox {
                    Some(inbox) => inbox
                        .send(Incoming::Request {
                            id: id.clone(),
                            method,
                            params,
                        })
                        .await
                        .is_ok(),
                    None => false,
                };
                if !delivered {
                    let _ = self
                        .respond(&id, json!({ "outcome": { "outcome": "cancelled" } }))
                        .await;
                }
            }
            (Some(id), None) => {
                let Some(id) = id.as_u64() else { return };
                let waiter = self.pending.lock().expect("pending lock").remove(&id);
                if let Some(waiter) = waiter {
                    let outcome = match message.get("error") {
                        Some(error) => Err(error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("request failed")
                            .to_string()),
                        None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                    };
                    let _ = waiter.send(outcome);
                }
            }
            (None, Some(method)) if method == "session/update" => {
                let inbox = self.inbox.lock().expect("inbox lock").clone();
                if let Some(inbox) = inbox {
                    let params = message.get("params").cloned().unwrap_or(Value::Null);
                    let _ = inbox.send(Incoming::Update(params)).await;
                }
            }
            _ => {}
        }
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    async fn write(&self, value: &Value) -> Result<()> {
        let mut stdin = self.stdin.lock().await;
        write_line(&mut *stdin, value).await
    }

    async fn respond(&self, id: &Value, result: Value) -> Result<()> {
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "result": result }))
            .await
    }

    fn request(
        &self,
        method: &str,
        params: Value,
    ) -> (Value, oneshot::Receiver<Result<Value, String>>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().expect("pending lock").insert(id, tx);
        (
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
            rx,
        )
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let (line, rx) = self.request(method, params);
        self.write(&line).await?;
        match tokio::time::timeout(PROMPT_TIMEOUT, rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(message))) => bail!("{method}: {message}"),
            Ok(Err(_)) => bail!("{method}: the agent went away"),
            Err(_) => bail!("{method}: timed out"),
        }
    }

    /// The session this chat talks in: the one already open, the saved one
    /// loaded back, or a new one when there is nothing to load.
    pub async fn ensure_session(&self, cwd: &Path, wanted: Option<&str>) -> Result<String> {
        if let Some(id) = self.session.lock().expect("session lock").clone() {
            return Ok(id);
        }
        let params = json!({ "cwd": cwd.display().to_string(), "mcpServers": [] });
        if let Some(wanted) = wanted {
            let mut load = params.clone();
            load["sessionId"] = Value::String(wanted.to_string());
            match self.call("session/load", load).await {
                Ok(loaded) => {
                    let id = loaded
                        .get("sessionId")
                        .and_then(Value::as_str)
                        .unwrap_or(wanted)
                        .to_string();
                    *self.session.lock().expect("session lock") = Some(id.clone());
                    self.primed.store(true, Ordering::Relaxed);
                    return Ok(id);
                }
                Err(err) => tracing::info!("session {wanted} not loaded, starting fresh: {err:#}"),
            }
        }
        let created = self.call("session/new", params).await?;
        let id = created
            .get("sessionId")
            .and_then(Value::as_str)
            .context("session/new returned no id")?
            .to_string();
        *self.session.lock().expect("session lock") = Some(id.clone());
        self.primed.store(false, Ordering::Relaxed);
        Ok(id)
    }

    /// Forget the session so the next message starts a new one.
    pub fn reset_session(&self) {
        *self.session.lock().expect("session lock") = None;
        *self.applied.lock().expect("applied lock") = Applied::default();
    }

    /// Whether the session has already been given its standing instructions.
    pub fn primed(&self) -> bool {
        self.primed.swap(true, Ordering::Relaxed)
    }

    /// Model, effort, and mode, sent only when they differ from what the
    /// session already has. A refused option is logged, not fatal.
    pub async fn configure(&self, session: &str, turn: &Turn) {
        let want = Applied {
            model: turn.model.clone(),
            effort: turn.effort.clone(),
            mode: Some(turn.mode.clone()),
        };
        let have = self.applied.lock().expect("applied lock").clone();
        for (config, value, was) in [
            ("model", &want.model, &have.model),
            ("effort", &want.effort, &have.effort),
        ] {
            if let Some(value) = value
                && value != was.as_deref().unwrap_or("")
                && let Err(err) = self
                    .call(
                        "session/set_config_option",
                        json!({ "sessionId": session, "configId": config, "value": value }),
                    )
                    .await
            {
                tracing::warn!("could not set {config}: {err:#}");
            }
        }
        if want.mode != have.mode
            && let Err(err) = self
                .call(
                    "session/set_mode",
                    json!({ "sessionId": session, "modeId": turn.mode }),
                )
                .await
        {
            tracing::warn!("could not set the mode: {err:#}");
        }
        *self.applied.lock().expect("applied lock") = want;
    }

    /// One turn: send the prompt, relay everything the agent says and asks
    /// until the prompt call returns.
    pub async fn prompt(
        &self,
        session: &str,
        text: &str,
        events: &mpsc::Sender<TurnEvent>,
    ) -> Result<TurnOutcome> {
        let (inbox_tx, mut inbox) = mpsc::channel::<Incoming>(64);
        *self.inbox.lock().expect("inbox lock") = Some(inbox_tx);
        let (line, mut rx) = self.request(
            "session/prompt",
            json!({ "sessionId": session, "prompt": [{ "type": "text", "text": text }] }),
        );
        let started = self.write(&line).await;
        let mut reply = String::new();
        let mut edits = Vec::new();
        let deadline = tokio::time::sleep(PROMPT_TIMEOUT);
        tokio::pin!(deadline);
        let outcome = match started {
            Err(err) => Err(err),
            Ok(()) => loop {
                tokio::select! {
                    response = &mut rx => break match response {
                        Ok(Ok(_)) => Ok(()),
                        Ok(Err(message)) => Err(anyhow!("agent turn failed: {message}")),
                        Err(_) => Err(anyhow!("the agent went away mid-turn")),
                    },
                    incoming = inbox.recv() => match incoming {
                        Some(Incoming::Update(params)) => {
                            self.on_update(&params, &mut reply, &mut edits, events).await;
                        }
                        Some(Incoming::Request { id, method, params }) => {
                            self.on_request(&id, &method, &params, events).await;
                        }
                        None => break Err(anyhow!("the agent closed its stream")),
                    },
                    _ = &mut deadline => break Err(anyhow!("the turn timed out")),
                }
            },
        };
        *self.inbox.lock().expect("inbox lock") = None;
        outcome.map(|()| TurnOutcome { reply, edits })
    }

    /// Hand a message to the turn already in flight, so it lands in the same
    /// conversation instead of starting a second one. False when there was no
    /// turn left to join and the caller still owns the message.
    pub async fn steer(&self, text: &str) -> Result<bool> {
        let session = self.session.lock().expect("session lock").clone();
        let Some(session) = session.filter(|_| self.inbox.lock().expect("inbox lock").is_some())
        else {
            return Ok(false);
        };
        let answer = self
            .call(
                "session/prompt",
                json!({
                    "sessionId": session,
                    "prompt": [{ "type": "text", "text": text }],
                    "_meta": { "steerOnly": true },
                }),
            )
            .await?;
        Ok(answer
            .get("_meta")
            .and_then(|meta| meta.get("steered"))
            .and_then(Value::as_bool)
            .unwrap_or(false))
    }

    async fn on_update(
        &self,
        params: &Value,
        reply: &mut String,
        edits: &mut Vec<String>,
        events: &mpsc::Sender<TurnEvent>,
    ) {
        let Some(update) = params.get("update") else {
            return;
        };
        let event = match update.get("sessionUpdate").and_then(Value::as_str) {
            Some("agent_message_chunk") => {
                let content = content_text(&update["content"]);
                reply.push_str(&content);
                TurnEvent::Text { content }
            }
            Some("agent_thought_chunk") => TurnEvent::Thought {
                content: content_text(&update["content"]),
            },
            Some("tool_call") => {
                if update.get("kind").and_then(Value::as_str) == Some("edit")
                    && let Some(path) = update
                        .get("locations")
                        .and_then(Value::as_array)
                        .and_then(|l| l.first())
                        .and_then(|l| l.get("path"))
                        .and_then(Value::as_str)
                    && !edits.iter().any(|e| e == path)
                {
                    edits.push(path.to_string());
                }
                let raw = update.get("rawInput").cloned().unwrap_or(json!({}));
                TurnEvent::ToolCall {
                    id: str_field(update, "toolCallId"),
                    name: update
                        .get("name")
                        .or_else(|| update.get("title"))
                        .and_then(Value::as_str)
                        .unwrap_or("tool")
                        .to_string(),
                    arguments: match raw {
                        Value::String(s) => s,
                        other => other.to_string(),
                    },
                }
            }
            Some("tool_call_update") => {
                let status = update.get("status").and_then(Value::as_str).unwrap_or("");
                if status != "completed" && status != "failed" {
                    return;
                }
                let mut output = content_text(&update["content"]);
                if output.is_empty()
                    && let Some(raw) = update.get("rawOutput")
                {
                    output = match raw {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                }
                TurnEvent::ToolResult {
                    id: str_field(update, "toolCallId"),
                    error: status == "failed",
                    output: excerpt(&output),
                }
            }
            Some("plan") => {
                let steps: Vec<Value> = update
                    .get("entries")
                    .and_then(Value::as_array)
                    .map(|entries| {
                        entries
                            .iter()
                            .map(|e| json!({ "label": str_field(e, "content"), "status": str_field(e, "status") }))
                            .collect()
                    })
                    .unwrap_or_default();
                TurnEvent::ToolCall {
                    id: "plan".into(),
                    name: "update_plan".into(),
                    arguments: json!({ "steps": steps }).to_string(),
                }
            }
            _ => return,
        };
        let _ = events.send(event).await;
    }

    /// A permission request is either a question (the agent's ask_user, sent
    /// as a `think` call with one option per answer) or an approval.
    async fn on_request(
        &self,
        id: &Value,
        method: &str,
        params: &Value,
        events: &mpsc::Sender<TurnEvent>,
    ) {
        if method != "session/request_permission" {
            let _ = self
                .write(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32601, "message": format!("{method} is not supported") },
                }))
                .await;
            return;
        }
        let call = &params["toolCall"];
        let options: Vec<(String, String, String)> = params
            .get("options")
            .and_then(Value::as_array)
            .map(|opts| {
                opts.iter()
                    .map(|o| {
                        (
                            str_field(o, "optionId"),
                            str_field(o, "name"),
                            str_field(o, "kind"),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let title = str_field(call, "title");
        let body = content_text(&call["content"]);
        let picked = if call.get("kind").and_then(Value::as_str) == Some("think") {
            let (tx, rx) = oneshot::channel();
            let sent = events
                .send(TurnEvent::Question {
                    header: title,
                    question: body,
                    options: options
                        .iter()
                        .filter(|(_, _, kind)| kind == "allow_once")
                        .map(|(_, name, _)| name.clone())
                        .collect(),
                    respond: tx,
                })
                .await;
            let choice = match sent {
                Ok(()) => await_or(rx, None).await,
                Err(_) => None,
            };
            choice.and_then(|choice| {
                options
                    .iter()
                    .find(|(_, name, _)| *name == choice)
                    .map(|(id, ..)| id.clone())
            })
        } else {
            let preview = if body.is_empty() {
                title
            } else {
                format!("{title}\n{body}")
            };
            let (tx, rx) = oneshot::channel();
            let sent = events
                .send(TurnEvent::ApprovalRequest {
                    preview,
                    scope: None,
                    respond: tx,
                })
                .await;
            let answer = match sent {
                Ok(()) => await_or(rx, Answer::Deny).await,
                Err(_) => Answer::Deny,
            };
            let wanted: &[&str] = match answer {
                Answer::Allow => &["allow_once"],
                Answer::AlwaysAllow => &["allow_always", "allow_once"],
                Answer::Deny => &["reject_once", "reject_always"],
            };
            wanted.iter().find_map(|kind| {
                options
                    .iter()
                    .find(|(_, _, k)| k == kind)
                    .map(|(id, ..)| id.clone())
            })
        };
        let outcome = match picked {
            Some(option) => json!({ "outcome": { "outcome": "selected", "optionId": option } }),
            None => json!({ "outcome": { "outcome": "cancelled" } }),
        };
        let _ = self.respond(id, outcome).await;
    }

    /// Ask the agent to stop the turn in flight. The prompt call then returns
    /// with a cancelled stop reason, which ends the turn normally.
    pub async fn cancel(&self) {
        let session = self.session.lock().expect("session lock").clone();
        if let Some(session) = session {
            let _ = self
                .write(&json!({ "jsonrpc": "2.0", "method": "session/cancel", "params": { "sessionId": session } }))
                .await;
        }
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        if let Some(handle) = self.reader.lock().expect("reader lock").take() {
            handle.abort();
        }
        // Dropping the child is what kill_on_drop acts on; the reader task no
        // longer holds it, so the process goes with the Agent.
        let _ = self.child.lock().expect("child lock").take();
    }
}

/// The text inside an ACP content block, or a list of them.
fn content_text(value: &Value) -> String {
    match value {
        Value::Array(items) => items.iter().map(content_text).collect::<Vec<_>>().join(""),
        Value::Object(_) => {
            if let Some(text) = value.get("text").and_then(Value::as_str) {
                text.to_string()
            } else if let Some(inner) = value.get("content") {
                content_text(inner)
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

#[cfg(test)]
#[path = "tests/bridge_test.rs"]
mod tests;
