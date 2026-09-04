//! What every request shares: the CLI it spawns and the settings the browser
//! chose, plus one instance per tab, so each tab runs its own session.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio::sync::{Mutex, broadcast};

use crate::cli::Cli;
use crate::run::Run;
use crate::settings::Settings;

const BACKLOG: usize = 2048;

const DEFAULT_INSTANCE: &str = "default";

/// One tab's state: its runs in flight and the channel only that tab listens
/// on. Two tabs are two instances, so a turn in one never blocks the other.
pub struct Instance {
    pub chat: Mutex<Option<Run>>,
    pub review: Mutex<Option<Run>>,
    pub events: broadcast::Sender<String>,
}

impl Instance {
    fn new() -> Self {
        let (events, _) = broadcast::channel(BACKLOG);
        Self {
            chat: Mutex::new(None),
            review: Mutex::new(None),
            events,
        }
    }

    /// Send one message to this tab. No listeners is normal: the user closed
    /// the tab mid-turn, and the turn carries on regardless.
    pub fn post(&self, message: Value) {
        if let Ok(line) = serde_json::to_string(&message) {
            let _ = self.events.send(line);
        }
    }

    /// Tell the tab what is running, so it is not left stuck busy or falsely
    /// idle after a reload. A chat blocked on a prompt says which one, so a
    /// tab that loads mid-prompt still gets the approval card.
    pub async fn post_run_state(&self) {
        let chat = self.chat.lock().await;
        let review = self.review.lock().await.is_some();
        let mut message = json!({ "type": "runState", "chat": chat.is_some(), "review": review });
        if let Some(run) = chat.as_ref() {
            message["id"] = json!(run.id);
            if let Some(event) = run.blocked_on() {
                message["pending"] = event;
            }
        }
        drop(chat);
        self.post(message);
    }

    /// Answer the running turn: an approval, a question, or a message queued
    /// while it was working. All three are one JSON line on its stdin.
    pub async fn answer(&self, line: Value) -> Result<(), String> {
        let mut slot = self.chat.lock().await;
        let run = slot.as_mut().ok_or("no turn is running")?;
        run.write(&line.to_string()).await?;
        // The prompt is settled; a tab loading now must not be shown it again.
        run.clear_pending();
        Ok(())
    }
}

pub struct AppState {
    pub cli: Cli,
    pub bind: SocketAddr,
    pub token: Option<String>,
    instances: Mutex<HashMap<String, Arc<Instance>>>,
    pub settings: Mutex<Settings>,
    pub login: Mutex<Option<oneshot::Sender<()>>>,
}

impl AppState {
    pub fn new(repo_root: PathBuf, bind: SocketAddr, token: Option<String>) -> Self {
        Self {
            cli: Cli::new(repo_root),
            bind,
            token,
            instances: Mutex::new(HashMap::new()),
            settings: Mutex::new(Settings::load()),
            login: Mutex::new(None),
        }
    }

    /// The tab a message came from, creating it on first sight. Instances are
    /// never removed: a closed tab's channel is cheap, and a reload with the
    /// same id must find its run still there.
    pub async fn instance(&self, id: &str) -> Arc<Instance> {
        let mut instances = self.instances.lock().await;
        instances
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(Instance::new()))
            .clone()
    }

    /// The instance a host message names. The page generates one id per tab
    /// and sends it with every message; a missing one shares the default.
    pub async fn instance_for(&self, message: &Value) -> Arc<Instance> {
        let id = message
            .get("instance")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .unwrap_or(DEFAULT_INSTANCE);
        self.instance(id).await
    }
}
