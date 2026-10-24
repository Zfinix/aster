//! Photon 2.0 provider: one bridge for the iMessage, WhatsApp, and Telegram
//! lines Photon hosts. Inbound messages come from polling `listMessages`, which
//! needs no public URL and works behind carrier NAT, so the bridge runs on the
//! phone itself. Each conversation keeps its own `aster acp` agent, the way the
//! Telegram bridge does, and replies go back as plain-text sends.

use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use tokio::sync::mpsc;
use ulid::Ulid;

use crate::bridge::Answer;
use crate::channel::{Skills, discover_skill_commands};

mod api;
mod commands;
mod cursor;
mod inbound;
mod state;
mod text;
mod turn;

use api::{Api, ApiError, RETRY_AFTER_CAP, extension_for};
use commands::handle_command;
use cursor::{Cursor, LastChat, parse_time};
use inbound::{Inbound, Part, is_allowed, normalize_handle, parse_inbound, stranger_reply};
use state::{Bridge, Pending, settings_path};
use text::{parse_approval, resolve_choice};
use turn::{start_turn, watch_wakeups};

/// Photon 2.0's REST origin; `ASTER_PHOTON_API` points the bridge elsewhere.
pub const SPECTRUM_API: &str = "https://api.photon.codes/v1";

/// The platforms answered when none are named: the ones a person chats on.
const SPECTRUM_PLATFORMS: &[&str] = &["imessage", "whatsapp", "telegram"];

/// How often the inbox is read: quickly while a conversation is moving, then
/// easing off to the slow end once it goes quiet.
const POLL_FAST: Duration = Duration::from_secs(2);
const POLL_SLOW: Duration = Duration::from_secs(5);

const PAGE_SIZE: &str = "100";

/// Pages read per poll before handing back to the loop, so a backlog cannot
/// starve the wake-up watcher or hold a turn's replies.
const MAX_PAGES: usize = 20;

/// Staged files outlive their turn so a re-read stays possible, then go after
/// a day.
const STAGED_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const STAGED_SWEEP: Duration = Duration::from_secs(60 * 60);
const STAGED_PREFIX: &str = "spectrum-";

#[derive(Clone)]
pub struct SpectrumConfig {
    /// A project API key (`pho_sk_…`) with `project:read` and `project:write`.
    pub api_key: String,
    pub project_id: String,
    pub api_base: String,
    /// Platforms answered; the rest of the project's traffic is left alone.
    pub platforms: Vec<String>,
    /// Sender handles allowed to drive the agent, optionally scoped to one
    /// platform as `imessage:+15551234567`.
    pub allowed_senders: Vec<String>,
    pub bin: PathBuf,
    pub repo_root: PathBuf,
    pub mode: String,
}

/// One conversation: Photon gives every chat a Space, and a Space id is only
/// meaningful on its platform.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ChatKey {
    pub platform: String,
    pub space: String,
}

impl ChatKey {
    /// The key as a file name and session name: Space ids are already plain,
    /// but anything else is folded so a path can never climb out.
    pub(crate) fn slug(&self) -> String {
        let safe = |text: &str| -> String {
            text.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        c
                    } else {
                        '-'
                    }
                })
                .take(64)
                .collect()
        };
        format!("{}-{}", safe(&self.platform), safe(&self.space))
    }
}

pub async fn run_spectrum(cfg: SpectrumConfig) -> Result<()> {
    ensure!(
        !cfg.api_key.trim().is_empty(),
        "the Photon API key is empty"
    );
    ensure!(
        cfg.project_id.starts_with("pho_prj_")
            && cfg
                .project_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "the Photon project id should look like pho_prj_…"
    );
    let api = Api::new(&cfg.api_base, &cfg.project_id, &cfg.api_key)?;
    let cursor_path = settings_path(&ChatKey {
        platform: "cursor".into(),
        space: cfg.project_id.clone(),
    });
    let saved = cursor_path
        .as_deref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| Cursor::from_json(&value));
    // Check the key against the inbox for the newest messages in it. A refused
    // key is fatal; anything else is the network, which a phone loses and
    // regains, so it is waited out.
    let query = [
        ("direction", "received"),
        ("order", "desc"),
        ("pageSize", PAGE_SIZE),
    ];
    let mut wait = Duration::from_secs(2);
    let newest = loop {
        match api.list_messages(&query).await {
            Ok(page) => break page_ids(&page),
            Err(e) => {
                if let Some(refused) = e.downcast_ref::<ApiError>()
                    && matches!(refused.status, 401 | 403 | 404)
                {
                    return Err(e).context(
                        "connecting to Photon; check the API key, its project:read and project:write permissions, and the project id",
                    );
                }
                tracing::warn!("photon is not reachable yet: {e:#}");
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(RETRY_AFTER_CAP);
            }
        }
    };
    // A first run starts at the newest message already in the inbox, by
    // Photon's clock rather than the phone's, so nothing from before the
    // bridge existed is answered and nothing after it is missed.
    let cursor = saved.unwrap_or_else(|| {
        let mut cursor = Cursor::default();
        for (id, created) in newest {
            cursor.admit(&id, created);
        }
        cursor
    });

    let platforms = if cfg.platforms.is_empty() {
        SPECTRUM_PLATFORMS.join(", ")
    } else {
        cfg.platforms.join(", ")
    };
    eprintln!(
        "aster remote: connected as {} on {platforms}, repo {}, mode {}",
        cfg.project_id,
        cfg.repo_root.display(),
        cfg.mode
    );
    if cfg.allowed_senders.is_empty() {
        eprintln!(
            "aster remote: no senders allowed yet; message the line once and restart with --sender <your handle>"
        );
    }

    let skills: Skills = Arc::new(discover_skill_commands(&cfg.repo_root));
    let mut cfg = cfg;
    if cfg.platforms.is_empty() {
        cfg.platforms = SPECTRUM_PLATFORMS.iter().map(|p| p.to_string()).collect();
    }
    let bridge = Arc::new(Bridge {
        api,
        cfg,
        skills,
        chats: Mutex::new(HashMap::new()),
        cursor: Mutex::new(cursor),
        cursor_path,
        told: Mutex::new(HashSet::new()),
        workers: Mutex::new(HashMap::new()),
    });
    bridge.save_cursor();
    tokio::spawn(watch_wakeups(Arc::clone(&bridge)));
    tokio::spawn(sweep_staged_files());
    poll_loop(bridge).await
}

fn page_ids(page: &Value) -> Vec<(String, i64)> {
    page.get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = item.get("id")?.as_str()?.to_string();
                    let created = parse_time(item.get("createdAt")?.as_str()?)?;
                    Some((id, created))
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn poll_loop(bridge: Arc<Bridge>) -> Result<()> {
    let mut wait = POLL_FAST;
    loop {
        tokio::time::sleep(wait).await;
        wait = match poll_once(&bridge).await {
            Ok(true) => POLL_FAST,
            Ok(false) => (wait + Duration::from_secs(1)).min(POLL_SLOW),
            Err(e) => match e.downcast_ref::<ApiError>() {
                Some(refused) if matches!(refused.status, 401 | 403) => {
                    return Err(e).context("Photon stopped accepting the API key");
                }
                Some(refused) if refused.status == 429 => refused
                    .retry_after
                    .unwrap_or(Duration::from_secs(10))
                    .max(POLL_SLOW),
                _ => {
                    tracing::warn!("listMessages failed: {e:#}");
                    (wait * 2).clamp(POLL_SLOW, RETRY_AFTER_CAP)
                }
            },
        };
    }
}

/// Read everything since the cursor, answer what is new, and say whether
/// there was any.
async fn poll_once(bridge: &Arc<Bridge>) -> Result<bool> {
    let since = bridge.cursor.lock().expect("cursor lock").query_since();
    let platform = match bridge.cfg.platforms.as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    };
    let mut token: Option<String> = None;
    let mut found = false;
    for _ in 0..MAX_PAGES {
        let mut query = vec![
            ("createdSince", since.as_str()),
            ("direction", "received"),
            ("order", "asc"),
            ("pageSize", PAGE_SIZE),
        ];
        if let Some(platform) = &platform {
            query.push(("platform", platform.as_str()));
        }
        if let Some(token) = &token {
            query.push(("pageToken", token.as_str()));
        }
        let page = bridge.api.list_messages(&query).await?;
        let items = page
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for item in &items {
            let (Some(id), Some(created)) = (
                item.get("id").and_then(Value::as_str),
                item.get("createdAt")
                    .and_then(Value::as_str)
                    .and_then(parse_time),
            ) else {
                continue;
            };
            let fresh = bridge
                .cursor
                .lock()
                .expect("cursor lock")
                .admit(id, created);
            if !fresh {
                continue;
            }
            found = true;
            // Saved before acting, so a restart never runs a turn twice.
            bridge.save_cursor();
            if let Some(inbound) = parse_inbound(item, &bridge.cfg.platforms) {
                dispatch(bridge, inbound);
            }
        }
        bridge.save_cursor();
        token = page
            .get("nextPageToken")
            .and_then(Value::as_str)
            .map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    Ok(found)
}

/// Hand a message to its chat's worker, starting one when the chat has none.
/// A chat's messages are handled one at a time and in order, while the poll
/// moves straight on.
fn dispatch(bridge: &Arc<Bridge>, inbound: Inbound) {
    let mut workers = bridge.workers.lock().expect("workers lock");
    let inbound = match workers.get(&inbound.chat) {
        Some(worker) => match worker.send(inbound) {
            Ok(()) => return,
            Err(mpsc::error::SendError(inbound)) => inbound,
        },
        None => inbound,
    };
    let chat = inbound.chat.clone();
    let (worker, mut waiting) = mpsc::unbounded_channel();
    workers.insert(chat.clone(), worker);
    let bridge = Arc::clone(bridge);
    tokio::spawn(async move {
        let mut next = Some(inbound);
        while let Some(inbound) = next {
            handle_inbound(&bridge, inbound).await;
            // Checked under the lock `dispatch` sends under, so a message
            // either lands here or finds no worker and starts a new one.
            let mut workers = bridge.workers.lock().expect("workers lock");
            next = waiting.try_recv().ok();
            if next.is_none() {
                workers.remove(&chat);
            }
        }
    });
}

async fn handle_inbound(bridge: &Arc<Bridge>, inbound: Inbound) {
    let Inbound {
        id,
        chat,
        agent,
        sender,
        parts,
    } = inbound;
    if !is_allowed(&bridge.cfg.allowed_senders, &chat.platform, &sender) {
        let first = bridge
            .told
            .lock()
            .expect("told lock")
            .insert(normalize_handle(&sender));
        match stranger_reply(&bridge.cfg.allowed_senders, &sender) {
            Some(text) if first => {
                bridge.chat(&chat, |state| state.agent_user = Some(agent.clone()));
                bridge
                    .outbox(&chat, Some(&id), "stranger")
                    .text(&text)
                    .await;
            }
            Some(_) => {}
            None => tracing::warn!(
                "ignored a message from a sender outside the allowlist on {}",
                chat.platform
            ),
        }
        return;
    }
    bridge.chat(&chat, |state| {
        state.touched = Some(Instant::now());
        state.agent_user = Some(agent.clone());
        state.last_seen = Some(id.clone());
    });
    bridge.cursor.lock().expect("cursor lock").last = Some(LastChat {
        chat: chat.clone(),
        agent,
    });

    let mut texts = Vec::new();
    let mut files = Vec::new();
    for part in parts {
        match part {
            Part::Text(text) => texts.push(text),
            Part::Attachment(attachment) => match stage_attachment(bridge, &attachment).await {
                Ok(path) => files.push(format!("@{path}")),
                Err(e) => {
                    tracing::warn!("attachment {attachment} not staged: {e:#}");
                    bridge
                        .outbox(&chat, Some(&id), "intake")
                        .text("That file didn't reach me. Send it again?")
                        .await;
                    return;
                }
            },
        }
    }
    let text = texts.join("\n");
    let trimmed = text.trim();

    if files.is_empty()
        && let Some(command) = trimmed.strip_prefix('/')
    {
        let (name, arg) = match command.split_once(char::is_whitespace) {
            Some((name, arg)) => (name, arg.trim()),
            None => (command, ""),
        };
        handle_command(bridge, &chat, &id, &name.to_lowercase(), arg).await;
        return;
    }

    // A pending approval or question claims the next plain message. A reply
    // that is not an answer at all still settles an approval, as a no, and
    // then goes on to the agent as what the person said instead.
    let pending = bridge.chat(&chat, |state| state.pending.take());
    match pending {
        Some(Pending::Approval(respond)) if files.is_empty() => {
            let answer = parse_approval(trimmed);
            let _ = respond.send(answer.unwrap_or(Answer::Deny));
            if answer.is_some() {
                return;
            }
        }
        Some(Pending::Question { options, respond }) if files.is_empty() => {
            let _ = respond.send(resolve_choice(trimmed, &options));
            return;
        }
        Some(Pending::Approval(respond)) => {
            let _ = respond.send(Answer::Deny);
        }
        Some(Pending::Question { respond, .. }) => {
            let _ = respond.send(None);
        }
        None => {}
    }

    let prompt = files
        .into_iter()
        .chain((!trimmed.is_empty()).then(|| trimmed.to_string()))
        .collect::<Vec<_>>()
        .join(" ");
    start_turn(bridge, &chat, Some(id), prompt);
}

/// Download an attachment to the temp dir, so the prompt can mention it and
/// the agent's `@path` attachment picks it up.
async fn stage_attachment(bridge: &Bridge, attachment: &str) -> Result<String> {
    let (bytes, content_type) = bridge.api.download(attachment).await?;
    let path = env::temp_dir().join("aster-pasted").join(format!(
        "{STAGED_PREFIX}{}.{}",
        Ulid::new(),
        extension_for(&content_type)
    ));
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&path, bytes).await?;
    Ok(path.display().to_string())
}

/// Deletes staged files older than a day, so one that was never picked up by
/// a turn does not sit in temp storage forever.
async fn sweep_staged_files() {
    let dir = env::temp_dir().join("aster-pasted");
    loop {
        tokio::time::sleep(STAGED_SWEEP).await;
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let cutoff = SystemTime::now() - STAGED_TTL;
        for entry in entries.flatten() {
            let path = entry.path();
            let is_ours = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(STAGED_PREFIX));
            let expired = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .map(|modified| modified < cutoff)
                .unwrap_or(false);
            if is_ours && expired {
                let _ = tokio::fs::remove_file(&path).await;
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/spectrum_test.rs"]
mod tests;
