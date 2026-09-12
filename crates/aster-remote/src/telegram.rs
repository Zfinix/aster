//! Telegram adapter: long-polls the Bot API, runs one agent turn per incoming
//! message, and relays approval prompts as inline keyboards. Tool calls stream
//! into a live-edited activity message so the chat mirrors the CLI.

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, anyhow};
use aster_ai::AiClient;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;
use ulid::Ulid;

use crate::bridge::{Agent, Answer, Turn, TurnEvent, TurnOutcome, WireMessage};
use crate::markdown;

const CHUNK_LIMIT: usize = 4000;

const ACTIVITY_WINDOW: usize = 6;

const ACTIVITY_EDIT_GAP: Duration = Duration::from_millis(1500);

/// Staged Telegram photos outlive their turn so a re-read stays possible,
/// then go after a day.
const PHOTO_TTL: Duration = Duration::from_secs(24 * 60 * 60);

const PHOTO_SWEEP: Duration = Duration::from_secs(60 * 60);

const ASIDE_NOTE: &str = "The user sent this from the same chat while you are working, so it is \
     part of the task in hand, not a new one. Take it in at this point in the work: if it changes \
     what you should do, say so in a line and adjust; if it is only context, say you have it and \
     carry on. Do not start over and do not answer it as a separate job.";

const SKIP_LABEL: &str = "Skip";

pub struct TelegramConfig {
    pub token: String,
    pub allowed_users: Vec<i64>,
    pub bin: PathBuf,
    pub repo_root: PathBuf,
    pub mode: String,
}

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
Each message you get starts with a tag like `[msg 42]`, or `[msg 42, replying \
to msg 40 from you: \"...\"]` when the user quoted a message; that quote is the \
thing they mean by \"this\" or \"that\". The `telegram` MCP server gives you chat \
tools: react (an emoji on a message, the current one unless you pass a \
message_id; use sparingly), reply (send text quoting a specific message, for \
answering one thing out of several or a message from earlier), send_gif, \
send_photo, send_document (share a repo file), send_poll, and send_code_page \
(send long code or reports as a private, deletable chat attachment instead of \
flooding the chat). Prefer them over describing what you would send. \
Hard rule: any code, file contents, or report longer than 40 lines must go \
through send_code_page and be sent as a private attachment in this chat, never \
published to a public page and never pasted into the chat. \
Under 40 lines, paste inline.";

enum Pending {
    Approval {
        subject: String,
        respond: oneshot::Sender<Answer>,
    },
    Question(oneshot::Sender<Option<String>>),
}

#[derive(Default)]
struct ChatState {
    /// When this chat last spoke, so a wake-up knows where to land.
    touched: Option<Instant>,
    history: Vec<WireMessage>,
    pending: Option<Pending>,
    running: Option<AbortHandle>,
    agent: Option<Arc<Agent>>,
    session: Option<String>,
    stopped: bool,
    /// Messages that arrived too late to join the running turn; they run as
    /// one turn as soon as it ends.
    queued: Vec<(i64, String)>,
    /// A learn pass is running for this chat; one at a time is plenty.
    learning: bool,
    mode: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    loaded: bool,
    pending_commit: Option<PendingCommit>,
}

struct PendingCommit {
    message: String,
    stage_all: bool,
}

fn chat_state<T>(chats: &Chats, chat_id: i64, act: impl FnOnce(&mut ChatState) -> T) -> T {
    let mut chats = chats.lock().expect("chats lock");
    let state = chats.entry(chat_id).or_default();
    if !state.loaded {
        let (mode, model, effort, session) = load_settings(chat_id);
        state.mode = mode;
        state.model = model;
        state.effort = effort;
        state.session = session;
        state.loaded = true;
    }
    act(state)
}

type Chats = Arc<Mutex<HashMap<i64, ChatState>>>;

fn settings_path(chat_id: i64) -> Option<PathBuf> {
    let home = env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".aster/remote")
            .join(format!("telegram-{chat_id}.json")),
    )
}

type Saved = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Mode, model, effort, and the agent session id, so a restarted bridge picks
/// the conversation back up.
fn load_settings(chat_id: i64) -> Saved {
    let Some(path) = settings_path(chat_id) else {
        return (None, None, None, None);
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return (None, None, None, None);
    };
    let Ok(saved) = serde_json::from_str::<Value>(&raw) else {
        return (None, None, None, None);
    };
    let field = |key: &str| saved.get(key).and_then(Value::as_str).map(str::to_string);
    (
        field("mode"),
        field("model"),
        field("effort"),
        field("session"),
    )
}

fn save_settings(chats: &Chats, chat_id: i64) {
    let Some(path) = settings_path(chat_id) else {
        return;
    };
    let saved = {
        let mut chats = chats.lock().expect("chats lock");
        let state = chats.entry(chat_id).or_default();
        json!({ "mode": state.mode, "model": state.model, "effort": state.effort, "session": state.session })
    };
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::warn!("could not create {}: {e}", parent.display());
        return;
    }
    if let Err(e) = std::fs::write(&path, saved.to_string()) {
        tracing::warn!("could not save chat settings: {e}");
    }
}

struct SkillCommand {
    name: String,
    description: String,
}

type Skills = Arc<HashMap<String, SkillCommand>>;

fn discover_skill_commands(repo_root: &std::path::Path) -> HashMap<String, SkillCommand> {
    let mut roots = vec![repo_root.join(".aster").join("skills")];
    if let Ok(home) = env::var("HOME") {
        roots.push(PathBuf::from(home).join(".aster").join("skills"));
    }
    let mut commands = HashMap::new();
    for skill in aster_skills::SkillSet::discover(&roots).iter() {
        let command: String = skill
            .name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .take(32)
            .collect();
        if command.is_empty() {
            continue;
        }
        commands.entry(command).or_insert_with(|| SkillCommand {
            name: skill.name.clone(),
            description: skill.description.clone(),
        });
    }
    commands
}

async fn model_catalog() -> Result<&'static Vec<String>> {
    static MODEL_CACHE: OnceLock<Vec<String>> = OnceLock::new();
    if let Some(models) = MODEL_CACHE.get() {
        return Ok(models);
    }
    let base = env::var("ASTER_BASE_URL").unwrap_or_else(|_| aster_ai::DEFAULT_BASE_URL.into());
    let key = aster_ai::keys::resolve_key(&base)
        .map(|(key, _)| key)
        .unwrap_or_default();
    // Through the client, so endpoints that list models their own way, such as
    // Workers AI, answer here too.
    let models = AiClient::new(base, key, String::new())
        .fetch_models()
        .await?;
    Ok(MODEL_CACHE.get_or_init(|| models))
}

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
    let skills: Skills = Arc::new(discover_skill_commands(&cfg.repo_root));
    api.register_commands(&skills).await;

    let cfg = Arc::new(cfg);
    let chats: Chats = Arc::new(Mutex::new(HashMap::new()));
    tokio::spawn(watch_wakeups(
        api.clone(),
        Arc::clone(&cfg),
        Arc::clone(&chats),
    ));
    tokio::spawn(sweep_staged_photos());
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
            handle_update(&api, &cfg, &chats, &skills, &update).await;
        }
    }
}

/// `<data home>/aster/wakeups/<id>.json` with a `text` field, written by the
/// phone's alarm receiver; each file becomes one turn in the chat that last spoke.
fn wake_dir() -> Option<PathBuf> {
    let root = match std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".local/share"),
    };
    Some(root.join("aster").join("wakeups"))
}

async fn watch_wakeups(api: Api, cfg: Arc<TelegramConfig>, chats: Chats) {
    let Some(dir) = wake_dir() else {
        return;
    };
    loop {
        tokio::time::sleep(WAKE_POLL).await;
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        for path in files {
            let text = std::fs::read_to_string(&path)
                .ok()
                .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
                .and_then(|v| v["text"].as_str().map(str::to_string));
            let _ = std::fs::remove_file(&path);
            let Some(text) = text else {
                continue;
            };
            // Before anyone has spoken since the bridge came up, the owner's
            // private chat is the place: for Telegram its id is the user's.
            let Some(chat_id) = latest_chat(&chats).or(cfg.allowed_users.first().copied()) else {
                tracing::warn!("wake-up with no chat to land in: {text}");
                continue;
            };
            let prompt = format!(
                "⏰ The reminder you set has fired: {text}
                 Pick this up now and report back when it is done."
            );
            let posted = api
                .send_html(chat_id, &format!("⏰ <i>{}</i>", markdown::escape(&text)))
                .await
                .unwrap_or(0);
            start_turn(&api, &cfg, &chats, chat_id, posted, &prompt);
        }
    }
}

fn latest_chat(chats: &Chats) -> Option<i64> {
    let chats = chats.lock().ok()?;
    chats
        .iter()
        .filter_map(|(id, state)| state.touched.map(|t| (t, *id)))
        .max()
        .map(|(_, id)| id)
}

const WAKE_POLL: Duration = Duration::from_secs(2);

async fn handle_update(
    api: &Api,
    cfg: &Arc<TelegramConfig>,
    chats: &Chats,
    skills: &Skills,
    update: &Value,
) {
    if let Some(message) = update.get("message") {
        handle_message(api, cfg, chats, skills, message).await;
    } else if let Some(callback) = update.get("callback_query") {
        handle_callback(api, cfg, chats, skills, callback).await;
    }
}

async fn handle_message(
    api: &Api,
    cfg: &Arc<TelegramConfig>,
    chats: &Chats,
    skills: &Skills,
    message: &Value,
) {
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
            "This bot isn't set up for you yet. Your user id is {sender}. Restart it with --user {sender} to allow it."
        );
        api.send_text(chat_id, &text).await;
        return;
    }
    chat_state(chats, chat_id, |state| state.touched = Some(Instant::now()));
    let text = match message.get("text").and_then(Value::as_str) {
        Some(text) => text.trim().to_string(),
        None => match incoming_photo(api, message).await {
            Some(path) => photo_prompt(&path, message),
            None => {
                api.send_text(chat_id, "I can only read text and photos for now.")
                    .await;
                return;
            }
        },
    };
    let trimmed = text.trim();
    let message_id = message
        .get("message_id")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if let Some(command) = trimmed.strip_prefix('/') {
        let (name, arg) = match command.split_once(char::is_whitespace) {
            Some((name, arg)) => (name, arg.trim()),
            None => (command, ""),
        };
        handle_command(api, cfg, chats, skills, chat_id, message_id, name, arg).await;
        return;
    }
    // A pending agent question claims the next plain message as its answer.
    let question = {
        let mut chats = chats.lock().expect("chats lock");
        let state = chats.entry(chat_id).or_default();
        match state.pending.take() {
            Some(Pending::Question(respond)) => Some(respond),
            other => {
                state.pending = other;
                None
            }
        }
    };
    if let Some(respond) = question {
        let answer = (trimmed != SKIP_LABEL).then(|| trimmed.to_string());
        let _ = respond.send(answer);
        return;
    }
    let prompt = incoming_prompt(message, trimmed);
    start_turn(api, cfg, chats, chat_id, message_id, &prompt);
}

/// A photo message as a staged file path, so the prompt can mention it and the
/// agent's `@path` attachment picks it up. Telegram serves photos at several
/// sizes; the widest is the one worth reading.
async fn incoming_photo(api: &Api, message: &Value) -> Option<String> {
    let file_id = photo_file_id(message)?;
    let file = api.get_file(file_id).await.ok()?;
    let bytes = api.download(&file).await.ok()?;
    let path = std::env::temp_dir()
        .join("aster-pasted")
        .join(format!("telegram-{}.jpg", Ulid::new()));
    tokio::fs::create_dir_all(path.parent()?).await.ok()?;
    tokio::fs::write(&path, bytes).await.ok()?;
    Some(path.display().to_string())
}

/// Deletes staged Telegram photos older than a day, so a photo that was never
/// picked up by a turn does not sit in temp storage forever. Runs alongside the
/// wakeup watcher for the life of the bridge.
async fn sweep_staged_photos() {
    let dir = std::env::temp_dir().join("aster-pasted");
    loop {
        tokio::time::sleep(PHOTO_SWEEP).await;
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let cutoff = SystemTime::now() - PHOTO_TTL;
        for entry in entries.flatten() {
            let path = entry.path();
            let is_ours = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("telegram-"));
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

/// The largest variant's file id, the one worth reading.
fn photo_file_id(message: &Value) -> Option<&str> {
    message
        .get("photo")?
        .as_array()?
        .last()?
        .get("file_id")?
        .as_str()
}

/// The staged path as a prompt mention, with the caption as its text.
fn photo_prompt(path: &str, message: &Value) -> String {
    match message.get("caption").and_then(Value::as_str) {
        Some(caption) => format!("@{path} {}", caption.trim()),
        None => format!("@{path}"),
    }
}

/// The user's text plus what the model cannot see: the message id, to react
/// to or quote, and the quoted message, so "this one" has its "this".
fn incoming_prompt(message: &Value, text: &str) -> String {
    let id = message
        .get("message_id")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let mut tag = format!("[msg {id}");
    if let Some(quoted) = message.get("reply_to_message") {
        let who = match quoted.get("from") {
            Some(from) if from.get("is_bot").and_then(Value::as_bool) == Some(true) => {
                "you".to_string()
            }
            Some(from) => from
                .get("first_name")
                .and_then(Value::as_str)
                .unwrap_or("them")
                .to_string(),
            None => "them".to_string(),
        };
        let qid = quoted
            .get("message_id")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let body = quoted
            .get("text")
            .or_else(|| quoted.get("caption"))
            .and_then(Value::as_str)
            .map(|t| truncate(t.trim(), QUOTE_CHARS))
            .unwrap_or_else(|| "(no text)".to_string());
        tag.push_str(&format!(", replying to msg {qid} from {who}: \"{body}\""));
    }
    tag.push(']');
    format!("{tag}\n{text}")
}

const QUOTE_CHARS: usize = 600;

#[allow(clippy::too_many_arguments)]
async fn handle_command(
    api: &Api,
    cfg: &Arc<TelegramConfig>,
    chats: &Chats,
    skills: &Skills,
    chat_id: i64,
    message_id: i64,
    name: &str,
    arg: &str,
) {
    match name {
        "start" | "help" => api.send_html_or_plain(chat_id, &help(cfg)).await,
        "new" | "clear" => {
            {
                let mut chats = chats.lock().expect("chats lock");
                let state = chats.entry(chat_id).or_default();
                state.history.clear();
                state.session = None;
                if let Some(agent) = &state.agent {
                    agent.reset_session();
                }
            }
            save_settings(chats, chat_id);
            api.send_text(chat_id, "Started a new conversation.").await;
        }
        "stop" => {
            let (running, agent, queued) = {
                let mut chats = chats.lock().expect("chats lock");
                let state = chats.entry(chat_id).or_default();
                // Set before aborting, so the driver reads it as a deliberate
                // stop rather than reporting the abort as a failure.
                state.stopped = state.running.is_some();
                let queued = state.queued.len();
                state.queued.clear();
                (state.running.take(), state.agent.clone(), queued)
            };
            match running {
                Some(handle) => {
                    handle.abort();
                    // The agent process outlives the turn, so it has to be
                    // told as well, or it keeps working on a message nobody
                    // is reading.
                    if let Some(agent) = agent {
                        agent.cancel().await;
                    }
                    let text = match queued {
                        0 => "Stopped.".to_string(),
                        1 => "Stopped, and dropped the message waiting behind it.".to_string(),
                        n => format!("Stopped, and dropped the {n} messages waiting behind it."),
                    };
                    api.send_text(chat_id, &text).await;
                }
                None => api.send_text(chat_id, "Nothing to stop.").await,
            }
        }
        "mode" => match set_override(chats, chat_id, arg, MODES, |state| &mut state.mode) {
            Some(reply) => api.send_text(chat_id, &reply).await,
            None => {
                let current = get_override(chats, chat_id, |state| state.mode.clone())
                    .unwrap_or_else(|| cfg.mode.clone());
                let keyboard = choice_keyboard("m", MODES, &current);
                api.send_keyboard(chat_id, "<b>Mode</b>\nHow the agent acts", keyboard)
                    .await;
            }
        },
        "effort" => match set_override(chats, chat_id, arg, EFFORTS, |state| &mut state.effort) {
            Some(reply) => api.send_text(chat_id, &reply).await,
            None => {
                let current = get_override(chats, chat_id, |state| state.effort.clone())
                    .unwrap_or_else(|| "default".into());
                let keyboard = choice_keyboard("e", EFFORTS, &current);
                api.send_keyboard(
                    chat_id,
                    "<b>Effort</b>\nHow much it thinks before answering",
                    keyboard,
                )
                .await;
            }
        },
        "model" => {
            if arg == "default" {
                chat_state(chats, chat_id, |state| state.model = None);
                save_settings(chats, chat_id);
                api.send_text(chat_id, "Model is back to the default.")
                    .await;
            } else {
                // Bare /model lists the catalog; an argument filters it.
                send_model_picker(api, chats, chat_id, arg, 0, None).await;
            }
        }
        "status" => {
            let (mode, model, effort, turns, busy) = chat_state(chats, chat_id, |state| {
                (
                    state.mode.clone().unwrap_or_else(|| cfg.mode.clone()),
                    state.model.clone().unwrap_or_else(|| "default".into()),
                    state.effort.clone().unwrap_or_else(|| "default".into()),
                    state.history.len(),
                    state.running.is_some(),
                )
            });
            let text = format!(
                "Repo: {}\nSession: telegram-{chat_id}\nMode: {mode}\nModel: {model}\nEffort: {effort}\nHistory: {turns} messages\nState: {}",
                cfg.repo_root.display(),
                if busy { "Working" } else { "Idle" },
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
                Err(e) => format!("Couldn't read the diff: {e}"),
            };
            if text.is_empty() {
                api.send_text(chat_id, "No uncommitted changes.").await;
            } else {
                let html = format!("<pre>{}</pre>", markdown::escape(&truncate(&text, 3500)));
                api.send_html_or_plain(chat_id, &html).await;
            }
        }
        "mirror" => mirror_command(api, cfg, chat_id, arg).await,
        "skills" => send_skill_picker(api, skills, chat_id, arg, 0, None).await,
        "commit" => send_commit_proposal(api, cfg, chats, chat_id, arg).await,
        other => {
            // Installed skills are commands too: /rust_review -> that skill.
            if let Some(skill) = skills.get(other) {
                let prompt = skill_prompt(skill, arg);
                start_turn(api, cfg, chats, chat_id, message_id, &prompt);
            } else {
                api.send_text(chat_id, "I don't know that command. /help has the list.")
                    .await;
            }
        }
    }
}

const MODES: &[&str] = &["plan", "manual", "auto", "edit", "yolo"];
const EFFORTS: &[&str] = &["off", "low", "medium", "high", "xhigh", "max", "ultra"];

const MODEL_PAGE: usize = 8;

const SKILL_MENU_LIMIT: usize = 15;

const SKILL_PAGE: usize = 8;

async fn send_skill_picker(
    api: &Api,
    skills: &Skills,
    chat_id: i64,
    filter: &str,
    page: usize,
    edit: Option<i64>,
) {
    let term = filter.to_lowercase();
    let mut matches: Vec<(&String, &SkillCommand)> = skills
        .iter()
        .filter(|(command, skill)| {
            term.is_empty()
                || command.contains(&term)
                || skill.description.to_lowercase().contains(&term)
        })
        .filter(|(command, _)| command.len() <= 58)
        .collect();
    matches.sort_by(|a, b| a.0.cmp(b.0));
    if matches.is_empty() {
        api.send_text(chat_id, &format!("No skills match \"{filter}\"."))
            .await;
        return;
    }

    let pages = matches.len().div_ceil(SKILL_PAGE);
    let page = page.min(pages - 1);
    let start = page * SKILL_PAGE;
    let mut rows: Vec<Value> = matches[start..(start + SKILL_PAGE).min(matches.len())]
        .iter()
        .map(|(command, skill)| {
            json!([{ "text": &skill.name, "callback_data": format!("S:{command}") }])
        })
        .collect();
    if pages > 1 {
        let mut nav = Vec::new();
        if page > 0 {
            nav.push(json!({
                "text": "‹ Prev",
                "callback_data": format!("Sp:{}:{filter}", page - 1),
            }));
        }
        nav.push(json!({ "text": format!("{}/{pages}", page + 1), "callback_data": "Mp:noop" }));
        if page + 1 < pages {
            nav.push(json!({
                "text": "Next ›",
                "callback_data": format!("Sp:{}:{filter}", page + 1),
            }));
        }
        rows.push(Value::Array(nav));
    }

    let text = format!(
        "<b>Skills</b>\n{} installed{}",
        matches.len(),
        if filter.is_empty() {
            String::new()
        } else {
            format!(" matching “{}”", markdown::escape(filter))
        }
    );
    match edit {
        Some(message_id) => {
            api.edit_html_keyboard(chat_id, message_id, &text, Value::Array(rows))
                .await
        }
        None => api.send_keyboard(chat_id, &text, Value::Array(rows)).await,
    }
}

const COMMIT_DIFF_LIMIT: usize = 12_000;

async fn send_commit_proposal(
    api: &Api,
    cfg: &Arc<TelegramConfig>,
    chats: &Chats,
    chat_id: i64,
    hint: &str,
) {
    let git = |args: &[&str]| {
        let mut command = tokio::process::Command::new("git");
        command.arg("-C").arg(&cfg.repo_root).args(args);
        async move {
            command
                .output()
                .await
                .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
                .unwrap_or_default()
        }
    };

    if git(&["status", "--short"]).await.is_empty() {
        api.send_text(chat_id, "Nothing to commit. The tree is clean.")
            .await;
        return;
    }
    // Staged changes win when present, matching what `git commit` would do.
    let staged = !git(&["diff", "--cached", "--stat"]).await.is_empty();
    let range: &[&str] = if staged { &["--cached"] } else { &[] };
    let stat = git(&[&["diff"], range, &["--stat"]].concat()).await;
    let diff = git(&[&["diff"], range].concat()).await;

    api.send_typing(chat_id).await;
    let mut prompt = format!(
        "Write a single Conventional Commits message for this change: \
         `type(scope): summary` in imperative mood, lowercase, no trailing period. \
         Add a short body only if the summary cannot carry the change. \
         Reply with the commit message alone, no code fences, no commentary.\n\n\
         Files:\n{stat}\n\nDiff:\n{}",
        truncate(&diff, COMMIT_DIFF_LIMIT)
    );
    if !hint.is_empty() {
        prompt.push_str(&format!("\n\nThe user says this change is about: {hint}"));
    }

    let message = match aster_remote_ask(cfg, &prompt).await {
        Ok(message) => message,
        Err(e) => {
            api.send_text(chat_id, &format!("Couldn't draft a commit message: {e:#}"))
                .await;
            return;
        }
    };
    let subject = message.lines().next().unwrap_or_default().to_string();
    if subject.is_empty() {
        api.send_text(chat_id, "The model sent back an empty message.")
            .await;
        return;
    }

    let scope = if staged {
        "staged changes"
    } else {
        "all changes"
    };
    let text = format!(
        "<b>Commit</b> · {scope}\n<pre>{}</pre>\n{}",
        markdown::escape(&message),
        markdown::escape(&truncate(&stat, 1000))
    );
    // The message rides in chat state; callback data caps at 64 bytes.
    chat_state(chats, chat_id, |state| {
        state.pending_commit = Some(PendingCommit {
            message: message.clone(),
            stage_all: !staged,
        })
    });
    let keyboard = json!([[
        {"text": "Commit", "callback_data": "C:ok"},
        {"text": "Cancel", "callback_data": "C:cancel"},
    ]]);
    api.send_keyboard(chat_id, &text, keyboard).await;
    let _ = subject;
}

async fn run_commit(repo_root: &std::path::Path, commit: &PendingCommit) -> String {
    let run = |args: Vec<String>| {
        let mut command = tokio::process::Command::new("git");
        command.arg("-C").arg(repo_root).args(args);
        async move { command.output().await }
    };
    if commit.stage_all {
        let staged = run(vec!["add".into(), "-A".into()]).await;
        if let Ok(out) = &staged
            && !out.status.success()
        {
            return format!("git add failed: {}", String::from_utf8_lossy(&out.stderr));
        }
    }
    match run(vec!["commit".into(), "-m".into(), commit.message.clone()]).await {
        Ok(out) if out.status.success() => {
            let summary = String::from_utf8_lossy(&out.stdout);
            let head = summary.lines().next().unwrap_or("committed");
            format!("✅ {head}")
        }
        Ok(out) => {
            // Hooks write to both streams, so surface whichever explains it.
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            let detail = if stderr.trim().is_empty() {
                stdout
            } else {
                stderr
            };
            format!("❌ commit failed\n{}", truncate(detail.trim(), 1500))
        }
        Err(e) => format!("❌ could not run git: {e}"),
    }
}

async fn aster_remote_ask(cfg: &Arc<TelegramConfig>, prompt: &str) -> Result<String> {
    crate::bridge::ask_once(&cfg.bin, &cfg.repo_root, prompt).await
}

fn skill_prompt(skill: &SkillCommand, input: &str) -> String {
    let mut prompt = format!(
        "Load the skill `{}` with the read_skill tool and follow its instructions.",
        skill.name
    );
    if !input.is_empty() {
        prompt.push_str(&format!(" Input: {input}"));
    }
    prompt
}

async fn send_model_picker(
    api: &Api,
    chats: &Chats,
    chat_id: i64,
    filter: &str,
    page: usize,
    edit: Option<i64>,
) {
    let models = match model_catalog().await {
        Ok(models) => models,
        Err(e) => {
            api.send_text(chat_id, &format!("Could not load the model list: {e:#}"))
                .await;
            return;
        }
    };
    let term = filter.to_lowercase();
    // Callback data caps at 64 bytes, so ids that would not fit are dropped.
    let matches: Vec<&String> = models
        .iter()
        .filter(|m| term.is_empty() || m.to_lowercase().contains(&term))
        .filter(|m| m.len() <= 60)
        .collect();
    if matches.is_empty() {
        api.send_text(chat_id, &format!("No models match \"{filter}\"."))
            .await;
        return;
    }

    let pages = matches.len().div_ceil(MODEL_PAGE);
    let page = page.min(pages - 1);
    let start = page * MODEL_PAGE;
    let current = get_override(chats, chat_id, |state| state.model.clone());
    let mut rows: Vec<Value> = matches[start..(start + MODEL_PAGE).min(matches.len())]
        .iter()
        .map(|m| {
            let label = match &current {
                Some(active) if active == *m => format!("• {m}"),
                _ => (*m).to_string(),
            };
            json!([{ "text": label, "callback_data": format!("M:{m}") }])
        })
        .collect();

    // Paging keeps the filter so Next/Prev stay inside the same result set.
    if pages > 1 {
        let mut nav = Vec::new();
        if page > 0 {
            nav.push(json!({
                "text": "‹ Prev",
                "callback_data": format!("Mp:{}:{filter}", page - 1),
            }));
        }
        nav.push(json!({
            "text": format!("{}/{pages}", page + 1),
            "callback_data": "Mp:noop",
        }));
        if page + 1 < pages {
            nav.push(json!({
                "text": "Next ›",
                "callback_data": format!("Mp:{}:{filter}", page + 1),
            }));
        }
        rows.push(Value::Array(nav));
    }

    let header = match current {
        Some(model) => format!("<b>Model</b>\nNow {}", markdown::escape(&model)),
        None => "<b>Model</b>\nNow the default".to_string(),
    };
    let text = format!(
        "{header}\n{} models{}",
        matches.len(),
        if filter.is_empty() {
            String::new()
        } else {
            format!(" matching “{}”", markdown::escape(filter))
        }
    );
    match edit {
        Some(message_id) => {
            api.edit_html_keyboard(chat_id, message_id, &text, Value::Array(rows))
                .await
        }
        None => api.send_keyboard(chat_id, &text, Value::Array(rows)).await,
    }
}

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
    chat_state(chats, chat_id, |state| {
        *slot(state) = Some(arg.to_string());
    });
    save_settings(chats, chat_id);
    Some(format!("Now {arg}."))
}

fn get_override<T>(chats: &Chats, chat_id: i64, read: impl FnOnce(&ChatState) -> T) -> T {
    chat_state(chats, chat_id, |state| read(state))
}

fn start_turn(
    api: &Api,
    cfg: &Arc<TelegramConfig>,
    chats: &Chats,
    chat_id: i64,
    message_id: i64,
    prompt: &str,
) {
    let prepared = chat_state(chats, chat_id, |state| {
        if state.running.is_some() {
            None
        } else {
            state.stopped = false;
            state.history.push(WireMessage::user(prompt));
            Some((
                state.mode.clone(),
                state.model.clone(),
                state.effort.clone(),
                state.agent.clone().filter(|agent| agent.is_alive()),
                state.session.clone(),
            ))
        }
    });
    let Some((mode, model, effort, agent, session)) = prepared else {
        // A turn is already going, so the message joins it instead of starting
        // a second conversation about the same work.
        steer_running_turn(api, cfg, chats, chat_id, message_id, prompt);
        return;
    };

    // The chat context rides in as env so the `telegram` MCP server the agent
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

    let (events_tx, events_rx) = mpsc::channel::<TurnEvent>(8);
    eprintln!("[{chat_id}] user: {}", console_text(untagged(prompt), 200));
    let prompt = prompt.to_string();
    let turn_chats = Arc::clone(chats);
    let turn_task = tokio::spawn(async move {
        // One agent process per chat, kept across turns; a session that was
        // saved before a restart is loaded back rather than started over.
        let agent = match agent {
            Some(agent) => agent,
            None => {
                let agent = Agent::spawn(&turn).await?;
                chat_state(&turn_chats, chat_id, |state| {
                    state.agent = Some(Arc::clone(&agent))
                });
                agent
            }
        };
        let session_id = agent
            .ensure_session(&turn.repo_root, session.as_deref())
            .await?;
        if session.as_deref() != Some(&session_id) {
            chat_state(&turn_chats, chat_id, |state| {
                state.session = Some(session_id.clone())
            });
            save_settings(&turn_chats, chat_id);
        }
        agent.configure(&session_id, &turn).await;
        // The standing rules go in with the first message of a session rather
        // than every one, so the recorded conversation stays conversation.
        let text = if agent.primed() {
            prompt
        } else {
            format!("{TELEGRAM_SYSTEM}\n\n{prompt}")
        };
        agent.prompt(&session_id, &text, &events_tx).await
    });
    {
        let mut chats = chats.lock().expect("chats lock");
        chats.entry(chat_id).or_default().running = Some(turn_task.abort_handle());
    }

    let api = api.clone();
    let chats = chats.clone();
    let repo_root = cfg.repo_root.clone();
    let cfg = cfg.clone();
    tokio::spawn(async move {
        let (result, calls) =
            drive_turn(&api, &chats, chat_id, message_id, events_rx, turn_task).await;
        let ok = finish_turn(&api, &chats, chat_id, message_id, Some(&repo_root), result).await;
        if ok && calls >= LEARN_MIN_CALLS && std::env::var("ASTER_LEARN").as_deref() != Ok("0") {
            spawn_learn(&api, &cfg, &chats, chat_id);
        }
        run_queued(&api, &cfg, &chats, chat_id);
    });
}

/// A message sent mid-turn goes to the turn in flight, so the agent hears it
/// while it works instead of picking it up as a second, contextless task. One
/// that arrives as the turn is ending waits and runs next.
fn steer_running_turn(
    api: &Api,
    cfg: &Arc<TelegramConfig>,
    chats: &Chats,
    chat_id: i64,
    message_id: i64,
    prompt: &str,
) {
    let agent = chat_state(chats, chat_id, |state| {
        state.agent.clone().filter(|agent| agent.is_alive())
    });
    let aside = format!("{ASIDE_NOTE}\n\n{prompt}");
    let prompt = prompt.to_string();
    let api = api.clone();
    let cfg = Arc::clone(cfg);
    let chats = Arc::clone(chats);
    tokio::spawn(async move {
        let joined = match agent {
            Some(agent) => agent.steer(&aside).await.unwrap_or(false),
            None => false,
        };
        eprintln!(
            "[{chat_id}] {} {}",
            if joined { "↳ mid-turn" } else { "⏳ queued" },
            console_text(untagged(&prompt), 200)
        );
        let orphaned = chat_state(&chats, chat_id, |state| {
            if joined {
                state.history.push(WireMessage::user(prompt));
                return false;
            }
            state.queued.push((message_id, prompt));
            state.running.is_none()
        });
        // Either way the message landed somewhere, and the turn that answers
        // it may be a while off, so say so now.
        let _ = api.react(chat_id, message_id, "👀").await;
        // The turn ended while this was in flight, so its finish already ran
        // whatever was waiting and nothing else will pick this up.
        if orphaned {
            run_queued(&api, &cfg, &chats, chat_id);
        }
    });
}

/// Whatever came in too late to join the last turn runs now, as one turn.
fn run_queued(api: &Api, cfg: &Arc<TelegramConfig>, chats: &Chats, chat_id: i64) {
    let Some((message_id, prompt)) = take_queued(chats, chat_id) else {
        return;
    };
    start_turn(api, cfg, chats, chat_id, message_id, &prompt);
}

/// The waiting messages as one prompt, answered as a reply to the first of
/// them. None when nothing is waiting.
fn take_queued(chats: &Chats, chat_id: i64) -> Option<(i64, String)> {
    let queued = chat_state(chats, chat_id, |state| std::mem::take(&mut state.queued));
    let (message_id, _) = queued.first()?;
    let prompt = queued
        .iter()
        .map(|(_, text)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    Some((*message_id, prompt))
}

#[cfg(test)]
#[path = "tests/telegram_queue_test.rs"]
mod queue_tests;

/// After a task-sized turn, score it and refine its skill in a child process,
/// so the chat sees what changed without waiting on it.
fn spawn_learn(api: &Api, cfg: &Arc<TelegramConfig>, chats: &Chats, chat_id: i64) {
    let session = chat_state(chats, chat_id, |state| {
        if state.learning {
            return None;
        }
        state.learning = true;
        state.session.clone()
    });
    let Some(session) = session else {
        return;
    };
    let api = api.clone();
    let cfg = Arc::clone(cfg);
    let chats = Arc::clone(chats);
    tokio::spawn(async move {
        let run = tokio::process::Command::new(&cfg.bin)
            .current_dir(&cfg.repo_root)
            .args(["learn", "--session", &session, "--json"])
            .kill_on_drop(true)
            .output();
        let outcome = match tokio::time::timeout(LEARN_TIMEOUT, run).await {
            Ok(Ok(output)) if output.status.success() => {
                serde_json::from_slice::<LearnReport>(&output.stdout)
                    .map_err(|e| format!("unreadable learn report: {e}"))
            }
            Ok(Ok(output)) => Err(learn_error(&output)),
            Ok(Err(e)) => Err(format!("could not start aster learn: {e}")),
            Err(_) => Err("aster learn timed out".to_string()),
        };
        chat_state(&chats, chat_id, |state| state.learning = false);
        match outcome {
            Ok(report) => {
                eprintln!("[{chat_id}] learn: {}", report.summary());
                if let Some(line) = render_learned(&report) {
                    api.send_html(chat_id, &line).await;
                }
            }
            Err(e) => eprintln!("[{chat_id}] learn failed: {e}"),
        }
    });
}

/// A failed `--json` run reports on stdout and leaves stderr empty, so
/// reading only stderr logs a blank reason.
fn learn_error(output: &std::process::Output) -> String {
    #[derive(Deserialize)]
    struct Failure {
        error: String,
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Ok(failure) = serde_json::from_str::<Failure>(stdout.trim()) {
        return failure.error;
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        output.status.to_string()
    } else {
        stderr
    }
}

/// What `aster learn --json` prints; mirrors the CLI's report.
#[derive(Debug, Deserialize)]
pub(crate) struct LearnReport {
    pub task: Option<String>,
    pub score: LearnScore,
    pub best: Option<LearnScore>,
    pub outcome: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) struct LearnScore {
    pub rounds: usize,
    pub calls: usize,
}

impl LearnReport {
    fn summary(&self) -> String {
        format!(
            "{} · {} rounds, {} calls · {}{}",
            self.task.as_deref().unwrap_or("no task"),
            self.score.rounds,
            self.score.calls,
            self.outcome,
            self.reason
                .as_deref()
                .map(|r| format!(" ({r})"))
                .unwrap_or_default()
        )
    }
}

/// One line for the chat when the skill moved; nothing when there was
/// nothing to learn.
pub(crate) fn render_learned(report: &LearnReport) -> Option<String> {
    let task = markdown::escape(report.task.as_deref()?);
    let score = report.score;
    let best = report.best.map(|b| b.rounds);
    let tail = match (report.outcome.as_str(), best) {
        ("new", _) => format!("{} rounds, {} calls · new skill", score.rounds, score.calls),
        ("improved", Some(best)) => format!(
            "{} rounds, {} calls · beat {best} · procedure updated",
            score.rounds, score.calls
        ),
        ("improved", None) => format!(
            "{} rounds, {} calls · procedure updated",
            score.rounds, score.calls
        ),
        ("updated", _) => format!("{} rounds · matched best · procedure refined", score.rounds),
        ("regressed", Some(best)) => {
            format!("{} rounds · best is {best} · lesson noted", score.rounds)
        }
        ("regressed", None) => format!("{} rounds · lesson noted", score.rounds),
        _ => return None,
    };
    Some(format!("📚 <b>{task}</b> · {tail}"))
}

const LEARN_MIN_CALLS: usize = 6;
const LEARN_TIMEOUT: Duration = Duration::from_secs(180);

#[allow(clippy::too_many_arguments)]
async fn drive_turn(
    api: &Api,
    chats: &Chats,
    chat_id: i64,
    reply_to: i64,
    mut events: mpsc::Receiver<TurnEvent>,
    turn_task: tokio::task::JoinHandle<Result<TurnOutcome>>,
) -> (Result<TurnOutcome>, usize) {
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

    let mut activity = Activity::new(api.clone(), chat_id, reply_to);
    let mut calls = 0usize;
    let mut plan_id: Option<i64> = None;
    loop {
        // An edit held back by the rate gap goes out once the gap has passed,
        // even when the agent is quiet: a long tool call is still a step.
        let event = match activity.due() {
            Some(at) => tokio::select! {
                event = events.recv() => event,
                () = tokio::time::sleep_until(at.into()) => {
                    activity.flush(false).await;
                    continue;
                }
            },
            None => events.recv().await,
        };
        let Some(event) = event else { break };
        match event {
            TurnEvent::ToolCall {
                id,
                name,
                arguments,
            } => {
                // The plan is the one thing worth its own message: it is the
                // agent's intent, and it must not scroll away with the steps.
                if name == "update_plan"
                    && let Some(plan) = plan_message(&arguments)
                {
                    activity.flush(true).await;
                    plan_id = match plan_id {
                        Some(existing) => {
                            api.edit_html(chat_id, existing, &plan).await;
                            Some(existing)
                        }
                        None => api.send_html(chat_id, &plan).await,
                    };
                }
                calls += 1;
                activity.push(id, tool_line(&name, &arguments));
                activity.flush(false).await;
                eprintln!("[{chat_id}] → {}", console_tool(&name, &arguments));
            }
            TurnEvent::ToolResult { id, error, output } => {
                // Every screenshot the agent takes reaches the chat, whether or
                // not it decides to attach it: the person cannot see the phone.
                let shots = shot_paths(&output);
                if !shots.is_empty() {
                    // The card so far stays above the photos; a fresh one
                    // carries on below them, so the thread reads top to bottom.
                    activity.flush(true).await;
                    let caption = activity.caption_for(&id);
                    for path in shots {
                        api.send_chat_action(chat_id, "upload_photo").await;
                        if let Err(err) = api.send_photo_file(chat_id, &path, &caption).await {
                            tracing::warn!("could not send {path}: {err:#}");
                        }
                    }
                    activity.rehome();
                }
                activity.complete(&id, error, output);
                activity.flush(false).await;
                eprintln!(
                    "[{chat_id}]   {}",
                    if error { "✗ tool failed" } else { "✓ ok" }
                );
            }
            TurnEvent::Text { content } => {
                activity.say(&content);
                activity.flush(false).await;
            }
            TurnEvent::Thought { content } => {
                activity.think(&content);
                activity.flush(false).await;
            }
            TurnEvent::ApprovalRequest {
                preview,
                scope,
                respond,
            } => {
                activity.flush(true).await;
                let subject = approval_subject(&preview);
                let mut text = format!(
                    "<b>Needs your go-ahead</b>\n<pre>{}</pre>",
                    markdown::escape(&truncate(&subject, 3000))
                );
                if let Some(scope) = scope {
                    text.push_str(&format!("\n<code>{}</code>", markdown::escape(&scope)));
                }
                let keyboard = json!([[
                    {"text": "Allow once", "callback_data": "a:allow"},
                    {"text": "Always allow", "callback_data": "a:always"},
                    {"text": "Deny", "callback_data": "a:deny"},
                ]]);
                api.send_keyboard_reply(chat_id, &text, keyboard, Some(reply_to))
                    .await;
                eprintln!(
                    "[{chat_id}] ⏸ approval needed: {}",
                    console_text(&subject, 120)
                );
                set_pending(chats, chat_id, Pending::Approval { subject, respond });
            }
            TurnEvent::Question {
                header,
                question,
                options,
                respond,
            } => {
                activity.flush(true).await;
                eprintln!("[{chat_id}] ? {}", console_text(&question, 120));
                let text = format!(
                    "<b>{}</b>\n{}",
                    markdown::escape(&header),
                    markdown::escape(&question)
                );
                // Native reply keyboard: options sit above the text field and
                // a tap sends the option as a normal message; typing a custom
                // answer works too.
                let rows: Vec<Value> = options
                    .iter()
                    .map(|opt| json!([{ "text": opt }]))
                    .chain(std::iter::once(json!([{ "text": SKIP_LABEL }])))
                    .collect();
                let keyboard = json!({
                    "keyboard": rows,
                    "one_time_keyboard": true,
                    "resize_keyboard": true,
                    "input_field_placeholder": "Pick an option or type an answer",
                });
                api.send_reply_keyboard_reply(chat_id, &text, keyboard, Some(reply_to))
                    .await;
                set_pending(chats, chat_id, Pending::Question(respond));
            }
        }
    }
    let result = turn_task
        .await
        .unwrap_or_else(|e| Err(anyhow!("turn cancelled: {e}")));
    typing.abort();
    let end = if result.is_ok() {
        TurnEnd::Done
    } else if chat_state(chats, chat_id, |state| state.stopped) {
        TurnEnd::Stopped
    } else {
        TurnEnd::Failed
    };
    activity.finish(end).await;
    match &result {
        Ok(outcome) => eprintln!(
            "[{chat_id}] ✔ done: {}",
            console_text(outcome.reply.trim(), 200)
        ),
        Err(e) => eprintln!("[{chat_id}] ✗ turn failed: {e:#}"),
    }
    (result, calls)
}

async fn finish_turn(
    api: &Api,
    chats: &Chats,
    chat_id: i64,
    reply_to: i64,
    repo_root: Option<&std::path::Path>,
    result: Result<TurnOutcome>,
) -> bool {
    let (result, stopped) = {
        let mut chats = chats.lock().expect("chats lock");
        let state = chats.entry(chat_id).or_default();
        state.running = None;
        state.pending = None;
        let stopped = std::mem::take(&mut state.stopped);
        if let Ok(outcome) = &result {
            state
                .history
                .push(WireMessage::assistant(outcome.reply.clone()));
        }
        (result, stopped)
    };
    let ok = result.is_ok();
    match result {
        Ok(outcome) => {
            let (text, gifs) = extract_gifs(&outcome.reply);
            send_reply_chunks(api, chat_id, &text, Some(reply_to)).await;
            for gif in gifs {
                api.send_animation(chat_id, &gif).await;
            }
            if !outcome.edits.is_empty() {
                send_edit_diff(api, chat_id, repo_root, &outcome.edits).await;
            }
        }
        // /stop already confirmed; reporting the abort as a failure on top of
        // it would be a second, noisier message.
        Err(_) if stopped => {}
        Err(e) => {
            let text = format!("I couldn't finish that. Send it again to retry.\n\n{e:#}");
            api.send_text_reply(chat_id, &text, Some(reply_to)).await;
        }
    }
    ok
}

fn set_pending(chats: &Chats, chat_id: i64, pending: Pending) {
    let mut chats = chats.lock().expect("chats lock");
    chats.entry(chat_id).or_default().pending = Some(pending);
}

async fn handle_callback(
    api: &Api,
    cfg: &Arc<TelegramConfig>,
    chats: &Chats,
    skills: &Skills,
    callback: &Value,
) {
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
    // The tapped message is edited to state the outcome, not just toasted.
    if let Some(choice) = data.strip_prefix("m:").filter(|c| MODES.contains(c)) {
        chat_state(chats, chat_id, |state| {
            state.mode = Some(choice.to_string())
        });
        save_settings(chats, chat_id);
        let note = format!("Mode is now {choice}. It applies from your next message.");
        api.answer_callback(callback_id, &note).await;
        api.settle_callback_message(callback, &note).await;
        return;
    }
    if let Some(choice) = data.strip_prefix("e:").filter(|c| EFFORTS.contains(c)) {
        chat_state(chats, chat_id, |state| {
            state.effort = Some(choice.to_string())
        });
        save_settings(chats, chat_id);
        let note = format!("Effort is now {choice}.");
        api.answer_callback(callback_id, &note).await;
        api.settle_callback_message(callback, &note).await;
        return;
    }
    if data == "Mp:noop" {
        api.answer_callback(callback_id, "").await;
        return;
    }
    if let Some(action) = data.strip_prefix("C:") {
        let pending = chat_state(chats, chat_id, |state| state.pending_commit.take());
        let Some(commit) = pending.filter(|_| action == "ok") else {
            api.answer_callback(callback_id, "Cancelled.").await;
            api.settle_callback_message(callback, "Commit cancelled.")
                .await;
            return;
        };
        api.answer_callback(callback_id, "Committing…").await;
        let outcome = run_commit(&cfg.repo_root, &commit).await;
        api.settle_callback_message(callback, &outcome).await;
        return;
    }
    if let Some(rest) = data.strip_prefix("Sp:") {
        let (page, filter) = match rest.split_once(':') {
            Some((page, filter)) => (page.parse().unwrap_or(0), filter),
            None => (rest.parse().unwrap_or(0), ""),
        };
        api.answer_callback(callback_id, "").await;
        let message_id = callback_message_ids(callback).map(|(_, id)| id);
        send_skill_picker(api, skills, chat_id, filter, page, message_id).await;
        return;
    }
    if let Some(command) = data.strip_prefix("S:") {
        let Some(skill) = skills.get(command) else {
            api.answer_callback(callback_id, "That skill is gone.")
                .await;
            return;
        };
        api.answer_callback(callback_id, &format!("Running {}", skill.name))
            .await;
        api.settle_callback_message(callback, &format!("▶️ {}", skill.name))
            .await;
        let prompt = skill_prompt(skill, "");
        start_turn(api, cfg, chats, chat_id, 0, &prompt);
        return;
    }
    if let Some(rest) = data.strip_prefix("Mp:") {
        let (page, filter) = match rest.split_once(':') {
            Some((page, filter)) => (page.parse().unwrap_or(0), filter),
            None => (rest.parse().unwrap_or(0), ""),
        };
        api.answer_callback(callback_id, "").await;
        let message_id = callback_message_ids(callback).map(|(_, id)| id);
        send_model_picker(api, chats, chat_id, filter, page, message_id).await;
        return;
    }
    if let Some(model) = data.strip_prefix("M:") {
        chat_state(chats, chat_id, |state| {
            state.model = Some(model.to_string())
        });
        save_settings(chats, chat_id);
        let note = format!("Model is now {model}.");
        api.answer_callback(callback_id, &note).await;
        api.settle_callback_message(callback, &note).await;
        return;
    }

    let pending = {
        let mut chats = chats.lock().expect("chats lock");
        chats.entry(chat_id).or_default().pending.take()
    };
    // An answered approval is deleted rather than settled: the activity list
    // already shows the step, so a second message would just repeat it.
    let (toast, answered) = match (pending, data) {
        (Some(Pending::Approval { respond, .. }), "a:allow") => {
            let _ = respond.send(Answer::Allow);
            ("Allowed once", true)
        }
        (Some(Pending::Approval { respond, .. }), "a:always") => {
            let _ = respond.send(Answer::AlwaysAllow);
            ("Always allowed", true)
        }
        (Some(Pending::Approval { subject, respond }), "a:deny") => {
            let _ = respond.send(Answer::Deny);
            // A denial has no step to show, so it leaves a line behind.
            api.settle_callback_message(callback, &format!("Denied · {subject}"))
                .await;
            ("Denied", false)
        }
        (None, _) => ("That prompt has expired.", false),
        (Some(pending), _) => {
            // Unrecognized data: put the prompt back rather than dropping it.
            set_pending(chats, chat_id, pending);
            ("Unknown action.", false)
        }
    };
    api.answer_callback(callback_id, toast).await;
    if answered {
        api.delete_callback_message(callback).await;
    }
}

struct Activity {
    api: Api,
    chat_id: i64,
    reply_to: i64,
    message_id: Option<i64>,
    lines: Vec<Step>,
    /// What the agent has said since its last tool call.
    saying: String,
    /// What the agent has thought since its last tool call.
    thinking: String,
    last_flush: Instant,
    /// A change the rate gap kept off the card, waiting for the gap to pass.
    held: bool,
}

struct Step {
    id: String,
    emoji: String,
    label: String,
    status: Status,
    /// The first line of what came back, once it has.
    result: String,
}

impl Step {
    fn marker(&self) -> &str {
        match self.status {
            Status::Running => &self.emoji,
            Status::Done => "✅",
            Status::Failed => "❌",
        }
    }
}

#[derive(PartialEq)]
enum Status {
    Running,
    Done,
    Failed,
}

/// How a turn ended, so the activity card says stopped only when the user
/// asked it to.
enum TurnEnd {
    Done,
    Stopped,
    Failed,
}

impl Activity {
    fn new(api: Api, chat_id: i64, reply_to: i64) -> Self {
        Self {
            api,
            chat_id,
            reply_to,
            message_id: None,
            lines: Vec::new(),
            saying: String::new(),
            thinking: String::new(),
            last_flush: Instant::now()
                .checked_sub(ACTIVITY_EDIT_GAP)
                .unwrap_or_else(Instant::now),
            held: false,
        }
    }

    /// When a held change can go out, if there is one.
    fn due(&self) -> Option<Instant> {
        self.held.then(|| self.last_flush + ACTIVITY_EDIT_GAP)
    }

    fn push(&mut self, id: String, line: String) {
        let line = match line == LOOK_LINE {
            true => self.look_line(),
            false => line,
        };
        let (emoji, label) = match line.split_once(' ') {
            Some((emoji, label)) => (emoji.to_string(), label.to_string()),
            None => (String::from("◦"), line),
        };
        self.lines.push(Step {
            id,
            emoji,
            label,
            status: Status::Running,
            result: String::new(),
        });
        // A new action ends the narration that led to it.
        self.saying.clear();
        self.thinking.clear();
    }

    /// A look at a screenshot says what the screenshot was of, taken from
    /// the step that shot it: "Screenshot the board" → "of the board".
    fn look_line(&self) -> String {
        let subject = self
            .lines
            .iter()
            .rev()
            .map(|step| plain_text(&step.label))
            .find_map(|label| {
                let rest = label.strip_prefix("Screenshot ")?;
                let rest = rest.strip_prefix("of ").unwrap_or(rest);
                (!rest.is_empty() && !rest.chars().next().is_some_and(|c| c.is_ascii_digit()))
                    .then(|| rest.to_string())
            });
        match subject {
            Some(subject) => format!(
                "👀 <b>Look at the screenshot of {}</b>",
                markdown::escape(&subject)
            ),
            None => LOOK_LINE.to_string(),
        }
    }

    fn say(&mut self, chunk: &str) {
        self.saying.push_str(chunk);
    }

    fn think(&mut self, chunk: &str) {
        self.thinking.push_str(chunk);
    }

    /// What a photo is captioned with: the step that took it, as plain text.
    fn caption_for(&self, id: &str) -> String {
        let label = self
            .lines
            .iter()
            .rfind(|step| step.id == id)
            .map(|step| plain_text(&step.label))
            .unwrap_or_default();
        if label.is_empty() {
            "📸 Screenshot".to_string()
        } else {
            format!("📸 {label}")
        }
    }

    /// Continue in a new message, so what follows lands below whatever was
    /// just posted rather than editing a card that is now above it.
    fn rehome(&mut self) {
        self.message_id = None;
    }

    fn complete(&mut self, id: &str, error: bool, output: String) {
        // A tool that reports no id still completes the oldest running step.
        let index = self
            .lines
            .iter()
            .rposition(|step| step.id == id)
            .or_else(|| {
                self.lines
                    .iter()
                    .position(|step| matches!(step.status, Status::Running))
            });
        if let Some(step) = index.and_then(|i| self.lines.get_mut(i)) {
            step.status = if error { Status::Failed } else { Status::Done };
            step.result = summarize_result(&output);
        }
    }

    fn render(&self, header: &str) -> String {
        let mut text = String::from(header);
        let hidden = self.lines.len().saturating_sub(ACTIVITY_WINDOW);
        if hidden > 0 {
            let noun = if hidden == 1 { "step" } else { "steps" };
            text.push_str(&format!("\n<i>… {hidden} earlier {noun}</i>"));
        }
        // Identical consecutive steps collapse, but only while they share a
        // status, so a failure is never hidden inside a run.
        let visible = &self.lines[hidden..];
        let mut i = 0;
        while i < visible.len() {
            let mut run = 1;
            while i + run < visible.len()
                && visible[i + run].label == visible[i].label
                && visible[i + run].status == visible[i].status
            {
                run += 1;
            }
            text.push_str("\n\n");
            text.push_str(visible[i].marker());
            text.push(' ');
            text.push_str(&visible[i].label);
            if run > 1 {
                text.push_str(&format!(" ×{run}"));
            }
            // The last outcome of a collapsed run is the one that matters.
            let result = &visible[i + run - 1].result;
            if !result.is_empty() {
                text.push_str(&format!("\n<i>{}</i>", markdown::escape(result)));
            }
            i += run;
        }
        let thinking = tail(&self.thinking, NARRATION_CHARS);
        if !thinking.is_empty() {
            text.push_str(&format!("\n\n💭 <i>{}</i>", markdown::escape(&thinking)));
        }
        let saying = tail(&self.saying, NARRATION_CHARS);
        if !saying.is_empty() {
            text.push_str(&format!("\n\n{}", markdown::escape(&saying)));
        }
        text
    }
    async fn flush(&mut self, force: bool) {
        if self.lines.is_empty() {
            return;
        }
        if !force && self.message_id.is_some() && self.last_flush.elapsed() < ACTIVITY_EDIT_GAP {
            self.held = true;
            return;
        }
        self.held = false;
        let text = self.render("<b>Working…</b>");
        match self.message_id {
            None => {
                self.message_id = self
                    .api
                    .send_html_reply(self.chat_id, &text, Some(self.reply_to))
                    .await;
            }
            Some(id) => self.api.edit_html(self.chat_id, id, &text).await,
        }
        self.last_flush = Instant::now();
    }

    async fn finish(&mut self, end: TurnEnd) {
        if self.lines.is_empty() {
            return;
        }
        let label = match end {
            TurnEnd::Done => "Done",
            TurnEnd::Stopped => "Stopped",
            TurnEnd::Failed => "Failed",
        };
        let count = self.lines.len();
        let steps = if count == 1 { "step" } else { "steps" };
        // The answer follows as its own message; the finished card keeps the
        // steps and drops the narration so nothing is said twice.
        self.saying.clear();
        self.thinking.clear();
        let text = self.render(&format!("<b>{label}</b> · {count} {steps}"));
        match self.message_id {
            None => {
                self.message_id = self
                    .api
                    .send_html_reply(self.chat_id, &text, Some(self.reply_to))
                    .await;
            }
            Some(id) => self.api.edit_html(self.chat_id, id, &text).await,
        }
    }
}

const DIFF_INLINE_LIMIT: usize = 3_000;
/// How much of a tool's output the activity card shows under its step.
const RESULT_LINES: usize = 2;
const RESULT_CHARS: usize = 160;
/// How much of the agent's live narration or reasoning the card shows.
const NARRATION_CHARS: usize = 400;

/// Capitalised, without a trailing full stop, so it reads as a step.
fn sentence(text: &str) -> String {
    let text = text.trim().trim_end_matches('.');
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// A phone action as a person would say it: the verb, then what it acted on.
fn phone_line(verb: &str, args: &[&str], step: &dyn Fn(&str, &str, &str) -> String) -> String {
    let rest = args.join(" ");
    match verb {
        "map" => "📱 <b>Read the screen</b>".into(),
        "ocr" => "📱 <b>Read the pixels</b>".into(),
        "find" => step("📱", "Find", &rest),
        "tap" => step("📱", "Tap", &rest),
        "press" => step("📱", "Hold", &rest),
        "type" => step("⌨️", "Type", &rest),
        "key" => step("📱", "Press", &rest),
        "swipe" => step(
            "📱",
            "Swipe",
            &args.iter().take(2).copied().collect::<Vec<_>>().join(" → "),
        ),
        "scroll" => step("📱", "Scroll", &rest),
        "shot" => step("📸", "Screenshot", &rest),
        "open" => step("📱", "Open", &rest),
        "restart" => step("📱", "Restart", &rest),
        "later" => match args {
            [when, rest @ ..] => step("⏰", "Later", &format!("{when}: {}", rest.join(" "))),
            _ => step("⏰", "Later", &rest),
        },
        "wait" => match args {
            [text, secs] => step("⏳", "Wait for", &format!("{text} ({secs}s)")),
            _ => step("⏳", "Wait for", &rest),
        },
        "volume" => step("🔊", "Volume", &rest),
        "media" => step("🎵", "Media", &rest),
        "notes" => "🔔 <b>Read notifications</b>".into(),
        other => step("📱", &markdown::escape(other), &rest),
    }
}

/// What came back, as one short phrase. The raw lines are for the model; the
/// person only needs to know whether the screen moved and where they are.
fn summarize_result(output: &str) -> String {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !is_plumbing(l))
        .collect();
    let Some(first) = lines.first() else {
        return String::new();
    };
    // A chat tool answering `{"ok":true}` has nothing to add to its check mark.
    if first.starts_with('{') && first.contains("\"ok\":true") {
        return String::new();
    }
    if let Some(rest) = first.strip_prefix("changed: ") {
        return describe_change(rest);
    }
    if first.starts_with("shot ") {
        return "picture sent".into();
    }
    if let Some(rest) = first.strip_prefix("ocr blocks=") {
        return match rest {
            "0" => "nothing readable on screen".into(),
            _ => format!("read {rest} pieces of text"),
        };
    }
    if let Some(rest) = first.strip_prefix("found after ") {
        let app = lines
            .get(1)
            .and_then(|l| l.split_once("pkg="))
            .map(|(_, r)| r);
        let app = app.and_then(|r| r.split_whitespace().next()).map(app_name);
        let secs = rest.trim_end_matches('s').parse::<f64>().unwrap_or(0.0);
        let when = if secs < 1.0 {
            "right away".to_string()
        } else {
            format!("after {secs:.0}s")
        };
        return match app {
            Some(app) => format!("found {when} in {app}"),
            None => format!("found {when}"),
        };
    }
    if let Some(rest) = first.strip_prefix("pkg=") {
        let mut words = rest.split_whitespace();
        let app = words.next().map(app_name).unwrap_or_default();
        if let Some(n) = words.find_map(|w| w.strip_prefix("matches=")) {
            return match n {
                "0" => format!("not on the {app} screen"),
                _ => format!("found in {app}"),
            };
        }
        if words.any(|w| w.starts_with("elements=")) {
            return format!("{app} is on screen");
        }
        return format!("in {app}");
    }
    if let Some(rest) = first.strip_prefix("opening ") {
        return format!("{} is open", rest.split(" (").next().unwrap_or(rest));
    }
    if let Some(rest) = first.strip_prefix("restarted ") {
        return format!(
            "{} is back",
            app_name(rest.split(':').next().unwrap_or(rest))
        );
    }
    if first.contains(" is an image; ") {
        return String::new();
    }
    if let Some(rest) = first.strip_prefix("now ") {
        return rest.to_string();
    }
    if let Some(rest) = first.strip_prefix("error: ") {
        return format!("did not work: {}", truncate(rest, 90));
    }
    truncate(
        &lines
            .iter()
            .take(RESULT_LINES)
            .copied()
            .collect::<Vec<_>>()
            .join(" · "),
        RESULT_CHARS,
    )
}

/// `+14 -1 pkg=com.android.chrome after_ms=1377` as words.
fn describe_change(rest: &str) -> String {
    let mut added = "";
    let mut removed = "";
    let mut app = String::new();
    for word in rest.split_whitespace() {
        if let Some(a) = word.strip_prefix('+') {
            added = a;
        } else if let Some(r) = word.strip_prefix('-') {
            removed = r;
        } else if let Some(p) = word.strip_prefix("pkg=") {
            app = app_name(p);
        }
    }
    if added == "0" && removed == "0" {
        return format!("nothing happened in {app}");
    }
    format!("{app} responded")
}

/// `com.android.chrome` as "chrome": the last segment, unless the package
/// is one nobody would recognise that way.
fn app_name(pkg: &str) -> String {
    match pkg {
        "app.lawnchair" | "com.android.launcher3" | "com.google.android.apps.nexuslauncher" => {
            "the launcher".into()
        }
        "com.android.systemui" => "the system".into(),
        "com.android.settings" => "Settings".into(),
        "com.android.vending" => "the Play Store".into(),
        other => {
            let last = other.rsplit('.').next().unwrap_or(other);
            let mut chars = last.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
    }
}

const LOOK_LINE: &str = "👀 <b>Look at the screenshot</b>";

fn is_image_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".webp", ".gif"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

fn is_plumbing(line: &str) -> bool {
    line == "stdout:"
        || line == "stderr:"
        || line.starts_with("exit code:")
        || line.starts_with("receipt: posted")
        || (line.starts_with('(') && line.ends_with("ms)"))
}

/// The end of a growing text, so a card shows the latest words rather than
/// the first ones.
fn tail(text: &str, chars: usize) -> String {
    let text = text.trim();
    let count = text.chars().count();
    if count <= chars {
        return text.to_string();
    }
    let skipped = text.chars().skip(count - chars).collect::<String>();
    format!("…{skipped}")
}

async fn send_edit_diff(
    api: &Api,
    chat_id: i64,
    repo_root: Option<&std::path::Path>,
    edits: &[String],
) {
    let files: Vec<String> = edits
        .iter()
        .map(|path| format!("•  <code>{}</code>", markdown::escape(path)))
        .collect();
    let header = format!("✏️ <b>Edited</b>\n{}", files.join("\n"));

    let Some(repo_root) = repo_root else {
        api.send_html_or_plain(chat_id, &header).await;
        return;
    };
    // Untracked files have no diff against HEAD, so stage intents first.
    let mut args = vec!["-C".to_string(), repo_root.display().to_string()];
    args.extend(["diff".into(), "--no-color".into(), "--".into()]);
    args.extend(edits.iter().cloned());
    let diff = tokio::process::Command::new("git")
        .args(&args)
        .output()
        .await
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default();

    if diff.is_empty() {
        api.send_html_or_plain(chat_id, &header).await;
        return;
    }
    if diff.len() <= DIFF_INLINE_LIMIT {
        let text = format!("{header}\n<pre>{}</pre>", markdown::escape(&diff));
        api.send_html_or_plain(chat_id, &text).await;
        return;
    }
    api.send_html_or_plain(chat_id, &header).await;
    let send_attachment = async {
        let path = crate::mcp_server::write_scratch_document("Changes", &diff).await?;
        let sent = api
            .send_document_file(chat_id, &path, Some("Changes"))
            .await;
        let _ = tokio::fs::remove_file(&path).await;
        sent
    };
    if send_attachment.await.is_err() {
        let text = format!(
            "<pre>{}</pre>",
            markdown::escape(&truncate(&diff, DIFF_INLINE_LIMIT))
        );
        api.send_html_or_plain(chat_id, &text).await;
    }
}

/// The PNGs an `asterctl shot` names in a tool's output, one per
/// `shot <path> (WxH, N bytes)` line.
fn shot_paths(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let at = words
                .windows(2)
                .position(|pair| pair[0] == "shot" && pair[1].ends_with(".png"))?;
            Some(words[at + 1].to_string())
        })
        .collect()
}

/// A card label without its HTML, for places that take plain text.
fn plain_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

fn plan_message(arguments: &str) -> Option<String> {
    let args: Value = serde_json::from_str(arguments).ok()?;
    let steps = args.get("steps")?.as_array()?;
    if steps.is_empty() {
        return None;
    }
    let mut text = String::from("📋 <b>Plan</b>");
    let mut done = 0;
    for step in steps {
        let label = step.get("label").and_then(Value::as_str).unwrap_or("");
        let status = step.get("status").and_then(Value::as_str).unwrap_or("");
        let marker = match status {
            "done" => {
                done += 1;
                "✅"
            }
            "in_progress" => "▶️",
            "blocked" => "⛔",
            "skipped" => "⏭",
            _ => "▫️",
        };
        let label = markdown::escape(label);
        // The step in flight is bolded so the plan reads at a glance.
        let line = if status == "in_progress" {
            format!("\n{marker} <b>{label}</b>")
        } else {
            format!("\n{marker} {label}")
        };
        text.push_str(&line);
    }
    text.push_str(&format!("\n\n{done}/{} done", steps.len()));
    Some(text)
}

fn tool_line(name: &str, arguments: &str) -> String {
    let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let field = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| args.get(k).and_then(Value::as_str))
            .unwrap_or("")
            .to_string()
    };
    let code = |text: &str| format!("<code>{}</code>", markdown::escape(&truncate(text, 80)));
    // `📖 <b>Read</b> lib.rs`; an empty target degrades to just the verb.
    let step = |emoji: &str, verb: &str, target: &str| {
        if target.is_empty() {
            format!("{emoji} <b>{verb}</b>")
        } else {
            format!("{emoji} <b>{verb}</b> {}", code(target))
        }
    };
    match name {
        "read_file" => match field(&["path"]) {
            path if is_image_path(&path) => LOOK_LINE.into(),
            path => step("📖", "Read", &short_path(&path)),
        },
        "list_files" => {
            let path = short_path(&field(&["path"]));
            let target = if path.is_empty() {
                "the repo root"
            } else {
                &path
            };
            step("📂", "List", target)
        }
        "search_files" => step(
            "🔎",
            "Search",
            &pretty_query(&field(&["query", "pattern", "regex"])),
        ),
        "find_files" => step(
            "🗂",
            "Find",
            &pretty_query(&field(&["pattern", "glob", "query"])),
        ),
        "run_command" => {
            let mut cmd = field(&["command"]);
            let extra: Vec<&str> = args
                .get("args")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            // A phone action reads best in the model's own words ("Tap the
            // Umbrella song result"), else as its verb and target.
            if cmd == "asterctl" && !extra.is_empty() {
                let summary = field(&["description"]);
                if !summary.is_empty() {
                    return step(
                        "📱",
                        &markdown::escape(&sentence(&truncate(&summary, 70))),
                        "",
                    );
                }
                return phone_line(extra[0], &extra[1..], &step);
            }
            if !extra.is_empty() {
                cmd.push(' ');
                cmd.push_str(&extra.join(" "));
            }
            // The model's summary takes the verb's place, and the command
            // itself follows, because the summary alone hides what ran.
            let summary = field(&["description"]);
            if !summary.is_empty() {
                return step("🖥", &markdown::escape(&truncate(&summary, 80)), &cmd);
            }
            step("🖥", "Run", &cmd)
        }
        "run_tests" => "🧪 <b>Running tests</b>".into(),
        "open_preview" => step("🌐", "Opened", &field(&["target"])),
        // MCP calls arrive through the bridge tool, so the real tool is an id
        // in the arguments; label it like a first-class tool.
        "aster_mcp" => {
            let id = field(&["id", "tool", "name"]);
            if id.is_empty() {
                step("🔌", "Look up", &field(&["query"]))
            } else {
                mcp_line(&id, &args, &step)
            }
        }
        "edit_file" => step("✍️", "Edit", &short_path(&field(&["path"]))),
        "remember" => step("🧠", "Remember", &field(&["name"])),
        "recall" => step("🧠", "Recall", &field(&["name"])),
        "forget" => step("🧠", "Forget", &field(&["name"])),
        "read_skill" => step("📚", "Skill", &field(&["name"])),
        "update_plan" => "📋 <b>Updating the plan</b>".into(),
        "exit_plan_mode" => "📋 <b>Plan ready</b>".into(),
        "ask_user" => "💬 <b>Asking you</b>".into(),
        other => mcp_line(other, &args, &step),
    }
}

fn mcp_line(id: &str, args: &Value, step: &dyn Fn(&str, &str, &str) -> String) -> String {
    let arg = |key: &str| args.get(key).and_then(Value::as_str).unwrap_or_default();
    match id {
        "giphy/search_gifs" => step("🎞", "Search gifs", arg("query")),
        "giphy/get_random_gif" => "🎲 <b>Picking a random gif</b>".into(),
        "giphy/get_trending_gifs" => "📈 <b>Checking trending gifs</b>".into(),
        "telegram/react" => "😄 <b>Reacting</b>".into(),
        "telegram/send_gif" => "🎞 <b>Sending a gif</b>".into(),
        "telegram/send_photo" => "🖼 <b>Sending a photo</b>".into(),
        "telegram/send_document" => "📎 <b>Sending a file</b>".into(),
        "telegram/send_code_page" => "📄 <b>Sending the code</b>".into(),
        "telegram/send_poll" => "📊 <b>Asking a poll</b>".into(),
        other => format!("⚙️ <b>{}</b>", markdown::escape(&humanize_tool_name(other))),
    }
}

fn humanize_tool_name(name: &str) -> String {
    match name.split_once('/') {
        Some((server, tool)) => format!("{}: {}", server, tool.replace('_', " ")),
        None => name.replace('_', " "),
    }
}

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
    "😄",
    "😆",
    "😅",
    "😍",
    "😘",
    "😜",
    "😎",
    "🙂",
    "😊",
    "😇",
    "😡",
    "😤",
    "🥰",
    "🥲",
    "😬",
    "😐",
    "🙃",
    "😏",
    "😲",
    "😳",
    "😔",
    "😮",
    "😴",
    "🥳",
    "😋",
    "😝",
    "😗",
    "🥺",
    "🤗",
    "🤨",
    "🙌",
    "👋",
    "😶‍🌫️",
    "😓",
    "😠",
    "⭐",
    "🧡",
    "💔",
    "🖤",
    "🥵",
    "🥶",
    "😈",
];

const GIF_LIMIT: usize = 3;
const LINK_BUTTON_LIMIT: usize = 3;
const BUTTON_LABEL_LIMIT: usize = 40;

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

/// A reply, split to fit, with its links as buttons under the last piece.
async fn send_reply_chunks(api: &Api, chat_id: i64, text: &str, reply_to: Option<i64>) {
    let buttons = link_buttons(text);
    let chunks = markdown::to_html_chunks(text, CHUNK_LIMIT);
    let last = chunks.len().saturating_sub(1);
    for (i, chunk) in chunks.iter().enumerate() {
        if i == last {
            api.send_links_reply(chat_id, chunk, reply_to, &buttons)
                .await;
        } else {
            api.send_html_or_plain_reply(chat_id, chunk, reply_to).await;
        }
    }
}

/// The links in a reply, as buttons to put under it. Telegram renders a URL
/// inside code as plain text, so a link the model wrote that way cannot be
/// tapped on a phone; a button is the only thing that opens it.
fn link_buttons(reply: &str) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    let mut push = |label: Option<&str>, url: &str| {
        let url = url.trim();
        if !is_web_url(url) || is_gif_url(url) || found.len() >= LINK_BUTTON_LIMIT {
            return;
        }
        if found.iter().any(|(_, u)| u == url) {
            return;
        }
        let label = label
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| clip(l, BUTTON_LABEL_LIMIT))
            .unwrap_or_else(|| format!("Open {}", clip(&pretty_url(url), BUTTON_LABEL_LIMIT - 5)));
        found.push((label, url.to_string()));
    };

    // A labelled link names itself; anything else is named after where it goes.
    let mut rest = reply;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find("](") else {
            rest = after;
            continue;
        };
        let tail = &after[close + 2..];
        let Some(end) = tail.find(')') else {
            rest = tail;
            continue;
        };
        push(Some(&after[..close]), &tail[..end]);
        rest = &tail[end + 1..];
    }
    for token in reply.split_whitespace() {
        let token = token.trim_matches(|c: char| "`\"'()[]<>,.;:!?".contains(c));
        push(None, token);
    }
    found
}

fn is_web_url(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://")) && url.len() > 10
}

/// A URL as a button reads best as the place it goes, not the scheme and path
/// that get it there.
fn pretty_url(url: &str) -> String {
    let bare = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    bare.to_string()
}

fn clip(text: &str, max: usize) -> String {
    let flat = text.replace('\n', " ");
    if flat.chars().count() <= max {
        return flat;
    }
    let cut: String = flat.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

fn is_gif_url(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://"))
        && (url.contains(".gif")
            || url.contains("giphy.com/media")
            || url.contains("media.tenor.com"))
}

fn approval_subject(preview: &str) -> String {
    let subject = preview.strip_prefix("run ").unwrap_or(preview).trim();
    match subject.strip_prefix('`').and_then(|s| s.split_once('`')) {
        Some((command, rest)) => format!("{command}{rest}"),
        None => subject.to_string(),
    }
}

fn short_path(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_string()
}

fn pretty_query(query: &str) -> String {
    let terms: Vec<&str> = query
        .split('|')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();
    match terms.as_slice() {
        [] | [_] => truncate(query, 48),
        [first, rest @ ..] => format!("{} +{} more", truncate(first, 40), rest.len()),
    }
}

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

/// The prompt without the `[msg N]` line the bridge put in front for the model.
fn untagged(prompt: &str) -> &str {
    match prompt.strip_prefix("[msg ") {
        Some(rest) => rest.split_once('\n').map(|(_, text)| text).unwrap_or(""),
        None => prompt,
    }
}

fn console_text(text: &str, limit: usize) -> String {
    let flattened = text.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate(&flattened, limit)
}

fn console_tool(name: &str, arguments: &str) -> String {
    let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let field = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| args.get(k).and_then(Value::as_str))
            .unwrap_or("")
            .to_string()
    };
    let target = match name {
        "read_file" | "list_files" | "edit_file" | "find_files" => field(&["path"]),
        "search_files" => field(&["query", "pattern", "regex"]),
        "run_command" => field(&["description", "command"]),
        "update_plan" | "ask_user" | "remember" => field(&["label", "question", "title"]),
        _ => String::new(),
    };
    let target = truncate(&target, 120);
    if target.is_empty() {
        name.to_string()
    } else {
        format!("{name}: {target}")
    }
}

fn callback_message_ids(callback: &Value) -> Option<(i64, i64)> {
    let message = callback.get("message")?;
    let chat_id = message.get("chat")?.get("id")?.as_i64()?;
    let message_id = message.get("message_id")?.as_i64()?;
    Some((chat_id, message_id))
}

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

/// A reply whose parent is gone must still send, so the parent is a hint.
fn add_reply(payload: &mut Value, reply_to: Option<i64>) {
    if let Some(message_id) = reply_to.filter(|id| *id > 0) {
        payload["reply_parameters"] = json!({
            "message_id": message_id,
            "allow_sending_without_reply": true,
        });
    }
}

/// `/mirror` starts the phone's screen mirror and replies with a tappable URL;
/// `/mirror off` stops it. asterdroid only: it shells out to the `asterctl`
/// binary the app installs, which desktop bridges do not have.
///
/// The URL names the phone from outside (its LAN/Tailscale address), never
/// `127.0.0.1`, which would only open on the phone itself.
#[cfg(target_os = "android")]
async fn mirror_command(api: &Api, cfg: &TelegramConfig, chat_id: i64, arg: &str) {
    const PORT: u16 = 7070;
    // Serve on the same address the reply links to. asterctl binds loopback
    // unless told otherwise, which puts the mirror where only the phone can
    // open it while the link says otherwise, so the tap times out.
    let ip = mirror_ip();
    let bind = ip.unwrap_or(std::net::IpAddr::from([127, 0, 0, 1]));
    let addr = std::net::SocketAddr::new(bind, PORT);
    let url = mirror_url_for(ip, PORT);
    let pid_file = cfg.repo_root.join(".mirror.pid");
    if arg == "off" {
        if !mirror_port_state(addr).is_ours() {
            api.send_text(chat_id, "The mirror isn't running.").await;
            return;
        }
        let pid = std::fs::read_to_string(&pid_file).ok();
        if let Some(pid) = pid {
            let _ = tokio::process::Command::new("kill")
                .args([pid.trim()])
                .output()
                .await;
        }
        let _ = std::fs::remove_file(&pid_file);
        api.send_text(chat_id, "Mirror stopped.").await;
        return;
    }
    let state = mirror_port_state(addr);
    if state.is_ours() {
        api.send_links_reply(
            chat_id,
            &format!("Mirror is up. Open it here: <code>{url}</code>"),
            None,
            &[("Open mirror".into(), url.clone())],
        )
        .await;
        return;
    }
    if state.is_listening() {
        api.send_text(
            chat_id,
            &format!(
                "Port {PORT} is held by another app, so the mirror can't start. Stop that \
                 app (or change its port) and try /mirror again."
            ),
        )
        .await;
        return;
    }
    // A mirror from an older build is still holding the screen capture on an
    // address this one is not about to use, so it goes first.
    if let Ok(stale) = std::fs::read_to_string(&pid_file) {
        let _ = tokio::process::Command::new("kill")
            .args([stale.trim()])
            .output()
            .await;
        let _ = std::fs::remove_file(&pid_file);
    }
    let bin = cfg.repo_root.join("bin").join("asterctl");
    let log = std::fs::File::create(cfg.repo_root.join(".mirror.log"));
    let spawned = tokio::process::Command::new(&bin)
        .arg("serve")
        .arg(PORT.to_string())
        .args(["--bind", &bind.to_string()])
        // Without this asterctl re-execs itself and exits, so the pid recorded
        // below is a process that is already gone and /mirror off kills
        // nothing. This spawn is already detached from the turn.
        .arg("--foreground")
        .stdout(std::process::Stdio::null())
        .stderr(log.map(|f| f.into()).unwrap_or(std::process::Stdio::null()))
        .spawn();
    match spawned {
        Ok(child) => {
            if let Some(pid) = child.id() {
                let _ = std::fs::write(&pid_file, pid.to_string());
            }
            for _ in 0..30 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if mirror_port_state(addr).is_ours() {
                    let reach = match ip {
                        Some(_) => String::new(),
                        None => "\nThis phone has no address anyone else can reach right now, \
                                 so the link only opens on the phone itself."
                            .into(),
                    };
                    api.send_links_reply(
                        chat_id,
                        &format!(
                            "Mirror is up. Open it here: <code>{url}</code>\n/mirror off \
                             stops it.{reach}"
                        ),
                        None,
                        &[("Open mirror".into(), url.clone())],
                    )
                    .await;
                    return;
                }
            }
            api.send_text(
                chat_id,
                "The mirror started but isn't answering yet. Try again in a moment.",
            )
            .await;
        }
        Err(e) => {
            api.send_text(chat_id, &format!("Couldn't start the mirror: {e}"))
                .await;
        }
    }
}

/// What is on the mirror port: nothing, the mirror, or a squatter. A bare
/// connect cannot tell the last two apart, and the difference is the difference
/// between "already up" and a port conflict the user has to resolve.
#[cfg(target_os = "android")]
enum MirrorPort {
    Free,
    Ours,
    Squatted,
}

#[cfg(target_os = "android")]
impl MirrorPort {
    fn is_listening(&self) -> bool {
        !matches!(self, MirrorPort::Free)
    }

    fn is_ours(&self) -> bool {
        matches!(self, MirrorPort::Ours)
    }
}

#[cfg(target_os = "android")]
fn mirror_port_state(addr: std::net::SocketAddr) -> MirrorPort {
    use std::io::{Read, Write};

    let Ok(mut stream) = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(300))
    else {
        return MirrorPort::Free;
    };
    // The mirror answers any HTTP request with a page; a non-HTTP listener
    // (or one that closes on a malformed request) does not.
    let request = b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n";
    if stream.write_all(request).is_err() {
        return MirrorPort::Squatted;
    }
    let mut buf = [0u8; 16];
    let read = stream.read(&mut buf).unwrap_or(0);
    match read {
        0 => MirrorPort::Squatted,
        _ if buf.starts_with(b"HTTP/") => MirrorPort::Ours,
        _ => MirrorPort::Squatted,
    }
}

/// The phone's address from outside itself, which is both where the mirror
/// binds and what the reply links to. The Tailscale route is probed first so
/// it opens from a laptop on any network; the LAN route is the fallback.
#[cfg(target_os = "android")]
fn mirror_ip() -> Option<std::net::IpAddr> {
    let route = |to: &str| {
        std::net::UdpSocket::bind("0.0.0.0:0")
            .and_then(|socket| {
                socket.connect(to)?;
                socket.local_addr()
            })
            .map(|addr| addr.ip())
            .ok()
            .filter(|ip| !ip.is_loopback())
    };
    pick_mirror_ip(route("100.100.100.100:80"), route("8.8.8.8:80"))
}

/// Prefer the Tailscale address over the LAN one: the browser reading the
/// mirror is usually not on the phone's wifi.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn pick_mirror_ip(
    tailscale: Option<std::net::IpAddr>,
    lan: Option<std::net::IpAddr>,
) -> Option<std::net::IpAddr> {
    match tailscale {
        Some(std::net::IpAddr::V4(v4)) if is_tailscale_ip(v4) => tailscale,
        _ => lan.or(tailscale),
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn is_tailscale_ip(ip: std::net::Ipv4Addr) -> bool {
    let [first, second, _, _] = ip.octets();
    first == 100 && (64..=127).contains(&second)
}

/// Build the reply URL from a discovered address, refusing loopback: a
/// loopback or missing address falls back to the hostname. Shared with the
/// tests, which run on every target; the android caller is the only runtime user.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn mirror_url_for(ip: Option<std::net::IpAddr>, port: u16) -> String {
    match ip {
        Some(std::net::IpAddr::V4(ip)) if !ip.is_loopback() => format!("http://{ip}:{port}"),
        _ => format!(
            "http://{}:{port}",
            std::env::var("HOSTNAME").unwrap_or_default()
        ),
    }
}

#[cfg(not(target_os = "android"))]
async fn mirror_command(api: &Api, _cfg: &Arc<TelegramConfig>, chat_id: i64, _arg: &str) {
    api.send_text(chat_id, "/mirror only works on asterdroid.")
        .await;
}

fn help(cfg: &TelegramConfig) -> String {
    let mirror_line = if cfg!(target_os = "android") {
        "/mirror - share this phone's screen in a browser\n/mirror off - stop it\n"
    } else {
        ""
    };
    format!(
        "<b>Aster</b>\n\
         Send a message to run the agent on <code>{}</code>.\n\
         Approvals arrive as buttons; the work streams live.\n\n\
         /new - start fresh\n\
         /clear - same as /new\n\
         /stop - cancel the running turn\n\
         /mode - how the agent acts (plan, manual, auto, edit, yolo)\n\
         /model - switch the model for this chat\n\
         /effort - how much it thinks before answering (off, low, medium, high)\n\
         /status - session, mode, model, and history\n\
         /diff - uncommitted changes in the repo\n\
         /commit - draft a commit message and commit\n\
         {mirror_line}\
         /help - this message\n\n\
         Installed skills show up as /commands too.",
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

    /// A photo's temporary download URL, valid for one hour.
    async fn get_file(&self, file_id: &str) -> Result<String> {
        let response = self.call("getFile", json!({ "file_id": file_id })).await?;
        Ok(response["file_path"]
            .as_str()
            .context("getFile returned no file_path")?
            .to_string())
    }

    /// Fetch a file by its `getFile` path; the token stays inside `base`.
    async fn download(&self, file_path: &str) -> Result<Vec<u8>> {
        let url = format!(
            "{}/file/{}",
            self.base.replace("/bot", "/file/bot"),
            file_path
        );
        let bytes = self
            .http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        Ok(bytes.to_vec())
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

    /// A PNG as an inline photo, so a screenshot shows in the chat rather
    /// than arriving as a file to open.
    pub(crate) async fn send_photo_file(
        &self,
        chat_id: i64,
        path: &str,
        caption: &str,
    ) -> Result<Value> {
        let bytes = tokio::fs::read(path)
            .await
            .with_context(|| format!("reading {path}"))?;
        let filename = std::path::Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "shot.png".into());
        let form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .text("caption", truncate(caption, 1000))
            .part(
                "photo",
                reqwest::multipart::Part::bytes(bytes).file_name(filename),
            );
        let response: Value = self
            .http
            .post(format!("{}/sendPhoto", self.base))
            .multipart(form)
            .send()
            .await?
            .json()
            .await?;
        unwrap_result("sendPhoto", response)
    }

    async fn register_commands(&self, skills: &Skills) {
        let mut commands = vec![
            json!({"command": "new", "description": "Start fresh"}),
            json!({"command": "stop", "description": "Cancel the running turn"}),
            json!({"command": "mode", "description": "How the agent acts (plan/manual/auto/edit/yolo)"}),
            json!({"command": "model", "description": "Switch the model for this chat"}),
            json!({"command": "effort", "description": "How much it thinks before answering"}),
            json!({"command": "status", "description": "Session, mode, model, and history"}),
            json!({"command": "diff", "description": "Uncommitted changes in the repo"}),
            json!({"command": "help", "description": "How this bot works"}),
        ];
        commands
            .push(json!({"command": "skills", "description": "Browse and run installed skills"}));
        commands
            .push(json!({"command": "commit", "description": "Draft a commit message and commit"}));
        // The menu is a shortlist, not a catalog: /skills browses the rest.
        let mut names: Vec<&String> = skills.keys().collect();
        names.sort();
        for name in names.into_iter().take(SKILL_MENU_LIMIT) {
            let skill = &skills[name];
            let description = if skill.description.trim().is_empty() {
                format!("Run the {} skill", skill.name)
            } else {
                truncate(&skill.description, 250)
            };
            commands.push(json!({"command": name, "description": description}));
        }
        let payload = json!({ "commands": commands });
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
        self.send_text_reply(chat_id, text, None).await;
    }

    pub(crate) async fn send_text_reply(&self, chat_id: i64, text: &str, reply_to: Option<i64>) {
        let mut payload = json!({ "chat_id": chat_id, "text": text });
        add_reply(&mut payload, reply_to);
        if let Err(e) = self.call("sendMessage", payload).await {
            tracing::warn!("sendMessage failed: {e:#}");
        }
    }

    async fn send_html(&self, chat_id: i64, html: &str) -> Option<i64> {
        self.send_html_reply(chat_id, html, None).await
    }

    async fn send_html_reply(
        &self,
        chat_id: i64,
        html: &str,
        reply_to: Option<i64>,
    ) -> Option<i64> {
        let mut payload = json!({
            "chat_id": chat_id,
            "text": html,
            "parse_mode": "HTML",
            "link_preview_options": { "is_disabled": true },
        });
        add_reply(&mut payload, reply_to);
        match self.call("sendMessage", payload).await {
            Ok(message) => message.get("message_id").and_then(Value::as_i64),
            Err(e) => {
                tracing::warn!("sendMessage (html) failed: {e:#}");
                None
            }
        }
    }

    async fn send_html_or_plain(&self, chat_id: i64, html: &str) {
        self.send_html_or_plain_reply(chat_id, html, None).await;
    }

    async fn send_html_or_plain_reply(&self, chat_id: i64, html: &str, reply_to: Option<i64>) {
        if self
            .send_html_reply(chat_id, html, reply_to)
            .await
            .is_none()
        {
            self.send_text_reply(chat_id, html, reply_to).await;
        }
    }

    /// React to a message so a quiet event reads without adding a message.
    pub(crate) async fn react(&self, chat_id: i64, message_id: i64, emoji: &str) -> Result<Value> {
        let payload = json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "reaction": [{ "type": "emoji", "emoji": emoji }],
        });
        self.call("setMessageReaction", payload).await
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

    /// The reply with its links as buttons under it. A keyboard Telegram
    /// refuses must not cost the message, so a failure resends it plain.
    async fn send_links_reply(
        &self,
        chat_id: i64,
        html: &str,
        reply_to: Option<i64>,
        buttons: &[(String, String)],
    ) {
        if buttons.is_empty() {
            self.send_html_or_plain_reply(chat_id, html, reply_to).await;
            return;
        }
        let keyboard: Vec<Value> = buttons
            .iter()
            .map(|(label, url)| json!([{ "text": label, "url": url }]))
            .collect();
        let mut payload = json!({
            "chat_id": chat_id,
            "text": html,
            "parse_mode": "HTML",
            "reply_markup": { "inline_keyboard": keyboard },
        });
        add_reply(&mut payload, reply_to);
        if let Err(e) = self.call("sendMessage", payload).await {
            tracing::warn!("sendMessage (links) failed: {e:#}");
            self.send_html_or_plain_reply(chat_id, html, reply_to).await;
        }
    }

    async fn send_keyboard(&self, chat_id: i64, html: &str, keyboard: Value) {
        self.send_keyboard_reply(chat_id, html, keyboard, None)
            .await;
    }

    async fn send_keyboard_reply(
        &self,
        chat_id: i64,
        html: &str,
        keyboard: Value,
        reply_to: Option<i64>,
    ) {
        let mut payload = json!({
            "chat_id": chat_id,
            "text": html,
            "parse_mode": "HTML",
            "reply_markup": { "inline_keyboard": keyboard },
        });
        add_reply(&mut payload, reply_to);
        if let Err(e) = self.call("sendMessage", payload).await {
            tracing::warn!("sendMessage failed: {e:#}");
        }
    }

    async fn send_animation(&self, chat_id: i64, url: &str) {
        let payload = json!({ "chat_id": chat_id, "animation": url });
        if let Err(e) = self.call("sendAnimation", payload).await {
            tracing::warn!("sendAnimation failed: {e:#}");
            self.send_text(chat_id, url).await;
        }
    }

    async fn send_typing(&self, chat_id: i64) {
        self.send_chat_action(chat_id, "typing").await;
    }

    pub(crate) async fn send_chat_action(&self, chat_id: i64, action: &str) {
        let payload = json!({ "chat_id": chat_id, "action": action });
        let _ = self.call("sendChatAction", payload).await;
    }

    async fn answer_callback(&self, callback_id: &str, text: &str) {
        let payload = json!({ "callback_query_id": callback_id, "text": text });
        if let Err(e) = self.call("answerCallbackQuery", payload).await {
            tracing::warn!("answerCallbackQuery failed: {e:#}");
        }
    }

    async fn delete_callback_message(&self, callback: &Value) {
        if let Some((chat_id, message_id)) = callback_message_ids(callback) {
            let payload = json!({ "chat_id": chat_id, "message_id": message_id });
            let _ = self.call("deleteMessage", payload).await;
        }
    }

    async fn edit_html_keyboard(&self, chat_id: i64, message_id: i64, html: &str, keyboard: Value) {
        let payload = json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": html,
            "parse_mode": "HTML",
            "reply_markup": { "inline_keyboard": keyboard },
        });
        if let Err(e) = self.call("editMessageText", payload).await {
            tracing::debug!("editMessageText failed: {e:#}");
        }
    }

    async fn send_reply_keyboard_reply(
        &self,
        chat_id: i64,
        html: &str,
        keyboard: Value,
        reply_to: Option<i64>,
    ) {
        let mut payload = json!({
            "chat_id": chat_id,
            "text": html,
            "parse_mode": "HTML",
            "reply_markup": keyboard,
        });
        add_reply(&mut payload, reply_to);
        if let Err(e) = self.call("sendMessage", payload).await {
            tracing::warn!("sendMessage failed: {e:#}");
        }
    }

    async fn settle_callback_message(&self, callback: &Value, text: &str) {
        if let Some((chat_id, message_id)) = callback_message_ids(callback) {
            let payload = json!({ "chat_id": chat_id, "message_id": message_id, "text": text });
            let _ = self.call("editMessageText", payload).await;
        }
    }
}

#[cfg(test)]
#[path = "tests/telegram_test.rs"]
mod tests;
