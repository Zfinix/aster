//! Native iMessage provider. Free and local: polls Messages.app's chat.db
//! (SQLite) for inbound messages and replies via AppleScript. Requires Full
//! Disk Access for the terminal so chat.db is readable. macOS only.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::bridge::{Answer, Turn, TurnEvent, WireMessage};
use crate::channel::{
    Chats, MODES, Pending, Skills, chat_state, console_text, console_tool, discover_skill_commands,
    get_override, set_override, skill_prompt, truncate,
};

const IMESSAGE_SYSTEM: &str = "You are Aster, running remotely over iMessage. \
 Replies are sent as plain iMessage texts, so keep them short and readable; \
 no markdown tables or long code blocks. Approvals arrive as plain questions; \
 the user answers yes/no/always in text.";

const MAX_QUEUED: usize = 10;
const CHUNK_LIMIT: usize = 3500;
const POLL_SECS: u64 = 3;

#[derive(Clone)]
pub struct IMessageConfig {
    /// iMessage handles (email or phone) allowed to drive the agent.
    pub allowed_senders: Vec<String>,
    pub db_path: PathBuf,
    pub bin: PathBuf,
    pub repo_root: PathBuf,
    pub mode: String,
}

/// Outbound side: AppleScript into Messages.app.
#[derive(Clone)]
struct IMessageApi;

impl IMessageApi {
    async fn send_text(&self, handle: &str, text: &str) {
        let esc = text.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            "tell application \"Messages\" to send \"{esc}\" to buddy \"{handle}\" of (1st service whose service type is iMessage)"
        );
        let out = tokio::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .await;
        match out {
            Ok(o) if o.status.success() => {}
            Ok(o) => tracing::warn!(
                "imessage send failed: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            ),
            Err(e) => tracing::warn!("imessage send failed: {e}"),
        }
    }
}

pub async fn run_imessage(cfg: IMessageConfig) -> Result<()> {
    let skills: Skills = Arc::new(discover_skill_commands(&cfg.repo_root));
    let chats: Chats = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    let api = Arc::new(IMessageApi);
    eprintln!(
        "aster remote: imessage polling {}, repo {}, mode {}",
        cfg.db_path.display(),
        cfg.repo_root.display(),
        cfg.mode
    );
    if cfg.allowed_senders.is_empty() {
        eprintln!(
            "aster remote: no senders allowed yet; message the chat once and restart with --sender <handle> to allow it"
        );
    }

    let (events_tx, mut events_rx) = mpsc::unbounded_channel::<(String, String)>();
    let poller_cfg = Arc::new(cfg.clone());
    let poller = tokio::spawn(async move { poll_loop(poller_cfg, events_tx).await });

    while let Some((sender, text)) = events_rx.recv().await {
        handle_message(&api, &cfg, &chats, &skills, &sender, &text).await;
    }
    poller.await?
}

/// Poll Messages.app's chat.db for new inbound messages. Requires Full Disk
/// Access for the terminal running aster, else sqlite3 gets permission denied.
async fn poll_loop(
    cfg: Arc<IMessageConfig>,
    events: mpsc::UnboundedSender<(String, String)>,
) -> Result<()> {
    let mut last_rowid = current_max_rowid(&cfg.db_path).await?;
    loop {
        tokio::time::sleep(Duration::from_secs(POLL_SECS)).await;
        let rows = match fetch_new(&cfg.db_path, last_rowid).await {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("imessage poll failed: {e:#}");
                continue;
            }
        };
        for (rowid, sender, text) in rows {
            last_rowid = last_rowid.max(rowid);
            if text.trim().is_empty() {
                continue;
            }
            if cfg.allowed_senders.is_empty() {
                eprintln!(
                    "aster remote: message from {sender} ignored; restart with --sender {sender} to allow it"
                );
                continue;
            }
            if !cfg.allowed_senders.iter().any(|a| a == &sender) {
                continue;
            }
            let _ = events.send((sender, text));
        }
    }
}

async fn sqlite_json(db: &std::path::Path, sql: &str) -> Result<Value> {
    let out = tokio::process::Command::new("sqlite3")
        .arg("-readonly")
        .arg("-json")
        .arg(db)
        .arg(sql)
        .output()
        .await
        .context("running sqlite3 against chat.db (is Full Disk Access granted?)")?;
    if !out.status.success() {
        anyhow::bail!(
            "sqlite3 failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    if stdout.trim().is_empty() {
        return Ok(Value::Array(vec![]));
    }
    Ok(serde_json::from_str(stdout.trim())?)
}

async fn current_max_rowid(db: &std::path::Path) -> Result<i64> {
    let v = sqlite_json(db, "SELECT COALESCE(MAX(ROWID),0) AS maxid FROM message").await?;
    Ok(v[0]["maxid"].as_i64().unwrap_or(0))
}

async fn fetch_new(db: &std::path::Path, last_rowid: i64) -> Result<Vec<(i64, String, String)>> {
    let sql = format!(
        "SELECT m.ROWID AS rowid, COALESCE(h.id,'') AS sender, m.text AS text \
         FROM message m LEFT JOIN handle h ON m.handle_id = h.ROWID \
         WHERE m.ROWID > {last_rowid} AND m.is_from_me = 0 \
           AND m.text IS NOT NULL AND m.text != '' \
         ORDER BY m.ROWID"
    );
    let v = sqlite_json(db, &sql).await?;
    Ok(v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    Some((
                        r["rowid"].as_i64()?,
                        r["sender"].as_str()?.to_string(),
                        r["text"].as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default())
}

async fn handle_message(
    api: &Arc<IMessageApi>,
    cfg: &IMessageConfig,
    chats: &Chats,
    skills: &Skills,
    sender: &str,
    text: &str,
) {
    let chat_id = chat_key(sender);
    let trimmed = text.trim();
    if let Some(command) = trimmed.strip_prefix('/') {
        let (name, arg) = match command.split_once(char::is_whitespace) {
            Some((name, arg)) => (name, arg.trim()),
            None => (command, ""),
        };
        handle_command(api, cfg, chats, skills, &chat_id, name, arg).await;
        return;
    }
    let pending = chat_state(chats, "imessage", 0, |state| state.pending.take());
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
    format!("imessage-{}", truncate(sender, 64))
}

async fn handle_command(
    api: &Arc<IMessageApi>,
    cfg: &IMessageConfig,
    chats: &Chats,
    skills: &Skills,
    chat_id: &str,
    name: &str,
    arg: &str,
) {
    match name {
        "start" | "help" => api.send_text(chat_id, &help(cfg)).await,
        "new" | "clear" => {
            chat_state(chats, "imessage", 0, |state| {
                state.history.clear();
                state.queued.clear();
            });
            api.send_text(chat_id, "Started a fresh conversation.")
                .await;
        }
        "stop" => {
            let stopped = chat_state(chats, "imessage", 0, |state| {
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
        "mode" => match set_override(chats, "imessage", 0, arg, MODES, |state| &mut state.mode) {
            Some(reply) => api.send_text(chat_id, &reply).await,
            None => {
                let current = get_override(chats, "imessage", 0, |state| state.mode.clone())
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
                get_override(chats, "imessage", 0, |state| state.mode.clone())
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

fn help(cfg: &IMessageConfig) -> String {
    format!(
        "Aster remote control over iMessage (native, free).\n\
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
    api: Arc<IMessageApi>,
    cfg: IMessageConfig,
    chats: Chats,
    chat_id: &str,
    prompt: &str,
) {
    let prepared = chat_state(&chats, "imessage", 0, |state| {
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
        chat_state(&chats, "imessage", 0, |state| {
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
        content: IMESSAGE_SYSTEM.into(),
    });
    wire.extend(history);

    let (events_tx, mut events_rx) = tokio::sync::mpsc::channel::<TurnEvent>(8);
    let turn_task =
        tokio::spawn(async move { crate::bridge::run_turn(&turn, &wire, &events_tx).await });
    chat_state(&chats, "imessage", 0, |state| {
        state.running = Some(turn_task.abort_handle())
    });
    eprintln!("[{chat_id}] user: {}", console_text(prompt, 200));

    let chats = chats.clone();
    let chat_id = chat_id.to_string();
    tokio::spawn(async move {
        while let Some(event) = events_rx.recv().await {
            match event {
                TurnEvent::ToolCall {
                    name, arguments, ..
                } => {
                    eprintln!("[{chat_id}] -> {}", console_tool(&name, &arguments));
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
                    chat_state(&chats, "imessage", 0, |state| {
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
                    chat_state(&chats, "imessage", 0, |state| {
                        state.pending = Some(Pending::Question(respond))
                    });
                }
            }
        }
        let result = turn_task
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("turn cancelled: {e}")));
        chat_state(&chats, "imessage", 0, |state| {
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
            if !current.is_empty() && current.len() + 1 + line.len() > limit {
                chunks.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
        if !current.is_empty() {
            chunks.push(current);
        }
        chunks
    }
}

#[cfg(test)]
#[path = "tests/imessage_test.rs"]
mod tests;
