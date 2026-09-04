//! What every request shares: the CLI it spawns, the settings the browser
//! chose, the runs in flight, and the channel every open tab listens on.

use std::net::SocketAddr;
use std::path::PathBuf;

use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio::sync::{Mutex, broadcast};

use crate::cli::Cli;
use crate::run::Run;
use crate::settings::Settings;

/// How many messages a tab can fall behind before it starts dropping them. A
/// streaming turn is chatty, and a backgrounded tab still has to keep up.
const BACKLOG: usize = 2048;

pub struct AppState {
    pub cli: Cli,
    /// The address the banner printed, and what the guard checks `Host` against.
    pub bind: SocketAddr,
    /// Set when the server is reachable off this machine, and then required.
    pub token: Option<String>,
    /// One `ToWebview` message per item, already JSON.
    pub events: broadcast::Sender<String>,
    pub settings: Mutex<Settings>,
    pub chat: Mutex<Option<Run>>,
    pub review: Mutex<Option<Run>>,
    pub login: Mutex<Option<oneshot::Sender<()>>>,
}

impl AppState {
    pub fn new(repo_root: PathBuf, bind: SocketAddr, token: Option<String>) -> Self {
        let (events, _) = broadcast::channel(BACKLOG);
        Self {
            cli: Cli::new(repo_root),
            bind,
            token,
            events,
            settings: Mutex::new(Settings::load()),
            chat: Mutex::new(None),
            review: Mutex::new(None),
            login: Mutex::new(None),
        }
    }

    /// Send one message to every open tab. No listeners is normal: the user
    /// closed the tab mid-turn, and the turn carries on regardless.
    pub fn post(&self, message: Value) {
        if let Ok(line) = serde_json::to_string(&message) {
            let _ = self.events.send(line);
        }
    }

    /// Tell every tab what is running, so none is left stuck busy or falsely
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
