//! Per-chat state and the bridge every task shares: the turn in flight, what
//! is waiting on the person, saved settings, and the outbox replies go through.

use std::collections::{HashMap, HashSet};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;
use ulid::Ulid;

use super::api::Api;
use super::cursor::{Cursor, LastChat};
use super::inbound::Inbound;
use super::text::plain_chunks;
use super::{ChatKey, SpectrumConfig};
use crate::bridge::{Agent, Answer};
use crate::channel::{Skills, verbatim_chunks};
use crate::telegram::is_stop_request;

pub(super) const CHUNK_LIMIT: usize = 3500;

pub(super) enum Pending {
    Approval(oneshot::Sender<Answer>),
    Question {
        options: Vec<String>,
        respond: oneshot::Sender<Option<String>>,
    },
}

#[derive(Default)]
pub(super) struct ChatState {
    /// When this chat last spoke, so a wake-up knows where to land.
    pub(super) touched: Option<Instant>,
    /// The agent's user in this Space; every send names it as the sender.
    pub(super) agent_user: Option<String>,
    /// The newest message seen here, so a reply only quotes when the chat has
    /// moved past what it answers.
    pub(super) last_seen: Option<String>,
    pub(super) pending: Option<Pending>,
    pub(super) running: Option<AbortHandle>,
    pub(super) agent: Option<Arc<Agent>>,
    pub(super) session: Option<String>,
    pub(super) stopped: bool,
    /// Messages that arrived too late to join the running turn; they run as
    /// one turn as soon as it ends.
    pub(super) queued: Vec<(Option<String>, String)>,
    pub(super) mode: Option<String>,
    pub(super) photos: Option<String>,
    loaded: bool,
}

/// What a new message does in a chat, given the turn in flight.
pub(super) enum Admission {
    Start,
    /// The turn was told to stop and is still winding down, so the message
    /// waits and runs after it.
    Queued,
    Busy,
}

impl ChatState {
    pub(super) fn admit(&mut self, reply_to: Option<&str>, prompt: &str) -> Admission {
        if self.running.is_none() {
            self.stopped = false;
            return Admission::Start;
        }
        if self.stopped && !is_stop_request(prompt) {
            self.queued
                .push((reply_to.map(str::to_string), prompt.to_string()));
            return Admission::Queued;
        }
        Admission::Busy
    }

    /// Abort the running turn and drop what was waiting behind it, saying how
    /// many messages that was; None when nothing runs. The turn stays
    /// `running` until its finish has run, so a message sent meanwhile cannot
    /// start a second turn that the old finish would then clobber.
    pub(super) fn stop(&mut self) -> Option<usize> {
        let running = self.running.as_ref()?;
        // Set before aborting, so the driver reads it as a deliberate stop
        // rather than reporting the abort as a failure.
        self.stopped = true;
        running.abort();
        Some(std::mem::take(&mut self.queued).len())
    }
}

/// Everything the bridge's tasks share.
pub(super) struct Bridge {
    pub(super) api: Api,
    pub(super) cfg: SpectrumConfig,
    pub(super) skills: Skills,
    pub(super) chats: Mutex<HashMap<ChatKey, ChatState>>,
    pub(super) cursor: Mutex<Cursor>,
    pub(super) cursor_path: Option<PathBuf>,
    /// Handles already told how to get in, so a stranger hears it once per
    /// run rather than once per message.
    pub(super) told: Mutex<HashSet<String>>,
    /// Each chat's messages waiting on the one being handled there, so a slow
    /// download in one chat never holds up another.
    pub(super) workers: Mutex<HashMap<ChatKey, mpsc::UnboundedSender<Inbound>>>,
}

impl Bridge {
    pub(super) fn chat<T>(&self, key: &ChatKey, act: impl FnOnce(&mut ChatState) -> T) -> T {
        let mut chats = self.chats.lock().expect("chats lock");
        let state = chats.entry(key.clone()).or_default();
        if !state.loaded {
            let saved: Value = settings_path(key)
                .and_then(|path| std::fs::read_to_string(path).ok())
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or(Value::Null);
            let field = |name: &str| saved.get(name).and_then(Value::as_str).map(str::to_string);
            state.mode = field("mode");
            state.session = field("session");
            state.photos = field("photos");
            state.loaded = true;
        }
        act(state)
    }

    pub(super) fn save_chat(&self, key: &ChatKey) {
        let saved = self.chat(
            key,
            |state| json!({ "mode": state.mode, "session": state.session, "photos": state.photos }),
        );
        if let Some(path) = settings_path(key) {
            write_json(&path, &saved);
        }
    }

    pub(super) fn save_cursor(&self) {
        let Some(path) = &self.cursor_path else {
            return;
        };
        let saved = self.cursor.lock().expect("cursor lock").to_json();
        write_json(path, &saved);
    }

    /// The chat that spoke most recently, or the one saved from before a
    /// restart, with the agent user to send as.
    pub(super) fn latest_chat(&self) -> Option<LastChat> {
        let live = {
            let chats = self.chats.lock().expect("chats lock");
            chats
                .iter()
                .filter_map(|(key, state)| {
                    Some((state.touched?, key.clone(), state.agent_user.clone()?))
                })
                .max_by_key(|(touched, ..)| *touched)
                .map(|(_, chat, agent)| LastChat { chat, agent })
        };
        live.or_else(|| self.cursor.lock().expect("cursor lock").last.clone())
    }

    pub(super) fn outbox(
        self: &Arc<Self>,
        chat: &ChatKey,
        reply_to: Option<&str>,
        label: &str,
    ) -> Outbox {
        let agent = self.chat(chat, |state| state.agent_user.clone());
        let base = match reply_to {
            Some(id) => format!("aster:{id}:{label}"),
            None => format!("aster:{}:{label}", Ulid::new()),
        };
        Outbox {
            bridge: Arc::clone(self),
            chat: chat.clone(),
            agent,
            base,
            seq: AtomicU32::new(0),
        }
    }
}

pub(super) fn settings_path(key: &ChatKey) -> Option<PathBuf> {
    let home = env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".aster/remote")
            .join(format!("spectrum-{}.json", key.slug())),
    )
}

/// Write through a temporary file and rename, so a crash mid-write leaves the
/// old state rather than half a file.
fn write_json(path: &Path, value: &Value) {
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::warn!("could not create {}: {e}", parent.display());
        return;
    }
    let partial = path.with_extension("json.tmp");
    let written =
        std::fs::write(&partial, value.to_string()).and_then(|()| std::fs::rename(&partial, path));
    if let Err(e) = written {
        tracing::warn!("could not save {}: {e}", path.display());
    }
}

/// Sends for one purpose in one chat. Each send's idempotency key is the
/// message it answers, the purpose, and its place in the sequence, so a retry,
/// or the same message handled again after a crash, cannot post twice.
pub(super) struct Outbox {
    bridge: Arc<Bridge>,
    chat: ChatKey,
    pub(super) agent: Option<String>,
    base: String,
    seq: AtomicU32,
}

impl Outbox {
    fn next_key(&self) -> String {
        format!("{}:{}", self.base, self.seq.fetch_add(1, Ordering::Relaxed))
    }

    pub(super) async fn text(&self, text: &str) {
        self.reply(text, None).await;
    }

    /// The text in as many messages as it takes; only the first quotes.
    pub(super) async fn reply(&self, text: &str, parent: Option<&str>) {
        self.send_chunks(plain_chunks(text, CHUNK_LIMIT), parent)
            .await;
    }

    /// The text with no markdown cleanup, and whether all of it went out.
    pub(super) async fn verbatim(&self, text: &str) -> bool {
        self.send_chunks(verbatim_chunks(text, CHUNK_LIMIT), None)
            .await
    }

    async fn send_chunks(&self, chunks: Vec<String>, parent: Option<&str>) -> bool {
        let Some(agent) = &self.agent else {
            tracing::warn!(
                "no agent user known for {}; reply dropped",
                self.chat.slug()
            );
            return false;
        };
        for (n, chunk) in chunks.into_iter().enumerate() {
            let content = json!({ "type": "text", "format": "plain", "text": chunk });
            let quoted = parent.filter(|_| n == 0);
            if let Err(e) = self
                .bridge
                .api
                .send(&self.next_key(), &self.chat.space, agent, quoted, content)
                .await
            {
                tracing::warn!("send to {} failed: {e:#}", self.chat.slug());
                eprintln!("[{}] error: a reply did not send: {e:#}", self.chat.slug());
                return false;
            }
        }
        true
    }

    /// A screenshot as an attachment message. Only images under the temp
    /// directory or the workspace go out, so a path in a tool's output can
    /// never turn into a way to send some other file off the phone.
    pub(super) async fn photo(&self, path: &str) {
        let Some(agent) = &self.agent else {
            return;
        };
        let path = Path::new(path);
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase);
        let content_type = match extension.as_deref() {
            Some("png") => "image/png",
            Some("jpg" | "jpeg") => "image/jpeg",
            Some("webp") => "image/webp",
            _ => return,
        };
        let sendable = path.canonicalize().is_ok_and(|path| {
            [env::temp_dir(), self.bridge.cfg.repo_root.clone()]
                .iter()
                .filter_map(|root| root.canonicalize().ok())
                .any(|root| path.starts_with(root))
        });
        if !sendable {
            tracing::warn!(
                "not sending {}: outside the temp dir and workspace",
                path.display()
            );
            return;
        }
        let key = self.next_key();
        let attachment = match self
            .bridge
            .api
            .upload(&format!("{key}:upload"), path, content_type)
            .await
        {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!("could not upload {}: {e:#}", path.display());
                return;
            }
        };
        let content = json!({ "type": "attachment", "attachmentId": attachment });
        if let Err(e) = self
            .bridge
            .api
            .send(&key, &self.chat.space, agent, None, content)
            .await
        {
            tracing::warn!("could not send {}: {e:#}", path.display());
        }
    }
}
