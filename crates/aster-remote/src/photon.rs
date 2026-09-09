//! Photon iMessage provider. Photon's macOS agent server watches iMessage and
//! POSTs signed webhooks to us; replies go back through its REST send API.
//! No public URL is needed beyond what the user configures in Photon.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::bridge::{Answer, Turn, TurnEvent, WireMessage};
use crate::channel::{
    Chats, MODES, Pending, Skills, chat_state, console_text, console_tool, discover_skill_commands,
    get_override, set_override, skill_prompt, truncate,
};

const PHOTON_SYSTEM: &str = "You are Aster, running remotely over iMessage via Photon. \
 Replies are sent as plain iMessage texts, so keep them short and readable; \
 no markdown tables or long code blocks. Approvals arrive as plain questions; \
 the user answers yes/no/always in text.";

const MAX_QUEUED: usize = 10;
const CHUNK_LIMIT: usize = 3500;
const HTTP_HEADER_LIMIT: usize = 64 * 1024;

#[derive(Clone)]
pub struct PhotonConfig {
    pub secret: String,
    pub port: u16,
    pub allowed_senders: Vec<String>,
    pub bin: std::path::PathBuf,
    pub repo_root: std::path::PathBuf,
    pub mode: String,
}

/// Outbound side: Photon's send API. The endpoint and payload follow the
/// dashboard's getting-started curl example; adjust there first if Photon
/// changes its wire format.
struct PhotonApi {
    http: reqwest::Client,
    base: String,
}

impl PhotonApi {
    fn new(secret: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: format!("http://localhost:8787/api/v1/{secret}"),
        }
    }

    async fn send_text(&self, chat_id: &str, text: &str) {
        if let Err(e) = self
            .http
            .post(format!("{base}/messages", base = self.base))
            .json(&json!({ "chat_identifier": chat_id, "message": text }))
            .send()
            .await
        {
            tracing::warn!("photon send failed: {e:#}");
        }
    }
}

pub async fn run_photon(cfg: PhotonConfig) -> Result<()> {
    let skills: Skills = Arc::new(discover_skill_commands(&cfg.repo_root));
    let chats: Chats = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    let api = Arc::new(PhotonApi::new(&cfg.secret));
    eprintln!(
        "aster remote: photon iMessage listening on :{}, repo {}, mode {}",
        cfg.port,
        cfg.repo_root.display(),
        cfg.mode
    );
    if cfg.allowed_senders.is_empty() {
        eprintln!(
            "aster remote: no senders allowed yet; message the chat once and restart with --sender <handle> to allow it"
        );
    }

    let (events_tx, mut events_rx) = mpsc::unbounded_channel::<(String, String)>();
    let server_cfg = Arc::new(cfg.clone());
    let webhook_cfg = Arc::clone(&server_cfg);
    let server = tokio::spawn(async move { webhook_server(webhook_cfg, events_tx).await });

    while let Some((sender, text)) = events_rx.recv().await {
        handle_message(&api, &server_cfg, &chats, &skills, &sender, &text).await;
    }
    server.await?
}

/// Minimal HTTP server for Photon webhooks. One connection at a time is fine:
/// Photon retries delivery, and turns run in their own tasks.
async fn webhook_server(
    cfg: Arc<PhotonConfig>,
    events: mpsc::UnboundedSender<(String, String)>,
) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], cfg.port)))
        .await
        .with_context(|| format!("binding 127.0.0.1:{}", cfg.port))?;
    loop {
        let (mut stream, _) = listener.accept().await?;
        let cfg = cfg.clone();
        let events = events.clone();
        tokio::spawn(async move {
            let _ = handle_connection(&mut stream, &cfg, events).await;
        });
    }
}

async fn handle_connection(
    stream: &mut tokio::net::TcpStream,
    cfg: &PhotonConfig,
    events: mpsc::UnboundedSender<(String, String)>,
) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > HTTP_HEADER_LIMIT {
            return Ok(());
        }
        if buf.windows(4).rev().any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let head_end = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(buf.len());
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default().to_string();
    let mut method = String::new();
    if let Some((m, _)) = request_line.split_once(' ') {
        method = m.to_string();
    }
    let mut signature = String::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("x-photon-signature")
        {
            signature = value.trim().to_string();
        }
    }

    let content_length = head
        .to_lowercase()
        .lines()
        .find_map(|l| {
            l.strip_prefix("content-length:")?
                .trim()
                .parse::<usize>()
                .ok()
        })
        .unwrap_or(0);
    let mut body = buf.split_off(head_end + 4);
    while body.len() < content_length {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }

    if method == "GET" {
        let response = b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok";
        stream.write_all(response).await?;
        return Ok(());
    }

    if !verify_signature(cfg, &signature, &body) {
        let response = b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\n\r\n";
        stream.write_all(response).await?;
        tracing::warn!("photon webhook rejected: bad signature");
        return Ok(());
    }

    let ok = parse_event(&body)
        .map(|(sender, text)| events.send((sender, text)).is_ok())
        .unwrap_or(false);
    let response: &[u8] = if ok {
        b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok"
    } else {
        b"HTTP/1.1 400 Bad Request\r\ncontent-length: 0\r\n\r\n"
    };
    stream.write_all(response).await?;
    Ok(())
}

/// Photon signs deliveries with HMAC-SHA256 of the raw body, hex-encoded in
/// the x-photon-signature header.
fn verify_signature(cfg: &PhotonConfig, signature: &str, body: &[u8]) -> bool {
    if cfg.secret.is_empty() {
        return true;
    }
    let provided = signature
        .trim()
        .trim_start_matches("sha256=")
        .trim_start_matches("v1=");
    let mut mac =
        Hmac::<Sha256>::new_from_slice(cfg.secret.as_bytes()).expect("hmac accepts any key length");
    mac.update(body);
    let expected = hex::encode(mac.finalize().into_bytes());
    const_time_eq(provided, &expected)
}

fn const_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// Pull (sender, text) out of a Photon webhook body. Photon's event shape is
/// `{ event: "message.received", message: { ... } }`; aliases keep us honest
/// across minor Photon versions.
fn parse_event(body: &[u8]) -> Option<(String, String)> {
    let event: Value = serde_json::from_slice(body).ok()?;
    let message = event.get("message").unwrap_or(&event);
    let sender = ["sender", "from", "chat_identifier", "chat_id"]
        .iter()
        .find_map(|k| message.get(*k))
        .or_else(|| event.get("sender"))
        .and_then(Value::as_str)?
        .to_string();
    let text = ["text", "body", "message", "content"]
        .iter()
        .find_map(|k| message.get(*k))
        .and_then(Value::as_str)?
        .to_string();
    (!text.is_empty()).then_some((sender, text))
}

async fn handle_message(
    api: &Arc<PhotonApi>,
    cfg: &Arc<PhotonConfig>,
    chats: &Chats,
    skills: &Skills,
    sender: &str,
    text: &str,
) {
    let chat_id = chat_key(sender);
    if !cfg.allowed_senders.is_empty() && !cfg.allowed_senders.iter().any(|a| a == sender) {
        api.send_text(
            &chat_id,
            &format!("Not authorized. Restart the bridge with --sender {sender} to allow it."),
        )
        .await;
        return;
    }
    let trimmed = text.trim();
    if let Some(command) = trimmed.strip_prefix('/') {
        let (name, arg) = match command.split_once(char::is_whitespace) {
            Some((name, arg)) => (name, arg.trim()),
            None => (command, ""),
        };
        handle_command(api, cfg, chats, skills, &chat_id, name, arg).await;
        return;
    }
    // A pending approval or question claims the next plain message.
    let pending = chat_state(chats, "photon", 0, |state| state.pending.take());
    match pending {
        Some(Pending::Approval { respond, .. }) => {
            let answer = match trimmed.to_lowercase().as_str() {
                "y" | "yes" | "allow" => Some(Answer::Allow),
                "always" => Some(Answer::AlwaysAllow),
                _ => Some(Answer::Deny),
            };
            let _ = respond.send(answer.unwrap_or(Answer::Deny));
            return;
        }
        Some(Pending::Question(respond)) => {
            let answer = (trimmed != "skip").then(|| trimmed.to_string());
            let _ = respond.send(answer);
            return;
        }
        None => {}
    }
    start_turn(api.clone(), cfg.clone(), chats.clone(), &chat_id, trimmed).await;
}

fn chat_key(sender: &str) -> String {
    format!("photon-{}", truncate(sender, 64))
}

async fn handle_command(
    api: &Arc<PhotonApi>,
    cfg: &Arc<PhotonConfig>,
    chats: &Chats,
    skills: &Skills,
    chat_id: &str,
    name: &str,
    arg: &str,
) {
    let numeric = chat_id.trim_start_matches("photon-").to_string();
    let _ = &numeric;
    match name {
        "start" | "help" => api.send_text(chat_id, &help(cfg)).await,
        "new" | "clear" => {
            chat_state(chats, "photon", 0, |state| {
                state.history.clear();
                state.queued.clear();
            });
            api.send_text(chat_id, "Started a fresh conversation.")
                .await;
        }
        "stop" => {
            let stopped = chat_state(chats, "photon", 0, |state| {
                state.queued.clear();
                state.running.take().is_some()
            });
            api.send_text(
                chat_id,
                if stopped {
                    "Stopped the current turn."
                } else {
                    "Nothing is running."
                },
            )
            .await;
        }
        "mode" => match set_override(chats, "photon", 0, arg, MODES, |state| &mut state.mode) {
            Some(reply) => api.send_text(chat_id, &reply).await,
            None => {
                let current = get_override(chats, "photon", 0, |state| state.mode.clone())
                    .unwrap_or_else(|| cfg.mode.clone());
                api.send_text(
                    chat_id,
                    &format!(
                        "Mode is {current}; reply `/mode <one of {}>`.",
                        MODES.join(", ")
                    ),
                )
                .await;
            }
        },
        "status" => {
            let text = format!(
                "Repo: {}\nMode: {}\nState: idle",
                cfg.repo_root.display(),
                get_override(chats, "photon", 0, |state| state.mode.clone())
                    .unwrap_or_else(|| cfg.mode.clone()),
            );
            api.send_text(chat_id, &text).await;
        }
        "skills" => {
            let mut names: Vec<String> = skills.keys().cloned().collect();
            names.sort();
            api.send_text(chat_id, &format!("Installed: {}", names.join(", ")))
                .await;
        }
        other => {
            if let Some(skill) = skills.get(other) {
                let prompt = skill_prompt(skill, arg);
                start_turn(api.clone(), cfg.clone(), chats.clone(), chat_id, &prompt).await;
            } else {
                api.send_text(chat_id, "Unknown command; /help lists what I know.")
                    .await;
            }
        }
    }
}

fn help(cfg: &PhotonConfig) -> String {
    format!(
        "Aster remote control over iMessage.\n\
         Send a message to run the agent on {}.\n\
         Approvals arrive as questions; answer yes/no/always.\n\n\
         /new — fresh conversation\n\
         /stop — cancel the running turn\n\
         /mode — how the agent acts (plan, manual, auto, edit, yolo)\n\
         /status — mode and repo\n\
         /help — this message",
        cfg.repo_root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| cfg.repo_root.display().to_string())
    )
}

async fn start_turn(
    api: Arc<PhotonApi>,
    cfg: Arc<PhotonConfig>,
    chats: Chats,
    chat_id: &str,
    prompt: &str,
) {
    let prepared = chat_state(&chats, "photon", 0, |state| {
        if state.running.is_some() {
            None
        } else {
            state.history.push(WireMessage::user(prompt));
            Some((
                state.history.clone(),
                state.mode.clone(),
                state.model.clone(),
                state.effort.clone(),
            ))
        }
    });
    let Some((history, mode, model, effort)) = prepared else {
        api.send_text(
            chat_id,
            "Working on the previous message; this one is queued.",
        )
        .await;
        chat_state(&chats, "photon", 0, |state| {
            if state.queued.len() < MAX_QUEUED {
                state.queued.push_back((0, prompt.to_string()));
            }
        });
        return;
    };

    let turn = Turn {
        bin: cfg.bin.clone(),
        repo_root: cfg.repo_root.clone(),
        session: chat_id.to_string(),
        mode: mode.unwrap_or_else(|| cfg.mode.clone()),
        model,
        effort,
        extra_env: vec![],
    };
    let mut wire = Vec::with_capacity(history.len() + 1);
    wire.push(WireMessage {
        role: "system".into(),
        content: PHOTON_SYSTEM.into(),
    });
    wire.extend(history);

    let (events_tx, mut events_rx) = tokio::sync::mpsc::channel::<TurnEvent>(8);
    let turn_task =
        tokio::spawn(async move { crate::bridge::run_turn(&turn, &wire, &events_tx).await });
    chat_state(&chats, "photon", 0, |state| {
        state.running = Some(turn_task.abort_handle())
    });
    eprintln!("[{chat_id}] user: {}", console_text(prompt, 200));

    let api = api.clone();
    let chats = chats.clone();
    let chat_id = chat_id.to_string();
    tokio::spawn(async move {
        let mut activity: Vec<String> = Vec::new();
        while let Some(event) = events_rx.recv().await {
            match event {
                TurnEvent::ToolCall {
                    name, arguments, ..
                } => {
                    activity.push(console_tool(&name, &arguments));
                    eprintln!("[{chat_id}] -> {name}");
                }
                TurnEvent::ToolResult { .. } => {}
                TurnEvent::ApprovalRequest {
                    preview, respond, ..
                } => {
                    api.send_text(
                        &chat_id,
                        &format!(
                            "Approval needed: {}\nReply yes, always, or no.",
                            truncate(&preview, 800)
                        ),
                    )
                    .await;
                    chat_state(&chats, "photon", 0, |state| {
                        state.pending = Some(Pending::Approval { respond })
                    });
                }
                TurnEvent::Question {
                    header,
                    question,
                    respond,
                    ..
                } => {
                    api.send_text(&chat_id, &format!("{header}\n{question}"))
                        .await;
                    chat_state(&chats, "photon", 0, |state| {
                        state.pending = Some(Pending::Question(respond))
                    });
                }
            }
        }
        let result = turn_task
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("turn cancelled: {e}")));
        chat_state(&chats, "photon", 0, |state| {
            state.running = None;
            state.pending = None;
            if let Ok(outcome) = &result {
                state
                    .history
                    .push(WireMessage::assistant(outcome.reply.clone()));
            }
        });
        match result {
            Ok(outcome) => {
                for chunk in markdown::to_plain_chunks(&outcome.reply, CHUNK_LIMIT) {
                    api.send_text(&chat_id, &chunk).await;
                }
            }
            Err(e) => {
                api.send_text(&chat_id, &format!("Turn failed: {e:#}"))
                    .await
            }
        }
    });
}

/// Split a reply into iMessage-sized plain-text chunks.
mod markdown {
    pub fn to_plain_chunks(text: &str, limit: usize) -> Vec<String> {
        let mut chunks = Vec::new();
        let mut current = String::new();
        for line in text.lines() {
            if current.len() + line.len() + 1 > limit && !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            current.push_str(line);
            current.push('\n');
        }
        if !current.trim().is_empty() {
            chunks.push(current);
        }
        chunks
    }
}

#[cfg(test)]
#[path = "tests/photon_test.rs"]
mod tests;
