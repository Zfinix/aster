//! Telegram adapter: long-polls the Bot API, runs one agent turn per incoming
//! message, and relays approval prompts as inline keyboards. Tool calls stream
//! into a live-edited activity message so the chat mirrors the CLI.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;

use crate::bridge::{Answer, Turn, TurnEvent, TurnOutcome, WireMessage, run_turn};
use crate::markdown;

/// Telegram caps messages at 4096 chars; leave headroom for tags.
const CHUNK_LIMIT: usize = 4000;

/// How many activity lines stay visible before older ones collapse.
const ACTIVITY_WINDOW: usize = 12;

/// Minimum gap between edits of the activity message (Telegram rate limit).
const ACTIVITY_EDIT_GAP: Duration = Duration::from_millis(1500);

pub struct TelegramConfig {
    /// Bot token from @BotFather.
    pub token: String,
    /// Telegram user ids allowed to drive the agent. Empty rejects everyone
    /// while telling senders their id, which is the onboarding path.
    pub allowed_users: Vec<i64>,
    /// Path to the `aster` binary to spawn per turn.
    pub bin: PathBuf,
    /// Repository the agent operates on.
    pub repo_root: PathBuf,
    /// Permission mode for remote turns.
    pub mode: String,
}

/// Injected ahead of each turn so the agent writes for a phone chat, not a
/// terminal. Sent as an extra system message on the wire.
const TELEGRAM_SYSTEM: &str = "\
The user is talking to you through a Telegram chat on their phone (via aster \
remote), not a terminal. Adjust how you answer: \
Keep replies short and conversational; lead with the answer. Phone screens \
are small, so prefer a few sentences over structure. \
Formatting support is limited to **bold**, `inline code`, fenced code blocks, \
and simple bullet lists. Never use tables, nested lists, or deep header \
hierarchies; they render as noise. Keep code snippets small and only when asked. \
Reference files as `path` in backticks; there are no clickable file links. \
Approval prompts and questions reach the user as tappable buttons; if one is \
denied or skipped, take the hint and do not immediately re-request it. \
When you want to send a gif (e.g. via the giphy tools), put its URL on a line \
by itself and it will render as a playing animation. \
The `telegram` MCP server gives you chat tools: react (emoji-react to the \
user's message; use sparingly), send_gif, send_photo, send_document (share a \
repo file), send_poll, and send_code_page (publish long code or reports as an \
in-app page instead of flooding the chat). Prefer them over describing what \
you would send.";

/// A prompt waiting for a button tap.
enum Pending {
    Approval(oneshot::Sender<Answer>),
    Question {
        options: Vec<String>,
        respond: oneshot::Sender<Option<String>>,
    },
}

#[derive(Default)]
struct ChatState {
    history: Vec<WireMessage>,
    pending: Option<Pending>,
    running: Option<AbortHandle>,
    /// Per-chat overrides set with /mode, /model, and /effort.
    mode: Option<String>,
    model: Option<String>,
    effort: Option<String>,
}

type Chats = Arc<Mutex<HashMap<i64, ChatState>>>;

/// Run the Telegram bridge until the process is stopped.
pub async fn run_telegram(cfg: TelegramConfig) -> Result<()> {
    let api = Api::new(&cfg.token)?;
    let me = api
        .call("getMe", json!({}))
        .await
        .context("connecting to Telegram; check the bot token")?;
    let username = me
        .get("username")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    eprintln!(
        "aster remote: connected as @{username}, repo {}, mode {}",
        cfg.repo_root.display(),
        cfg.mode
    );
    if cfg.allowed_users.is_empty() {
        eprintln!(
            "aster remote: no users allowed yet; message the bot once and restart with --user <your id>"
        );
    }
    api.register_commands().await;

    let cfg = Arc::new(cfg);
    let chats: Chats = Arc::new(Mutex::new(HashMap::new()));
    let mut offset = 0i64;
    loop {
        let updates = match api.get_updates(offset).await {
            Ok(updates) => updates,
            Err(e) => {
                tracing::warn!("getUpdates failed: {e:#}");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };
        for update in updates {
            if let Some(id) = update.get("update_id").and_then(Value::as_i64) {
                offset = offset.max(id + 1);
            }
            handle_update(&api, &cfg, &chats, &update).await;
        }
    }
}

async fn handle_update(api: &Api, cfg: &Arc<TelegramConfig>, chats: &Chats, update: &Value) {
    if let Some(message) = update.get("message") {
        handle_message(api, cfg, chats, message).await;
    } else if let Some(callback) = update.get("callback_query") {
        handle_callback(api, cfg, chats, callback).await;
    }
}

async fn handle_message(api: &Api, cfg: &Arc<TelegramConfig>, chats: &Chats, message: &Value) {
    let Some(chat_id) = message
        .get("chat")
        .and_then(|c| c.get("id"))
        .and_then(Value::as_i64)
    else {
        return;
    };
    let sender = message
        .get("from")
        .and_then(|f| f.get("id"))
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if !cfg.allowed_users.contains(&sender) {
        let text = format!(
            "Not authorized. Your Telegram user id is {sender}; restart the bridge with --user {sender} to allow it."
        );
        api.send_text(chat_id, &text).await;
        return;
    }
    let Some(text) = message.get("text").and_then(Value::as_str) else {
        api.send_text(chat_id, "Only text messages are supported for now.")
            .await;
        return;
    };
    let trimmed = text.trim();
    if let Some(command) = trimmed.strip_prefix('/') {
        let (name, arg) = match command.split_once(char::is_whitespace) {
            Some((name, arg)) => (name, arg.trim()),
            None => (command, ""),
        };
        handle_command(api, cfg, chats, chat_id, name, arg).await;
        return;
    }
    let message_id = message
        .get("message_id")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    start_turn(api, cfg, chats, chat_id, message_id, trimmed).await;
}

/// One /command, mirroring the TUI's command set where it makes sense remotely.
async fn handle_command(
    api: &Api,
    cfg: &Arc<TelegramConfig>,
    chats: &Chats,
    chat_id: i64,
    name: &str,
    arg: &str,
) {
    match name {
        "start" | "help" => api.send_html_or_plain(chat_id, &help(cfg)).await,
        "new" | "clear" => {
            {
                let mut chats = chats.lock().expect("chats lock");
                chats.entry(chat_id).or_default().history.clear();
            }
            api.send_text(chat_id, "Started a fresh conversation.")
                .await;
        }
        "stop" => {
            let running = {
                let mut chats = chats.lock().expect("chats lock");
                chats.entry(chat_id).or_default().running.take()
            };
            match running {
                Some(handle) => {
                    handle.abort();
                    api.send_text(chat_id, "Stopped the current turn.").await;
                }
                None => api.send_text(chat_id, "Nothing is running.").await,
            }
        }
        "mode" => match set_override(chats, chat_id, arg, MODES, |state| &mut state.mode) {
            Some(reply) => api.send_text(chat_id, &reply).await,
            None => {
                let current = get_override(chats, chat_id, |state| state.mode.clone())
                    .unwrap_or_else(|| cfg.mode.clone());
                let keyboard = choice_keyboard("m", MODES, &current);
                api.send_keyboard(chat_id, "<b>Mode</b> — how the agent acts", keyboard)
                    .await;
            }
        },
        "effort" => match set_override(chats, chat_id, arg, EFFORTS, |state| &mut state.effort) {
            Some(reply) => api.send_text(chat_id, &reply).await,
            None => {
                let current = get_override(chats, chat_id, |state| state.effort.clone())
                    .unwrap_or_else(|| "default".into());
                let keyboard = choice_keyboard("e", EFFORTS, &current);
                api.send_keyboard(chat_id, "<b>Effort</b> — reasoning budget", keyboard)
                    .await;
            }
        },
        "model" => {
            if arg.is_empty() {
                let current = get_override(chats, chat_id, |state| state.model.clone())
                    .unwrap_or_else(|| "configured default".into());
                api.send_text(
                    chat_id,
                    &format!(
                        "Model: {current}. Set with /model <name>, reset with /model default."
                    ),
                )
                .await;
            } else {
                let value = (arg != "default").then(|| arg.to_string());
                let note = match &value {
                    Some(model) => format!("Model set to {model}."),
                    None => "Model reset to the configured default.".into(),
                };
                {
                    let mut chats = chats.lock().expect("chats lock");
                    chats.entry(chat_id).or_default().model = value;
                }
                api.send_text(chat_id, &note).await;
            }
        }
        "status" => {
            let (mode, model, effort, turns, busy) = {
                let mut chats = chats.lock().expect("chats lock");
                let state = chats.entry(chat_id).or_default();
                (
                    state.mode.clone().unwrap_or_else(|| cfg.mode.clone()),
                    state.model.clone().unwrap_or_else(|| "default".into()),
                    state.effort.clone().unwrap_or_else(|| "default".into()),
                    state.history.len(),
                    state.running.is_some(),
                )
            };
            let text = format!(
                "Repo: {}\nSession: telegram-{chat_id}\nMode: {mode}\nModel: {model}\nEffort: {effort}\nHistory: {turns} messages\nState: {}",
                cfg.repo_root.display(),
                if busy { "working" } else { "idle" },
            );
            api.send_text(chat_id, &text).await;
        }
        "diff" => {
            let output = tokio::process::Command::new("git")
                .args(["-C", &cfg.repo_root.display().to_string(), "diff", "--stat"])
                .output()
                .await;
            let text = match output {
                Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
                Err(e) => format!("git diff failed: {e}"),
            };
            if text.is_empty() {
                api.send_text(chat_id, "No uncommitted changes.").await;
            } else {
                let html = format!("<pre>{}</pre>", markdown::escape(&truncate(&text, 3500)));
                api.send_html_or_plain(chat_id, &html).await;
            }
        }
        _ => {
            api.send_text(chat_id, "Unknown command; /help lists what I know.")
                .await;
        }
    }
}

const MODES: &[&str] = &["plan", "manual", "auto", "edit", "yolo"];
const EFFORTS: &[&str] = &["off", "low", "medium", "high"];

/// One button per option, the current one marked with a dot.
fn choice_keyboard(prefix: &str, options: &[&str], current: &str) -> Value {
    let buttons: Vec<Value> = options
        .iter()
        .map(|opt| {
            let label = if *opt == current {
                format!("• {opt}")
            } else {
                (*opt).to_string()
            };
            json!({"text": label, "callback_data": format!("{prefix}:{opt}")})
        })
        .collect();
    // Rows of three keep the keyboard compact on phones.
    Value::Array(buttons.chunks(3).map(|row| json!(row)).collect())
}

/// Validate and store a /mode-style override; `None` means "show usage".
fn set_override(
    chats: &Chats,
    chat_id: i64,
    arg: &str,
    allowed: &[&str],
    slot: impl FnOnce(&mut ChatState) -> &mut Option<String>,
) -> Option<String> {
    if arg.is_empty() {
        return None;
    }
    if !allowed.contains(&arg) {
        return Some(format!("Expected one of: {}.", allowed.join(", ")));
    }
    let mut chats = chats.lock().expect("chats lock");
    *slot(chats.entry(chat_id).or_default()) = Some(arg.to_string());
    Some(format!("Set to {arg}."))
}

fn get_override<T>(chats: &Chats, chat_id: i64, read: impl FnOnce(&ChatState) -> T) -> T {
    let mut chats = chats.lock().expect("chats lock");
    read(chats.entry(chat_id).or_default())
}

/// Kick off one agent turn for this chat unless one is already running.
async fn start_turn(
    api: &Api,
    cfg: &Arc<TelegramConfig>,
    chats: &Chats,
    chat_id: i64,
    message_id: i64,
    prompt: &str,
) {
    let prepared = {
        let mut chats = chats.lock().expect("chats lock");
        let state = chats.entry(chat_id).or_default();
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
    };
    let Some((history, mode, model, effort)) = prepared else {
        api.send_text(
            chat_id,
            "Still working on the previous message; /stop cancels it.",
        )
        .await;
        return;
    };

    // The chat context rides in as env so the `telegram` MCP server the child
    // spawns can act on this conversation.
    let mcp_extra = json!({
        "telegram": {
            "command": cfg.bin.display().to_string(),
            "args": ["remote", "mcp-telegram"],
        }
    });
    let turn = Turn {
        bin: cfg.bin.clone(),
        repo_root: cfg.repo_root.clone(),
        session: format!("telegram-{chat_id}"),
        mode: mode.unwrap_or_else(|| cfg.mode.clone()),
        model,
        effort,
        extra_env: vec![
            ("TELEGRAM_CHAT_ID".into(), chat_id.to_string()),
            ("TELEGRAM_MESSAGE_ID".into(), message_id.to_string()),
            ("ASTER_MCP_EXTRA".into(), mcp_extra.to_string()),
        ],
    };
    // Prepended per turn rather than stored, so /new never loses it and the
    // recorded session history stays pure conversation.
    let mut wire = Vec::with_capacity(history.len() + 1);
    wire.push(WireMessage {
        role: "system".into(),
        content: TELEGRAM_SYSTEM.into(),
    });
    wire.extend(history);

    let (events_tx, events_rx) = mpsc::channel::<TurnEvent>(8);
    let turn_task = tokio::spawn(async move { run_turn(&turn, &wire, &events_tx).await });
    {
        let mut chats = chats.lock().expect("chats lock");
        chats.entry(chat_id).or_default().running = Some(turn_task.abort_handle());
    }

    let api = api.clone();
    let chats = chats.clone();
    tokio::spawn(async move {
        let result = drive_turn(&api, &chats, chat_id, events_rx, turn_task).await;
        finish_turn(&api, &chats, chat_id, result).await;
    });
}

/// Pump turn events into Telegram while the turn runs, then return its result.
async fn drive_turn(
    api: &Api,
    chats: &Chats,
    chat_id: i64,
    mut events: mpsc::Receiver<TurnEvent>,
    turn_task: tokio::task::JoinHandle<Result<TurnOutcome>>,
) -> Result<TurnOutcome> {
    // Telegram's typing status fades after ~5s, so keep it alive for the
    // whole turn instead of pinging it per tool call.
    let typing = tokio::spawn({
        let api = api.clone();
        async move {
            loop {
                api.send_typing(chat_id).await;
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    });

    let mut activity = Activity::new(api.clone(), chat_id);
    while let Some(event) = events.recv().await {
        match event {
            TurnEvent::ToolCall { name, arguments } => {
                activity.push(tool_line(&name, &arguments));
                activity.flush(false).await;
            }
            TurnEvent::ApprovalRequest {
                preview,
                scope,
                respond,
            } => {
                activity.flush(true).await;
                let mut text = format!(
                    "<b>Approval needed</b>\n<pre>{}</pre>",
                    markdown::escape(&truncate(&preview, 3000))
                );
                if let Some(scope) = scope {
                    text.push_str(&format!("\n<code>{}</code>", markdown::escape(&scope)));
                }
                let keyboard = json!([[
                    {"text": "Allow", "callback_data": "a:allow"},
                    {"text": "Always", "callback_data": "a:always"},
                    {"text": "Deny", "callback_data": "a:deny"},
                ]]);
                api.send_keyboard(chat_id, &text, keyboard).await;
                set_pending(chats, chat_id, Pending::Approval(respond));
            }
            TurnEvent::Question {
                header,
                question,
                options,
                respond,
            } => {
                activity.flush(true).await;
                let text = format!(
                    "<b>{}</b>\n{}",
                    markdown::escape(&header),
                    markdown::escape(&question)
                );
                let buttons: Vec<Value> = options
                    .iter()
                    .enumerate()
                    .map(|(i, opt)| json!([{"text": opt, "callback_data": format!("q:{i}")}]))
                    .chain(std::iter::once(
                        json!([{"text": "Skip", "callback_data": "q:skip"}]),
                    ))
                    .collect();
                api.send_keyboard(chat_id, &text, Value::Array(buttons))
                    .await;
                set_pending(chats, chat_id, Pending::Question { options, respond });
            }
        }
    }
    let result = turn_task
        .await
        .unwrap_or_else(|e| Err(anyhow!("turn cancelled: {e}")));
    typing.abort();
    activity.finish(result.is_ok()).await;
    result
}

/// Record the outcome and report it back to the chat.
async fn finish_turn(api: &Api, chats: &Chats, chat_id: i64, result: Result<TurnOutcome>) {
    let result = {
        let mut chats = chats.lock().expect("chats lock");
        let state = chats.entry(chat_id).or_default();
        state.running = None;
        state.pending = None;
        if let Ok(outcome) = &result {
            state
                .history
                .push(WireMessage::assistant(outcome.reply.clone()));
        }
        result
    };
    match result {
        Ok(outcome) => {
            let (text, gifs) = extract_gifs(&outcome.reply);
            for chunk in markdown::to_html_chunks(&text, CHUNK_LIMIT) {
                api.send_html_or_plain(chat_id, &chunk).await;
            }
            for gif in gifs {
                api.send_animation(chat_id, &gif).await;
            }
            if !outcome.edits.is_empty() {
                let files: Vec<String> = outcome
                    .edits
                    .iter()
                    .map(|path| format!("•  <code>{}</code>", markdown::escape(path)))
                    .collect();
                let text = format!("<b>Edited files</b>\n{}", files.join("\n"));
                api.send_html_or_plain(chat_id, &text).await;
            }
        }
        Err(e) => {
            api.send_text(chat_id, &format!("Turn failed: {e:#}")).await;
        }
    }
}

fn set_pending(chats: &Chats, chat_id: i64, pending: Pending) {
    let mut chats = chats.lock().expect("chats lock");
    chats.entry(chat_id).or_default().pending = Some(pending);
}

async fn handle_callback(api: &Api, cfg: &Arc<TelegramConfig>, chats: &Chats, callback: &Value) {
    let callback_id = callback
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let sender = callback
        .get("from")
        .and_then(|f| f.get("id"))
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if !cfg.allowed_users.contains(&sender) {
        api.answer_callback(callback_id, "🚫 Not authorized.").await;
        return;
    }
    let Some(chat_id) = callback
        .get("message")
        .and_then(|m| m.get("chat"))
        .and_then(|c| c.get("id"))
        .and_then(Value::as_i64)
    else {
        return;
    };
    let data = callback
        .get("data")
        .and_then(Value::as_str)
        .unwrap_or_default();

    // Settings buttons are stateless and must not consume a pending prompt.
    if let Some(choice) = data.strip_prefix("m:").filter(|c| MODES.contains(c)) {
        {
            let mut chats = chats.lock().expect("chats lock");
            chats.entry(chat_id).or_default().mode = Some(choice.to_string());
        }
        api.answer_callback(callback_id, &format!("Mode set to {choice}."))
            .await;
        api.remove_keyboard(callback).await;
        return;
    }
    if let Some(choice) = data.strip_prefix("e:").filter(|c| EFFORTS.contains(c)) {
        {
            let mut chats = chats.lock().expect("chats lock");
            chats.entry(chat_id).or_default().effort = Some(choice.to_string());
        }
        api.answer_callback(callback_id, &format!("Effort set to {choice}."))
            .await;
        api.remove_keyboard(callback).await;
        return;
    }

    let pending = {
        let mut chats = chats.lock().expect("chats lock");
        chats.entry(chat_id).or_default().pending.take()
    };
    let ack = match (pending, data) {
        (Some(Pending::Approval(respond)), "a:allow") => {
            let _ = respond.send(Answer::Allow);
            "Allowed"
        }
        (Some(Pending::Approval(respond)), "a:always") => {
            let _ = respond.send(Answer::AlwaysAllow);
            "Always allowed"
        }
        (Some(Pending::Approval(respond)), "a:deny") => {
            let _ = respond.send(Answer::Deny);
            "Denied"
        }
        (Some(Pending::Question { respond, .. }), "q:skip") => {
            let _ = respond.send(None);
            "Skipped"
        }
        (Some(Pending::Question { options, respond }), choice) => {
            let picked = choice
                .strip_prefix("q:")
                .and_then(|i| i.parse::<usize>().ok())
                .and_then(|i| options.get(i).cloned());
            let _ = respond.send(picked);
            "Answered"
        }
        (None, _) => "This prompt already expired.",
        (Some(pending), _) => {
            // Unrecognized data: put the prompt back rather than dropping it.
            set_pending(chats, chat_id, pending);
            "Unknown action."
        }
    };
    api.answer_callback(callback_id, ack).await;
    api.remove_keyboard(callback).await;
}

/// The live "what the agent is doing" message, edited in place as tools run.
struct Activity {
    api: Api,
    chat_id: i64,
    message_id: Option<i64>,
    lines: Vec<String>,
    last_flush: Instant,
}

impl Activity {
    fn new(api: Api, chat_id: i64) -> Self {
        Self {
            api,
            chat_id,
            message_id: None,
            lines: Vec::new(),
            last_flush: Instant::now()
                .checked_sub(ACTIVITY_EDIT_GAP)
                .unwrap_or_else(Instant::now),
        }
    }

    fn push(&mut self, line: String) {
        self.lines.push(line);
    }

    fn render(&self, header: &str) -> String {
        let mut text = String::from(header);
        let hidden = self.lines.len().saturating_sub(ACTIVITY_WINDOW);
        if hidden > 0 {
            text.push_str(&format!("\n…  {hidden} earlier steps"));
        }
        let visible = &self.lines[hidden..];
        let mut i = 0;
        while i < visible.len() {
            let mut run = 1;
            while i + run < visible.len() && visible[i + run] == visible[i] {
                run += 1;
            }
            text.push('\n');
            text.push_str(&visible[i]);
            if run > 1 {
                text.push_str(&format!(" ×{run}"));
            }
            i += run;
        }
        text
    }
    /// Send or edit the activity message; unforced calls are rate-limited.
    async fn flush(&mut self, force: bool) {
        if self.lines.is_empty() {
            return;
        }
        if !force && self.message_id.is_some() && self.last_flush.elapsed() < ACTIVITY_EDIT_GAP {
            return;
        }
        let text = self.render("<b>Working…</b>");
        match self.message_id {
            None => self.message_id = self.api.send_html(self.chat_id, &text).await,
            Some(id) => self.api.edit_html(self.chat_id, id, &text).await,
        }
        self.last_flush = Instant::now();
    }

    /// Settle the message on its final state once the turn ends.
    async fn finish(&mut self, ok: bool) {
        if self.lines.is_empty() {
            return;
        }
        let header = if ok {
            format!("<b>Done</b> · {} steps", self.lines.len())
        } else {
            format!("<b>Stopped</b> · {} steps", self.lines.len())
        };
        let text = self.render(&header);
        match self.message_id {
            None => self.message_id = self.api.send_html(self.chat_id, &text).await,
            Some(id) => self.api.edit_html(self.chat_id, id, &text).await,
        }
    }
}

/// One activity line for a tool call: emoji, verb, and the interesting argument.
fn tool_line(name: &str, arguments: &str) -> String {
    let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let field = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| args.get(k).and_then(Value::as_str))
            .unwrap_or("")
            .to_string()
    };
    let code = |text: &str| format!("<code>{}</code>", markdown::escape(&truncate(text, 80)));
    match name {
        "read_file" => format!("📖 read {}", code(&field(&["path"]))),
        "list_files" => format!("📂 list {}", code(&field(&["path"]))),
        "search_files" => format!("🔎 search {}", code(&field(&["query", "pattern", "regex"]))),
        "find_files" => format!("🗂 find {}", code(&field(&["pattern", "glob", "query"]))),
        "run_command" => {
            let mut cmd = field(&["command"]);
            if let Some(args) = args.get("args").and_then(Value::as_array) {
                let extra: Vec<&str> = args.iter().filter_map(Value::as_str).collect();
                if !extra.is_empty() {
                    cmd.push(' ');
                    cmd.push_str(&extra.join(" "));
                }
            }
            format!("🖥 {}", code(&cmd))
        }
        "run_tests" => "🧪 running tests".into(),
        "aster_mcp" => format!("🔌 {}", code(&field(&["id", "tool", "name", "query"]))),
        "edit_file" => format!("✍️ editing {}", code(&field(&["path"]))),
        "remember" => format!("🧠 remember {}", code(&field(&["name"]))),
        "recall" => format!("🧠 recall {}", code(&field(&["name"]))),
        "read_skill" => format!("📚 skill {}", code(&field(&["name"]))),
        "update_plan" => "📋 updating the plan".into(),
        "exit_plan_mode" => "📋 plan ready".into(),
        "ask_user" => "💬 asking you".into(),
        "giphy/search_gifs" => format!("🎞 searching gifs for {}", code(&field(&["query"]))),
        "giphy/get_random_gif" => "🎲 picking a random gif".into(),
        "giphy/get_trending_gifs" => "📈 checking trending gifs".into(),
        "telegram/react" => "😄 reacting".into(),
        "telegram/send_gif" => "🎞 sending a gif".into(),
        "telegram/send_photo" => "🖼 sending a photo".into(),
        "telegram/send_document" => "📎 sending a file".into(),
        "telegram/send_code_page" => "📄 publishing a code page".into(),
        "telegram/send_poll" => "📊 asking a poll".into(),
        other => format!("⚙️ {}", markdown::escape(&humanize_tool_name(other))),
    }
}

/// Turn a raw tool id like `giphy/get_trending_gifs` into `giphy: get trending gifs`.
fn humanize_tool_name(name: &str) -> String {
    match name.split_once('/') {
        Some((server, tool)) => format!("{}: {}", server, tool.replace('_', " ")),
        None => name.replace('_', " "),
    }
}

/// Reaction emoji Telegram accepts, offered to the agent via the react tool.
pub(crate) const REACTIONS: &[&str] = &[
    "👍",
    "👎",
    "❤",
    "🔥",
    "🎉",
    "🤔",
    "😁",
    "😢",
    "🙏",
    "👏",
    "💯",
    "⚡",
    "👀",
    "🤝",
    "🫡",
    "🤯",
    "😱",
    "🤩",
    "🕊",
    "👨‍💻",
];

/// How many gifs one reply may attach.
const GIF_LIMIT: usize = 3;

/// Pull gif URLs out of a reply so they can render as animations.
/// Lines that are only a gif URL (bare or markdown image) leave the text;
/// inline mentions stay in place but still attach.
fn extract_gifs(reply: &str) -> (String, Vec<String>) {
    let mut gifs: Vec<String> = Vec::new();
    let mut kept = Vec::new();
    for line in reply.lines() {
        let trimmed = line.trim();
        let bare = trimmed
            .strip_prefix("![")
            .and_then(|rest| rest.split_once("]("))
            .map(|(_, url)| url.trim_end_matches(')').trim())
            .unwrap_or(trimmed);
        if is_gif_url(bare) {
            if gifs.len() < GIF_LIMIT && !gifs.iter().any(|g| g == bare) {
                gifs.push(bare.to_string());
            }
            continue;
        }
        for token in line.split_whitespace() {
            let token = token.trim_matches(|c: char| "()[]<>,.".contains(c));
            if is_gif_url(token) && gifs.len() < GIF_LIMIT && !gifs.iter().any(|g| g == token) {
                gifs.push(token.to_string());
            }
        }
        kept.push(line);
    }
    (kept.join("\n"), gifs)
}

fn is_gif_url(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://"))
        && (url.contains(".gif")
            || url.contains("giphy.com/media")
            || url.contains("media.tenor.com"))
}

/// Truncate on a char boundary, marking the cut with an ellipsis.
fn truncate(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut cut = limit;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &text[..cut])
}

/// Unwrap the Bot API's `{ok, result, description}` envelope.
fn unwrap_result(method: &str, response: Value) -> Result<Value> {
    if !response.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        let description = response
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        anyhow::bail!("telegram {method}: {description}");
    }
    Ok(response.get("result").cloned().unwrap_or(Value::Null))
}

fn help(cfg: &TelegramConfig) -> String {
    format!(
        "<b>Aster remote control</b>\n\
         Send a message to run the agent on <code>{}</code>.\n\
         Approvals arrive as buttons; activity streams live.\n\n\
         /new — start a fresh conversation\n\
         /clear — same as /new\n\
         /stop — cancel the running turn\n\
         /mode — how the agent acts (plan, manual, auto, edit, yolo)\n\
         /model — switch the model for this chat\n\
         /effort — reasoning budget (off, low, medium, high)\n\
         /status — session, mode, model, and history\n\
         /diff — uncommitted changes in the repo\n\
         /help — this message",
        markdown::escape(
            &cfg.repo_root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| cfg.repo_root.display().to_string())
        )
    )
}

/// Minimal Telegram Bot API client over HTTPS.
#[derive(Clone)]
pub(crate) struct Api {
    http: reqwest::Client,
    base: String,
}

impl Api {
    pub(crate) fn new(token: &str) -> Result<Self> {
        // Long polls hold the connection ~50s, so the client timeout sits above.
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(70))
            .build()?;
        Ok(Self {
            http,
            base: format!("https://api.telegram.org/bot{token}"),
        })
    }

    pub(crate) async fn call(&self, method: &str, payload: Value) -> Result<Value> {
        let response: Value = self
            .http
            .post(format!("{}/{method}", self.base))
            .json(&payload)
            .send()
            .await?
            .json()
            .await?;
        unwrap_result(method, response)
    }

    /// Upload a local file as a document; URLs go through `call` instead.
    pub(crate) async fn send_document_file(
        &self,
        chat_id: i64,
        path: &str,
        caption: Option<&str>,
    ) -> Result<Value> {
        let bytes = tokio::fs::read(path)
            .await
            .with_context(|| format!("reading {path}"))?;
        anyhow::ensure!(
            bytes.len() <= 50 * 1024 * 1024,
            "{path} is over Telegram's 50 MB bot upload limit"
        );
        let filename = std::path::Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into());
        let mut form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part(
                "document",
                reqwest::multipart::Part::bytes(bytes).file_name(filename),
            );
        if let Some(caption) = caption {
            form = form.text("caption", caption.to_string());
        }
        let response: Value = self
            .http
            .post(format!("{}/sendDocument", self.base))
            .multipart(form)
            .send()
            .await?
            .json()
            .await?;
        unwrap_result("sendDocument", response)
    }

    /// Show /new, /stop, and /help in Telegram's command menu.
    async fn register_commands(&self) {
        let payload = json!({
            "commands": [
                {"command": "new", "description": "Start a fresh conversation"},
                {"command": "clear", "description": "Start a fresh conversation"},
                {"command": "stop", "description": "Cancel the running turn"},
                {"command": "mode", "description": "How the agent acts (plan/manual/auto/edit/yolo)"},
                {"command": "model", "description": "Switch the model for this chat"},
                {"command": "effort", "description": "Reasoning budget (off/low/medium/high)"},
                {"command": "status", "description": "Session, mode, model, and history"},
                {"command": "diff", "description": "Uncommitted changes in the repo"},
                {"command": "help", "description": "How this bot works"},
            ]
        });
        if let Err(e) = self.call("setMyCommands", payload).await {
            tracing::warn!("setMyCommands failed: {e:#}");
        }
    }

    async fn get_updates(&self, offset: i64) -> Result<Vec<Value>> {
        let result = self
            .call(
                "getUpdates",
                json!({
                    "offset": offset,
                    "timeout": 50,
                    "allowed_updates": ["message", "callback_query"],
                }),
            )
            .await?;
        Ok(result.as_array().cloned().unwrap_or_default())
    }

    async fn send_text(&self, chat_id: i64, text: &str) {
        let payload = json!({ "chat_id": chat_id, "text": text });
        if let Err(e) = self.call("sendMessage", payload).await {
            tracing::warn!("sendMessage failed: {e:#}");
        }
    }

    /// Send HTML and return the new message id, or `None` on failure.
    async fn send_html(&self, chat_id: i64, html: &str) -> Option<i64> {
        let payload = json!({
            "chat_id": chat_id,
            "text": html,
            "parse_mode": "HTML",
            "link_preview_options": { "is_disabled": true },
        });
        match self.call("sendMessage", payload).await {
            Ok(message) => message.get("message_id").and_then(Value::as_i64),
            Err(e) => {
                tracing::warn!("sendMessage (html) failed: {e:#}");
                None
            }
        }
    }

    /// Send HTML, falling back to plain text if Telegram rejects the markup.
    async fn send_html_or_plain(&self, chat_id: i64, html: &str) {
        if self.send_html(chat_id, html).await.is_none() {
            self.send_text(chat_id, html).await;
        }
    }

    async fn edit_html(&self, chat_id: i64, message_id: i64, html: &str) {
        let payload = json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": html,
            "parse_mode": "HTML",
        });
        if let Err(e) = self.call("editMessageText", payload).await {
            tracing::debug!("editMessageText failed: {e:#}");
        }
    }

    async fn send_keyboard(&self, chat_id: i64, html: &str, keyboard: Value) {
        let payload = json!({
            "chat_id": chat_id,
            "text": html,
            "parse_mode": "HTML",
            "reply_markup": { "inline_keyboard": keyboard },
        });
        if let Err(e) = self.call("sendMessage", payload).await {
            tracing::warn!("sendMessage failed: {e:#}");
        }
    }

    /// Render a gif by URL; Telegram fetches and plays it inline.
    async fn send_animation(&self, chat_id: i64, url: &str) {
        let payload = json!({ "chat_id": chat_id, "animation": url });
        if let Err(e) = self.call("sendAnimation", payload).await {
            tracing::warn!("sendAnimation failed: {e:#}");
            self.send_text(chat_id, url).await;
        }
    }

    async fn send_typing(&self, chat_id: i64) {
        let payload = json!({ "chat_id": chat_id, "action": "typing" });
        let _ = self.call("sendChatAction", payload).await;
    }

    async fn answer_callback(&self, callback_id: &str, text: &str) {
        let payload = json!({ "callback_query_id": callback_id, "text": text });
        if let Err(e) = self.call("answerCallbackQuery", payload).await {
            tracing::warn!("answerCallbackQuery failed: {e:#}");
        }
    }

    /// Best-effort removal of the tapped prompt's buttons.
    async fn remove_keyboard(&self, callback: &Value) {
        let Some(message) = callback.get("message") else {
            return;
        };
        let (Some(chat_id), Some(message_id)) = (
            message
                .get("chat")
                .and_then(|c| c.get("id"))
                .and_then(Value::as_i64),
            message.get("message_id").and_then(Value::as_i64),
        ) else {
            return;
        };
        let payload = json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "reply_markup": { "inline_keyboard": [] },
        });
        let _ = self.call("editMessageReplyMarkup", payload).await;
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_gifs, tool_line, truncate};

    #[test]
    fn extract_gifs_removes_bare_url_lines() {
        let reply = "Here you go!\nhttps://media.giphy.com/media/abc/giphy.gif";
        let (text, gifs) = extract_gifs(reply);
        assert_eq!(text, "Here you go!");
        assert_eq!(gifs, vec!["https://media.giphy.com/media/abc/giphy.gif"]);
    }

    #[test]
    fn extract_gifs_unwraps_markdown_images() {
        let reply = "![party](https://media.tenor.com/xyz/party.gif)";
        let (text, gifs) = extract_gifs(reply);
        assert!(text.is_empty());
        assert_eq!(gifs, vec!["https://media.tenor.com/xyz/party.gif"]);
    }

    #[test]
    fn extract_gifs_keeps_inline_mentions_in_text() {
        let reply = "see https://x.com/a.gif for the vibe";
        let (text, gifs) = extract_gifs(reply);
        assert_eq!(text, reply);
        assert_eq!(gifs, vec!["https://x.com/a.gif"]);
    }

    #[test]
    fn extract_gifs_ignores_plain_replies() {
        let (text, gifs) = extract_gifs("no media here, just https://docs.rs");
        assert_eq!(text, "no media here, just https://docs.rs");
        assert!(gifs.is_empty());
    }

    #[test]
    fn tool_line_labels_known_tools() {
        let line = tool_line("read_file", r#"{"path":"src/main.rs"}"#);
        assert_eq!(line, "📖 read <code>src/main.rs</code>");
    }

    #[test]
    fn tool_line_truncates_long_commands() {
        let command = format!(r#"{{"command":"{}"}}"#, "x".repeat(200));
        let line = tool_line("run_command", &command);
        assert!(line.len() < 140);
        assert!(line.contains('…'));
    }

    #[test]
    fn tool_line_escapes_html_in_arguments() {
        let line = tool_line("read_file", r#"{"path":"a<b>.rs"}"#);
        assert!(line.contains("a&lt;b&gt;.rs"));
    }

    #[test]
    fn tool_line_falls_back_to_name() {
        assert_eq!(tool_line("mystery", "{}"), "🔧 mystery");
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        let text = "é".repeat(50);
        let cut = truncate(&text, 41);
        assert!(cut.ends_with('…'));
        assert!(cut.len() <= 44);
    }
}
