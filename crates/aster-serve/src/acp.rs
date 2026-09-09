//! One long-lived `aster acp` child per tab. Chat turns run as ACP prompts
//! over its stdio, so the agent boots once instead of once per message, and
//! ACP traffic is translated back into the NDJSON events the tab already
//! understands. An agent belongs to its session, not its tab: loading that
//! session in another tab moves the agent there and leaves a clean slate.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::ChildStdin;
use tokio::sync::{Mutex as AsyncMutex, oneshot};

use crate::state::{AppState, Instance};

use crate::run::Run;

/// Transcript session id to the agent bound to it. Weak, so a dead agent is
/// never kept alive by its own entry.
pub type Registry = AsyncMutex<HashMap<String, Weak<Agent>>>;

/// A permission the agent asked about and the browser has not answered yet.
struct Permission {
    id: u64,
    question: bool,
    options: Vec<Value>,
}

/// A message that arrived while a turn was running, waiting its turn.
struct Queued {
    id: String,
    message: Value,
    mode: String,
}

/// Everything one in-flight prompt accumulates.
struct Turn {
    event_id: String,
    reply: String,
    edits: Vec<String>,
    tool_names: HashMap<String, String>,
    permission: Option<Permission>,
    /// Reasoning chars seen so far; ACP gives no token counts, so the tab
    /// gets a chars/4 estimate. `None` once the answer starts.
    reasoning_chars: usize,
    reasoning_started: Option<Instant>,
    /// Mirrored into the turn's `Run`, so a tab that loads mid-prompt still
    /// sees the card the browser that started the turn saw.
    pending: Arc<Mutex<Option<Value>>>,
}

/// Session state that survives between turns: the agent holds the history, so
/// only the newest user message crosses the wire.
#[derive(Default)]
struct Inner {
    session_id: Option<String>,
    /// The browser session the agent session is bound to; `None` for a fresh
    /// conversation.
    loaded: Option<String>,
    synced: usize,
    /// User messages the child has already received, oldest first. The browser
    /// caps the history it sends, so past the cap the message count stops
    /// growing and this is what tells a new message from a repeated one.
    sent_users: Vec<String>,
    model: Option<String>,
    effort: Option<String>,
    mode: Option<String>,
    injected: Vec<String>,
    loading: bool,
    turn: Option<Turn>,
}

pub struct Agent {
    /// Behind a lock because an agent can move to another tab when its
    /// session is loaded there.
    instance: std::sync::Mutex<Weak<Instance>>,
    registry: Weak<Registry>,
    root: std::path::PathBuf,
    /// Kept so a discarded agent's child can be killed instead of leaking.
    child: AsyncMutex<Option<tokio::process::Child>>,
    stdin: AsyncMutex<ChildStdin>,
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, oneshot::Sender<Value>>>,
    inner: Mutex<Inner>,
    queue: Mutex<Vec<Queued>>,
    /// Set by the reader task when the child's stdout closes; the session
    /// state is worthless from then on.
    dead: AtomicBool,
}

impl Agent {
    /// The tab's agent, or a fresh one. A dead child is replaced on the next
    /// turn; the spawn error is the only explanation available. The slot lock
    /// is held across the whole spawn, so two chats never race into two
    /// children with one leaked.
    pub async fn ensure(state: &AppState, instance: &Arc<Instance>) -> Result<Arc<Agent>, String> {
        let cli = &state.cli;
        let mut slot = instance.agent.lock().await;
        if let Some(agent) = slot.as_ref()
            && agent.alive()
        {
            return Ok(agent.clone());
        }
        let mut child = cli
            .command(&["acp"])
            .spawn()
            .map_err(|e| format!("could not launch aster acp: {e}"))?;
        let stdout = child.stdout.take().ok_or("aster acp has no stdout")?;
        let stdin = child.stdin.take().ok_or("aster acp has no stdin")?;
        let stderr = child.stderr.take();
        let agent = Arc::new(Agent {
            instance: std::sync::Mutex::new(Arc::downgrade(instance)),
            registry: Arc::downgrade(&state.agents),
            root: cli.root.clone(),
            child: AsyncMutex::new(Some(child)),
            stdin: AsyncMutex::new(stdin),
            next_id: AtomicU64::new(0),
            pending: Mutex::new(HashMap::new()),
            inner: Mutex::new(Inner::default()),
            queue: Mutex::new(Vec::new()),
            dead: AtomicBool::new(false),
        });
        if let Some(stderr) = stderr {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if !line.trim().is_empty() {
                        eprintln!("{line}");
                    }
                }
            });
        }
        let reader = agent.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<Value>(line) {
                    Ok(message) => reader.route(message).await,
                    Err(_) => tracing::debug!("{line}"),
                }
            }
            reader.dead.store(true, Ordering::SeqCst);
            reader.fail_all("aster acp exited unexpectedly. The next message restarts it.");
            // Only evict a slot that still holds this agent; a replacement
            // may already have been spawned by the time the reader unwinds.
            let instance = reader.instance.lock().ok().and_then(|weak| weak.upgrade());
            if let Some(instance) = instance {
                let mut slot = instance.agent.lock().await;
                if slot
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &reader))
                {
                    slot.take();
                }
            }
        });
        *slot = Some(agent.clone());
        drop(slot);
        let initialized = agent
            .call(
                "initialize",
                json!({
                    "protocolVersion": 1,
                    "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false } },
                    "clientInfo": { "name": "aster-serve", "title": "Aster", "version": "0.5.0" }
                }),
            )
            .await;
        if initialized.is_err() {
            {
                let mut child = agent.child.lock().await;
                if let Some(child) = child.as_mut() {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                }
            }
            let mut slot = instance.agent.lock().await;
            if slot
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &agent))
            {
                slot.take();
            }
            return initialized.map(|_| agent);
        }
        // Reap the child when it eventually exits; nobody else waits on it.
        let reaper = agent.clone();
        tokio::spawn(async move {
            let mut child = reaper.child.lock().await;
            if let Some(child) = child.as_mut() {
                let _ = child.wait().await;
            }
        });
        initialized.map(|_| agent)
    }

    /// Queued messages belong to the conversation being cancelled or left;
    /// they never run.
    pub fn drop_queue(&self) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.clear();
        }
    }

    fn alive(&self) -> bool {
        !self.dead.load(Ordering::SeqCst)
    }

    fn instance(&self) -> Option<Arc<Instance>> {
        self.instance.lock().ok().and_then(|weak| weak.upgrade())
    }

    /// The transcript session the agent is bound to, if any.
    pub fn loaded(&self) -> Option<String> {
        self.inner
            .lock()
            .ok()
            .and_then(|inner| inner.loaded.clone())
    }

    fn set_instance(&self, instance: &Arc<Instance>) {
        if let Ok(mut weak) = self.instance.lock() {
            *weak = Arc::downgrade(instance);
        }
    }

    /// Forget a registry entry that still points at this agent.
    async fn release(self: &Arc<Self>, session: &str) {
        if let Some(registry) = self.registry.upgrade() {
            let mut registry = registry.lock().await;
            if registry
                .get(session)
                .is_some_and(|weak| weak.upgrade().is_some_and(|a| Arc::ptr_eq(&a, self)))
            {
                registry.remove(session);
            }
        }
    }

    /// Kill the child. Used when the agent's session moved to another tab and
    /// nothing here needs the process any more.
    async fn discard(self: &Arc<Self>) {
        self.dead.store(true, Ordering::SeqCst);
        self.fail_all("aster acp was switched away from.");
        self.drop_queue();
        // The turn task unwinding after the kill must not post into the tab
        // that just reset, so its state goes now.
        self.inner.lock().expect("agent state poisoned").turn = None;
        if let Some(previous) = self.loaded() {
            self.release(&previous).await;
        }
        // Closing stdin ends the child, which lets the reaper's wait return
        // and free the child lock the kill below needs.
        {
            let mut stdin = self.stdin.lock().await;
            let _ = stdin.shutdown().await;
        }
        let mut child = self.child.lock().await;
        if let Some(child) = child.as_mut() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }

    /// The tab's agent bound to the session the tab is showing. A session
    /// another tab's agent owns moves here whole: that agent is cancelled,
    /// re-pointed at this tab, and the agent that was here is killed, so its
    /// next chat starts a fresh session. One session id never has two agents.
    pub async fn for_session(
        state: &AppState,
        instance: &Arc<Instance>,
        wanted: Option<&str>,
    ) -> Result<Arc<Agent>, String> {
        let agent = Self::ensure(state, instance).await?;
        if agent.loaded().as_deref() == wanted {
            return Ok(agent);
        }
        let owner = match wanted {
            Some(wanted) => state
                .agents
                .lock()
                .await
                .get(wanted)
                .and_then(Weak::upgrade)
                .filter(|owner| owner.alive())
                .filter(|owner| !Arc::ptr_eq(owner, &agent)),
            None => None,
        };
        let Some(owner) = owner else {
            // Nobody else owns it; this agent rebinds at its next turn, and
            // its stale registry entry goes with that bind.
            if let Some(previous) = agent.loaded() {
                agent.release(&previous).await;
            }
            return Ok(agent);
        };
        owner.cancel().await;
        owner.drop_queue();
        let old = owner.instance();
        if let Some(old) = &old {
            let mut slot = old.agent.lock().await;
            if slot
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &owner))
            {
                slot.take();
            }
        }
        owner.set_instance(instance);
        *instance.agent.lock().await = Some(owner.clone());
        // The old tab's agent is gone, so its UI resets to a fresh
        // conversation rather than showing a session it no longer owns.
        if let Some(old) = old {
            old.post(json!({ "type": "newConversation" }));
        }
        if let Some(previous) = agent.loaded() {
            agent.release(&previous).await;
        }
        agent.discard().await;
        Ok(owner)
    }

    fn fail_all(&self, message: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            for (_, sender) in pending.drain() {
                let _ = sender.send(json!({ "__error": message }));
            }
        }
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let (sender, receiver) = oneshot::channel();
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.pending
            .lock()
            .map_err(|_| "agent state poisoned".to_string())?
            .insert(id, sender);
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(format!("{line}\n").as_bytes())
            .await
            .map_err(|e| format!("could not reach aster acp: {e}"))?;
        drop(stdin);
        let answer = receiver
            .await
            .map_err(|_| "aster acp is gone".to_string())?;
        if let Some(error) = answer["__error"].as_str() {
            return Err(error.to_string());
        }
        if let Some(error) = answer["error"]["message"].as_str() {
            return Err(error.to_string());
        }
        Ok(answer["result"].clone())
    }

    async fn write_line(&self, line: Value) {
        let mut stdin = self.stdin.lock().await;
        let _ = stdin.write_all(format!("{line}\n").as_bytes()).await;
    }

    /// One inbound line from the agent: a call to answer, an answer to one of
    /// ours, or a session update to translate.
    async fn route(&self, message: Value) {
        let id = message["id"].as_u64();
        if message["method"].is_string() {
            if let Some(id) = id {
                self.serve(id, &message).await;
            }
            return;
        }
        if let Some(id) = id {
            let sender = self
                .pending
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(&id));
            if let Some(sender) = sender {
                let _ = sender.send(message);
                return;
            }
        }
        if message["method"] == json!("session/update")
            && !self.inner.lock().is_ok_and(|inner| inner.loading)
        {
            self.translate(&message["params"]["update"]).await;
        }
    }

    /// Requests the agent sends a client. Only permissions are supported; the
    /// fs and terminal capabilities were never declared, so anything else
    /// gets a refusal instead of a hang.
    async fn serve(&self, id: u64, message: &Value) {
        if message["method"] != json!("session/request_permission") {
            self.write_line(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": "not supported" }
            }))
            .await;
            return;
        }
        let options = message["params"]["options"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let fields = &message["params"]["toolCall"]["fields"];
        let question = fields["kind"] == json!("think");
        let body = content_text(&fields["content"]);
        let title = fields["title"].as_str().unwrap_or_default().to_string();
        let event = if question {
            json!({
                "type": "question",
                "header": if title.is_empty() { "A question".to_string() } else { title.clone() },
                "question": body,
                "options": options
                    .iter()
                    .filter(|o| o["kind"] == json!("allow_once"))
                    .map(|o| o["name"].clone())
                    .collect::<Vec<_>>(),
            })
        } else {
            json!({
                "type": "approval_request",
                "kind": if fields["kind"] == json!("switch_mode") { "plan" } else { "action" },
                "preview": if title.is_empty() { "Approve this action".to_string() } else { title.clone() },
                "markdown": if body.is_empty() { Value::Null } else { json!(body) },
                "scope": Value::Null,
            })
        };
        let posted = {
            let mut inner = self.inner.lock().expect("agent state poisoned");
            match inner.turn.as_mut() {
                Some(turn) => {
                    let event_id = turn.event_id.clone();
                    if let Ok(mut slot) = turn.pending.lock() {
                        *slot = Some(event.clone());
                    }
                    turn.permission = Some(Permission {
                        id,
                        question,
                        options,
                    });
                    Some(event_id)
                }
                None => None,
            }
        };
        match posted {
            Some(event_id) => self.post(&event_id, &event),
            // No turn is listening, so the agent gets a clean refusal rather
            // than a hang.
            None => {
                self.write_line(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "outcome": { "outcome": "cancelled" } }
                }))
                .await;
            }
        }
    }

    fn post(&self, event_id: &str, event: &Value) {
        if let Some(instance) = self.instance() {
            instance.post(json!({ "type": "chatEvent", "id": event_id, "event": event }));
        }
    }

    /// Claim the tab's chat slot and run the turn in the background, so the
    /// POST that started it returns at once. The agent keeps its history, so
    /// only the messages it has not seen cross the wire.
    pub async fn chat(
        self: &Arc<Self>,
        state: &Arc<AppState>,
        instance: &Arc<Instance>,
        id: String,
        message: &Value,
        mode: &str,
    ) -> Result<(), String> {
        let mut slot = instance.chat.lock().await;
        // A message during an in-flight turn queues behind it and runs when
        // the turn finishes; nothing the user typed is ever dropped.
        if slot.is_some() {
            drop(slot);
            self.queue
                .lock()
                .map_err(|_| "agent state poisoned".to_string())?
                .push(Queued {
                    id,
                    message: message.clone(),
                    mode: mode.to_string(),
                });
            return Ok(());
        }
        let pending: Arc<Mutex<Option<Value>>> = Arc::new(std::sync::Mutex::new(None));
        *slot = Some(Run::detached(id.clone(), pending.clone()));
        drop(slot);

        let agent = self.clone();
        let message = message.clone();
        let mode = mode.to_string();
        let instance = instance.clone();
        let state = state.clone();
        tokio::spawn(async move {
            // One task runs the whole chain: this turn, then each queued
            // message in order, so nothing recurses through `chat`.
            let mut agent = agent;
            let mut current = Some((id, message, mode, pending));
            while let Some((id, message, mode, pending)) = current.take() {
                let outcome = agent.clone().turn(&id, &message, &mode, pending).await;
                // Only clear a slot this turn still owns; a replacement turn
                // may already have claimed it.
                let mut slot = instance.chat.lock().await;
                if slot.as_ref().is_some_and(|run| run.id == id) {
                    slot.take();
                }
                drop(slot);
                if let Err(error) = outcome {
                    instance.post(json!({ "type": "chatError", "id": id, "message": error }));
                }
                instance.post_run_state().await;
                let next = agent
                    .queue
                    .lock()
                    .ok()
                    .and_then(|mut queue| (!queue.is_empty()).then(|| queue.remove(0)));
                let Some(next) = next else { break };
                // A queued message may be for a session another tab's agent
                // took over while this turn ran; the agent follows it, so one
                // session id never ends up bound to two agents.
                agent = match Self::for_session(&state, &instance, next.message["session"].as_str())
                    .await
                {
                    Ok(agent) => agent,
                    Err(error) => {
                        instance
                            .post(json!({ "type": "chatError", "id": next.id, "message": error }));
                        break;
                    }
                };
                let pending: Arc<Mutex<Option<Value>>> = Arc::new(std::sync::Mutex::new(None));
                let mut slot = instance.chat.lock().await;
                if slot.is_some() {
                    // A fresh chat claimed the slot first; back of its queue.
                    drop(slot);
                    if let Ok(mut queue) = agent.queue.lock() {
                        queue.insert(0, next);
                    }
                    break;
                }
                *slot = Some(Run::detached(next.id.clone(), pending.clone()));
                drop(slot);
                current = Some((next.id, next.message, next.mode, pending));
            }
        });
        Ok(())
    }

    async fn turn(
        self: Arc<Self>,
        id: &str,
        message: &Value,
        mode: &str,
        pending: Arc<Mutex<Option<Value>>>,
    ) -> Result<(), String> {
        {
            let mut inner = self.inner.lock().expect("agent state poisoned");
            inner.turn = Some(Turn {
                event_id: id.to_string(),
                reply: String::new(),
                edits: Vec::new(),
                tool_names: HashMap::new(),
                permission: None,
                reasoning_chars: 0,
                reasoning_started: None,
                pending,
            });
        }
        let result = self.prompt_turn(message, mode).await;
        // A newer turn may have replaced this one mid-prompt; it owns the
        // state and the tab now, so this one bows out.
        let turn = {
            let mut inner = self.inner.lock().expect("agent state poisoned");
            if inner.turn.as_ref().is_some_and(|turn| turn.event_id == id) {
                inner.turn.take().expect("turn state was just set")
            } else {
                return Ok(());
            }
        };
        if let Ok(mut slot) = turn.pending.lock() {
            *slot = None;
        }
        match result {
            Ok(Some(stop)) if stop == "refusal" => {
                self.post(
                    id,
                    &json!({
                        "type": "error",
                        "message": "The model refused to continue this turn.",
                    }),
                );
                Ok(())
            }
            Ok(_) => {
                self.post(
                    id,
                    &json!({ "type": "done", "reply": turn.reply, "edits": turn.edits }),
                );
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    async fn prompt_turn(
        self: &Arc<Self>,
        message: &Value,
        mode: &str,
    ) -> Result<Option<String>, String> {
        let messages = message["messages"].as_array().cloned().unwrap_or_default();
        let wanted = message["session"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let needs_bind = {
            let inner = self.inner.lock().expect("agent state poisoned");
            inner.session_id.is_none()
                || inner.loaded != wanted
                || messages.len() < inner.synced
                || (messages.len() == 1 && inner.synced >= 1)
        };
        let session_id = if needs_bind {
            self.bind(&wanted, &messages).await?
        } else {
            self.inner
                .lock()
                .expect("agent state poisoned")
                .session_id
                .clone()
                .ok_or("the agent session is gone")?
        };

        // Only what changed goes over the wire; a repeated value would still
        // cost a round trip to the child.
        let (model, effort, mode_changed) = {
            let inner = self.inner.lock().expect("agent state poisoned");
            (
                message["model"]
                    .as_str()
                    .filter(|m| !m.is_empty() && inner.model.as_deref() != Some(m)),
                message["effort"]
                    .as_str()
                    .filter(|e| !e.is_empty() && inner.effort.as_deref() != Some(e)),
                inner.mode.as_deref() != Some(mode),
            )
        };
        if let Some(model) = model {
            self.call(
                "session/set_config_option",
                json!({ "sessionId": session_id, "configId": "model", "value": model }),
            )
            .await?;
        }
        if let Some(effort) = effort {
            self.call(
                "session/set_config_option",
                json!({ "sessionId": session_id, "configId": "effort", "value": effort }),
            )
            .await?;
        }
        if mode_changed {
            self.call(
                "session/set_mode",
                json!({ "sessionId": session_id, "modeId": mode }),
            )
            .await?;
        }
        {
            let mut inner = self.inner.lock().expect("agent state poisoned");
            if let Some(model) = model {
                inner.model = Some(model.to_string());
            }
            if let Some(effort) = effort {
                inner.effort = Some(effort.to_string());
            }
            if mode_changed {
                inner.mode = Some(mode.to_string());
            }
        }

        let (fresh, injected) = {
            let mut inner = self.inner.lock().expect("agent state poisoned");
            let fresh = if messages.len() > inner.synced {
                messages
                    .iter()
                    .skip(inner.synced)
                    .filter(|m| m["role"] == json!("user"))
                    .map(|m| {
                        json!({ "type": "text", "text": m["content"].as_str().unwrap_or_default() })
                    })
                    .collect::<Vec<_>>()
            } else {
                // The browser caps the history it sends, so past the cap the
                // count stops growing and the window slides. The new messages
                // are the tail after the last assistant reply, minus the ones
                // the child already has.
                let start = messages.len()
                    - messages
                        .iter()
                        .rev()
                        .take_while(|m| m["role"] != json!("assistant"))
                        .count();
                let tail = messages[start..]
                    .iter()
                    .filter(|m| m["role"] == json!("user"))
                    .map(|m| m["content"].as_str().unwrap_or_default().to_string())
                    .collect::<Vec<_>>();
                let already = (1..=inner.sent_users.len().min(tail.len()))
                    .rev()
                    .find(|k| tail[..*k] == inner.sent_users[inner.sent_users.len() - *k..])
                    .unwrap_or(0);
                tail[already..]
                    .iter()
                    .map(|text| json!({ "type": "text", "text": text }))
                    .collect::<Vec<_>>()
            };
            inner.synced = messages.len();
            let sent = fresh
                .iter()
                .map(|block| block["text"].as_str().unwrap_or_default().to_string());
            inner.sent_users.extend(sent);
            let excess = inner.sent_users.len().saturating_sub(32);
            inner.sent_users.drain(..excess);
            (fresh, std::mem::take(&mut inner.injected))
        };
        if fresh.is_empty() && injected.is_empty() {
            return Ok(None);
        }
        let mut prompt = Vec::with_capacity(fresh.len() + 1);
        if !injected.is_empty() {
            prompt.push(json!({ "type": "text", "text": injected.join("\n\n") }));
        }
        prompt.extend(fresh);
        let result = self
            .call(
                "session/prompt",
                json!({ "sessionId": session_id, "prompt": prompt }),
            )
            .await?;
        Ok(result["stopReason"].as_str().map(str::to_owned))
    }

    /// Point the agent at the transcript the browser is showing: load the
    /// stored session, or start a fresh one. While loading, the agent replays
    /// the history as updates, which the tab must not render twice.
    async fn bind(
        self: &Arc<Self>,
        wanted: &Option<String>,
        messages: &[Value],
    ) -> Result<String, String> {
        let message_count = messages.len();
        let cwd = self.root.to_string_lossy().to_string();
        let session_id = match wanted {
            Some(wanted) => {
                self.inner.lock().expect("agent state poisoned").loading = true;
                let loaded = self
                    .call(
                        "session/load",
                        json!({ "cwd": cwd, "mcpServers": [], "sessionId": wanted }),
                    )
                    .await;
                self.inner.lock().expect("agent state poisoned").loading = false;
                let loaded = loaded?;
                let last_is_user = message_count > 0;
                let mut inner = self.inner.lock().expect("agent state poisoned");
                inner.synced = message_count - usize::from(last_is_user);
                loaded["sessionId"].as_str().unwrap_or(wanted).to_string()
            }
            None => {
                let params = json!({ "cwd": cwd, "mcpServers": [] });
                let mut created = self.call("session/new", params.clone()).await?;
                // A missing id once is odd; twice in a row means the child
                // is broken, and the plain error says so.
                if created["sessionId"].as_str().is_none_or(str::is_empty) {
                    created = self.call("session/new", params).await?;
                }
                self.inner.lock().expect("agent state poisoned").synced = 0;
                created["sessionId"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .ok_or("the agent did not start a session. Send the message again.")?
            }
        };
        let previous = {
            let mut inner = self.inner.lock().expect("agent state poisoned");
            let previous = inner.loaded.take();
            inner.session_id = Some(session_id.clone());
            inner.loaded = wanted.clone();
            inner.model = None;
            inner.effort = None;
            inner.mode = None;
            inner.sent_users = messages[..inner.synced.min(messages.len())]
                .iter()
                .filter(|m| m["role"] == json!("user"))
                .filter_map(|m| m["content"].as_str())
                .map(str::to_owned)
                .collect();
            previous
        };
        // The registry is what lets a session move between tabs instead of
        // being bound twice, so it is kept exact: the old entry goes if it is
        // still ours, the new one goes in.
        if let Some(registry) = self.registry.upgrade() {
            let mut registry = registry.lock().await;
            if let Some(previous) = previous
                && registry
                    .get(&previous)
                    .is_some_and(|weak| weak.upgrade().is_some_and(|a| Arc::ptr_eq(&a, self)))
            {
                registry.remove(&previous);
            }
            if let Some(wanted) = wanted {
                registry.insert(wanted.clone(), Arc::downgrade(self));
            }
        }
        Ok(session_id)
    }

    /// An approval, a question answer, or a mid-turn injection from the tab.
    pub async fn answer(&self, message: &Value) -> Result<(), String> {
        if message["type"] == json!("inject") {
            if let Some(text) = message["text"].as_str() {
                self.inner
                    .lock()
                    .expect("agent state poisoned")
                    .injected
                    .push(text.to_string());
            }
            return Ok(());
        }
        let (permission_id, options, question) = {
            let mut inner = self.inner.lock().expect("agent state poisoned");
            let turn = inner.turn.as_mut().ok_or("no turn is running")?;
            let permission = turn.permission.take().ok_or("no prompt is waiting")?;
            (permission.id, permission.options, permission.question)
        };
        let wanted = if question {
            let choice = message["choice"].as_str().unwrap_or_default();
            options
                .iter()
                .find(|o| o["name"] == json!(choice))
                .and_then(|o| o["optionId"].as_str())
                .unwrap_or("skip")
                .to_string()
        } else {
            let allow = message["allow"].as_bool().unwrap_or(false);
            match (allow, message["always"].as_bool() == Some(true)) {
                (true, true) => "allow_always".to_string(),
                (true, false) => "allow".to_string(),
                (false, _) => "reject".to_string(),
            }
        };
        self.respond(permission_id, &options, &wanted).await;
        if let Some(turn) = self
            .inner
            .lock()
            .expect("agent state poisoned")
            .turn
            .as_ref()
            && let Ok(mut slot) = turn.pending.lock()
        {
            *slot = None;
        }
        Ok(())
    }

    async fn respond(&self, id: u64, options: &[Value], wanted: &str) {
        let picked = options
            .iter()
            .find(|o| o["optionId"] == json!(wanted))
            .or_else(|| {
                options.iter().find(|o| {
                    o["kind"] == json!("allow_once") || o["kind"] == json!("allow_always")
                })
            })
            .or_else(|| options.iter().find(|o| o["kind"] == json!("reject_once")))
            .or_else(|| options.first());
        let result = match picked {
            Some(option) => json!({
                "outcome": { "outcome": "selected", "optionId": option["optionId"] }
            }),
            None => json!({ "outcome": { "outcome": "cancelled" } }),
        };
        self.write_line(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
            .await;
    }

    /// Stop the turn in flight. An idle cancel leaves the agent warm; the
    /// next message reuses it.
    pub async fn cancel(&self) {
        let (in_flight, session_id) = {
            let inner = self.inner.lock().expect("agent state poisoned");
            (inner.turn.is_some(), inner.session_id.clone())
        };
        if in_flight && let Some(session_id) = session_id {
            self.write_line(json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": { "sessionId": session_id }
            }))
            .await;
        }
    }

    /// One session update from the agent, translated into the event the tab
    /// already renders. Plan and available-commands updates have no
    /// equivalent here, so they are dropped rather than smuggled as text.
    async fn translate(&self, update: &Value) {
        let kind = update["sessionUpdate"].as_str().unwrap_or_default();
        let (event_id, event) = {
            let mut inner = self.inner.lock().expect("agent state poisoned");
            let event = match kind {
                "agent_message_chunk" => {
                    let text = content_text(&update["content"]);
                    let done = {
                        let turn = inner.turn.as_mut().expect("chunk outside a turn");
                        turn.reply.push_str(&text);
                        turn.reasoning_started.take().map(|start| {
                            json!({
                                "type": "reasoning_done",
                                "tokens": turn.reasoning_chars / 4,
                                "duration_ms": start.elapsed().as_millis() as u64,
                            })
                        })
                    };
                    if let Some(done) = done {
                        self.post(&inner.turn.as_ref().unwrap().event_id, &done);
                    }
                    json!({ "type": "token", "content": text })
                }
                "agent_thought_chunk" => {
                    let text = content_text(&update["content"]);
                    let tokens = match inner.turn.as_mut() {
                        Some(turn) => {
                            if turn.reasoning_started.is_none() {
                                turn.reasoning_started = Some(Instant::now());
                            }
                            turn.reasoning_chars += text.chars().count();
                            turn.reasoning_chars / 4
                        }
                        None => return,
                    };
                    json!({
                        "type": "reasoning_delta",
                        "content": text,
                        "tokens": tokens,
                    })
                }
                "tool_call" => {
                    let id = update["toolCallId"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let name = update["name"]
                        .as_str()
                        .or(update["title"].as_str())
                        .unwrap_or("tool")
                        .to_string();
                    if let Some(turn) = inner.turn.as_mut() {
                        turn.tool_names.insert(id.clone(), name.clone());
                        if name == "edit_file" && update["rawInput"]["path"].is_string() {
                            turn.edits
                                .push(update["rawInput"]["path"].as_str().unwrap().to_string());
                        }
                    }
                    let raw = &update["rawInput"];
                    let arguments = match raw {
                        Value::String(text) => json!(text),
                        Value::Null => json!({}),
                        other => other.clone(),
                    };
                    json!({ "type": "tool_call", "id": id, "name": name, "arguments": arguments })
                }
                "tool_call_update" => {
                    let id = update["toolCallId"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let name = inner
                        .turn
                        .as_ref()
                        .and_then(|turn| turn.tool_names.get(&id))
                        .cloned()
                        .unwrap_or_default();
                    let output = &update["fields"]["rawOutput"];
                    let result = match output {
                        Value::String(text) => json!(text),
                        Value::Null => json!(""),
                        other => json!(serde_json::to_string(other).unwrap_or_default()),
                    };
                    json!({
                        "type": "tool_result",
                        "id": id,
                        "name": name,
                        "result": result,
                        "error": update["fields"]["status"] == json!("failed"),
                    })
                }
                "session_info" => match update["title"].as_str() {
                    Some(title) if !title.is_empty() => json!({ "type": "title", "title": title }),
                    _ => return,
                },
                "current_mode_update" => {
                    if let Some(mode) = update["currentModeId"].as_str() {
                        inner.mode = Some(mode.to_string());
                    }
                    return;
                }
                _ => return,
            };
            // Updates outside a turn have no tab event to attach to; the
            // browser would see an empty id and render it on nothing.
            let event_id = inner.turn.as_ref().map(|turn| turn.event_id.clone());
            (event_id, event)
        };
        if let Some(event_id) = event_id {
            self.post(&event_id, &event);
        }
    }
}

fn content_text(content: &Value) -> String {
    match content {
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join(""),
        Value::String(text) => text.clone(),
        _ => String::new(),
    }
}
