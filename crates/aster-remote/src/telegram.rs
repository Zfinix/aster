//! Telegram adapter: long-polls the Bot API, runs one agent turn per incoming
//! message, and relays approval prompts as inline keyboards. Tool calls stream
//! into a live-edited activity message so the chat mirrors the CLI.

use std::collections::{HashMap, VecDeque};
use std::env;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;

use crate::bridge::{Answer, Turn, TurnEvent, TurnOutcome, WireMessage, run_turn};
use crate::markdown;

const CHUNK_LIMIT: usize = 4000;

const ACTIVITY_WINDOW: usize = 6;

const ACTIVITY_EDIT_GAP: Duration = Duration::from_millis(1500);

const MAX_QUEUED: usize = 10;

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
The `telegram` MCP server gives you chat tools: react (emoji-react to the \
user's message; use sparingly), send_gif, send_photo, send_document (share a \
repo file), send_poll, and send_code_page (send long code or reports as a \
private, deletable chat attachment instead of flooding the chat). Prefer them \
over describing what you would send. \
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
    history: Vec<WireMessage>,
    pending: Option<Pending>,
    running: Option<AbortHandle>,
    queued: VecDeque<(i64, String)>,
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
        let (mode, model, effort) = load_settings(chat_id);
        state.mode = mode;
        state.model = model;
        state.effort = effort;
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

fn load_settings(chat_id: i64) -> (Option<String>, Option<String>, Option<String>) {
    let Some(path) = settings_path(chat_id) else {
        return (None, None, None);
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return (None, None, None);
    };
    let Ok(saved) = serde_json::from_str::<Value>(&raw) else {
        return (None, None, None);
    };
    let field = |key: &str| saved.get(key).and_then(Value::as_str).map(str::to_string);
    (field("mode"), field("model"), field("effort"))
}

fn save_settings(chats: &Chats, chat_id: i64) {
    let Some(path) = settings_path(chat_id) else {
        return;
    };
    let saved = {
        let mut chats = chats.lock().expect("chats lock");
        let state = chats.entry(chat_id).or_default();
        json!({ "mode": state.mode, "model": state.model, "effort": state.effort })
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
    let key = aster_ai::keys::resolve_key(&base).map(|(key, _)| key);
    let mut request = reqwest::Client::new()
        .get(format!("{}/models", base.trim_end_matches('/')))
        .timeout(Duration::from_secs(15));
    if let Some(key) = key {
        request = request.bearer_auth(key);
    }
    let body: Value = request.send().await?.json().await?;
    let mut models: Vec<String> = body
        .get("data")
        .and_then(Value::as_array)
        .map(|data| {
            data.iter()
                .filter_map(|m| m.get("id").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    models.sort();
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
    start_turn(api, cfg, chats, chat_id, message_id, trimmed);
}

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
                state.queued.clear();
            }
            api.send_text(chat_id, "Started a fresh conversation.")
                .await;
        }
        "stop" => {
            let (running, queued) = {
                let mut chats = chats.lock().expect("chats lock");
                let state = chats.entry(chat_id).or_default();
                let queued = state.queued.len();
                state.queued.clear();
                (state.running.take(), queued)
            };
            match running {
                Some(handle) => {
                    handle.abort();
                    let text = if queued > 0 {
                        format!("Stopped the current turn and dropped {queued} queued message(s).")
                    } else {
                        "Stopped the current turn.".to_string()
                    };
                    api.send_text(chat_id, &text).await;
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
            if arg == "default" {
                chat_state(chats, chat_id, |state| state.model = None);
                save_settings(chats, chat_id);
                api.send_text(chat_id, "Model reset to the configured default.")
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
                if busy { "working" } else { "idle" },
            );
            api.send_text(chat_id, &text).await;
        }
        "queue" | "queued" => {
            let (busy, items) = chat_state(chats, chat_id, |state| {
                (state.running.is_some(), state.queued.clone())
            });
            if items.is_empty() {
                api.send_text(
                    chat_id,
                    if busy {
                        "Nothing queued; the current turn is running."
                    } else {
                        "Nothing queued and nothing is running."
                    },
                )
                .await;
            } else {
                let count = items.len();
                let status = if busy {
                    format!("<b>{count} queued</b> · a turn is running")
                } else {
                    format!("<b>{count} queued</b>, next up")
                };
                let mut lines = vec![status, String::new()];
                for (n, (_, text)) in items.iter().enumerate() {
                    lines.push(format!(
                        "<b>{}.</b> <code>{}</code>",
                        n + 1,
                        markdown::escape(&console_text(text, 160))
                    ));
                }
                api.send_html_or_plain(chat_id, &lines.join("\n")).await;
            }
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
        "skills" => send_skill_picker(api, skills, chat_id, arg, 0, None).await,
        "commit" => send_commit_proposal(api, cfg, chats, chat_id, arg).await,
        other => {
            // Installed skills are commands too: /rust_review -> that skill.
            if let Some(skill) = skills.get(other) {
                let prompt = skill_prompt(skill, arg);
                start_turn(api, cfg, chats, chat_id, message_id, &prompt);
            } else {
                api.send_text(chat_id, "Unknown command; /help lists what I know.")
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
        api.send_text(chat_id, "Nothing to commit; the tree is clean.")
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
            api.send_text(chat_id, &format!("Could not draft a message: {e:#}"))
                .await;
            return;
        }
    };
    let subject = message.lines().next().unwrap_or_default().to_string();
    if subject.is_empty() {
        api.send_text(chat_id, "The model returned an empty message.")
            .await;
        return;
    }

    let scope = if staged {
        "staged changes"
    } else {
        "all changes"
    };
    let text = format!(
        "<b>Commit</b> — {scope}\n<pre>{}</pre>\n{}",
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
        Some(model) => format!("<b>Model</b> — now {}", markdown::escape(&model)),
        None => "<b>Model</b> — now the configured default".to_string(),
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
    Some(format!("Set to {arg}."))
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
        // A turn is already going; queue rather than dropping the message. The
        // queue drains in order at the end of each successful turn.
        let (accepted, depth) = chat_state(chats, chat_id, |state| {
            if state.queued.len() >= MAX_QUEUED {
                (false, state.queued.len())
            } else {
                state.queued.push_back((message_id, prompt.to_string()));
                (true, state.queued.len())
            }
        });
        let text = if accepted && depth == 1 {
            "Working on your previous message; this one is queued and will run next.".to_string()
        } else if accepted {
            format!("Queued as number {depth}; I'll run it after the current ones.")
        } else {
            format!("The queue is full (max {MAX_QUEUED}); /stop the current turn first.")
        };
        // Fire the notice from its own task so start_turn never awaits; the
        // notice is fire-and-forget and the turn body below spawns its own task.
        let api = api.clone();
        tokio::spawn(async move { api.send_text(chat_id, &text).await });
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
    eprintln!("[{chat_id}] user: {}", console_text(prompt, 200));

    let api = api.clone();
    let chats = chats.clone();
    let repo_root = cfg.repo_root.clone();
    let cfg = cfg.clone();
    tokio::spawn(async move {
        let result = drive_turn(&api, &chats, chat_id, events_rx, turn_task).await;
        finish_turn(&api, &cfg, &chats, chat_id, Some(&repo_root), result).await;
    });
}

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
    let mut plan_id: Option<i64> = None;
    while let Some(event) = events.recv().await {
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
                activity.push(id, tool_line(&name, &arguments));
                activity.flush(false).await;
                eprintln!("[{chat_id}] → {}", console_tool(&name, &arguments));
            }
            TurnEvent::ToolResult { id, error } => {
                activity.complete(&id, error);
                activity.flush(false).await;
                eprintln!(
                    "[{chat_id}]   {}",
                    if error { "✗ tool failed" } else { "✓ ok" }
                );
            }
            TurnEvent::ApprovalRequest {
                preview,
                scope,
                respond,
            } => {
                activity.flush(true).await;
                let subject = approval_subject(&preview);
                let mut text = format!(
                    "<b>Approval needed</b>\n<pre>{}</pre>",
                    markdown::escape(&truncate(&subject, 3000))
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
                api.send_reply_keyboard(chat_id, &text, keyboard).await;
                set_pending(chats, chat_id, Pending::Question(respond));
            }
        }
    }
    let result = turn_task
        .await
        .unwrap_or_else(|e| Err(anyhow!("turn cancelled: {e}")));
    typing.abort();
    activity.finish(result.is_ok()).await;
    match &result {
        Ok(outcome) => eprintln!(
            "[{chat_id}] ✔ done: {}",
            console_text(outcome.reply.trim(), 200)
        ),
        Err(e) => eprintln!("[{chat_id}] ✗ turn failed: {e:#}"),
    }
    result
}

async fn finish_turn(
    api: &Api,
    cfg: &Arc<TelegramConfig>,
    chats: &Chats,
    chat_id: i64,
    repo_root: Option<&std::path::Path>,
    result: Result<TurnOutcome>,
) {
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
    let ok = result.is_ok();
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
                send_edit_diff(api, chat_id, repo_root, &outcome.edits).await;
            }
        }
        Err(e) => {
            api.send_text(chat_id, &format!("Turn failed: {e:#}")).await;
        }
    }
    // This turn is done and running is cleared, so start the next queued
    // message. Only successful turns drain: a failed turn leaves the queue
    // sitting for /stop or /new to clear.
    if ok {
        drain_queued(api, cfg, chats, chat_id).await;
    }
}

async fn drain_queued(api: &Api, cfg: &Arc<TelegramConfig>, chats: &Chats, chat_id: i64) {
    loop {
        let next = {
            let mut chats = chats.lock().expect("chats lock");
            let state = chats.entry(chat_id).or_default();
            if state.running.is_some() {
                None
            } else {
                state
                    .queued
                    .pop_front()
                    .map(|item| (item, state.queued.len()))
            }
        };
        match next {
            Some(((message_id, prompt), remaining)) => {
                api.send_text(
                    chat_id,
                    &format!(
                        "▶ Running your queued message ({} left): {}",
                        remaining,
                        console_text(&prompt, 200)
                    ),
                )
                .await;
                start_turn(api, cfg, chats, chat_id, message_id, &prompt);
            }
            None => break,
        }
    }
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
        let note = format!("Mode set to {choice}. It applies from the next message.");
        api.answer_callback(callback_id, &note).await;
        api.settle_callback_message(callback, &note).await;
        return;
    }
    if let Some(choice) = data.strip_prefix("e:").filter(|c| EFFORTS.contains(c)) {
        chat_state(chats, chat_id, |state| {
            state.effort = Some(choice.to_string())
        });
        save_settings(chats, chat_id);
        let note = format!("Effort set to {choice}.");
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
            api.answer_callback(callback_id, "Cancelled").await;
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
        let note = format!("Model set to {model}.");
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
            ("Allowed", true)
        }
        (Some(Pending::Approval { respond, .. }), "a:always") => {
            let _ = respond.send(Answer::AlwaysAllow);
            ("Always allowed", true)
        }
        (Some(Pending::Approval { subject, respond }), "a:deny") => {
            let _ = respond.send(Answer::Deny);
            // A denial has no step to show, so it leaves a line behind.
            api.settle_callback_message(callback, &format!("🚫 {subject}"))
                .await;
            ("Denied", false)
        }
        (None, _) => ("This prompt already expired.", false),
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
    message_id: Option<i64>,
    lines: Vec<Step>,
    last_flush: Instant,
}

struct Step {
    id: String,
    emoji: String,
    label: String,
    status: Status,
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

    fn push(&mut self, id: String, line: String) {
        let (emoji, label) = match line.split_once(' ') {
            Some((emoji, label)) => (emoji.to_string(), label.to_string()),
            None => (String::from("◦"), line),
        };
        self.lines.push(Step {
            id,
            emoji,
            label,
            status: Status::Running,
        });
    }

    fn complete(&mut self, id: &str, error: bool) {
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
        }
    }

    fn render(&self, header: &str) -> String {
        let mut text = String::from(header);
        let hidden = self.lines.len().saturating_sub(ACTIVITY_WINDOW);
        if hidden > 0 {
            text.push_str(&format!("\n…  {hidden} earlier steps"));
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
            text.push('\n');
            text.push_str(visible[i].marker());
            text.push(' ');
            text.push_str(&visible[i].label);
            if run > 1 {
                text.push_str(&format!(" ×{run}"));
            }
            i += run;
        }
        text
    }
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

const DIFF_INLINE_LIMIT: usize = 3_000;

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
        "read_file" => step("📖", "Read", &short_path(&field(&["path"]))),
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
            // The model's summary takes the verb's place: it already reads as one.
            let summary = field(&["description"]);
            if !summary.is_empty() {
                return format!("🖥 <b>{}</b>", markdown::escape(&truncate(&summary, 80)));
            }
            let mut cmd = field(&["command"]);
            if let Some(args) = args.get("args").and_then(Value::as_array) {
                let extra: Vec<&str> = args.iter().filter_map(Value::as_str).collect();
                if !extra.is_empty() {
                    cmd.push(' ');
                    cmd.push_str(&extra.join(" "));
                }
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
        "telegram/send_code_page" => "📄 <b>Publishing a code page</b>".into(),
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
         /queued — list messages waiting while I'm busy\n\
         /diff — uncommitted changes in the repo\n\
         /commit — draft a commit message and commit\n\
         /help — this message\n\n\
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

    async fn register_commands(&self, skills: &Skills) {
        let mut commands = vec![
            json!({"command": "new", "description": "Start a fresh conversation"}),
            json!({"command": "stop", "description": "Cancel the running turn"}),
            json!({"command": "mode", "description": "How the agent acts (plan/manual/auto/edit/yolo)"}),
            json!({"command": "model", "description": "Switch the model for this chat"}),
            json!({"command": "effort", "description": "Reasoning budget (off/low/medium/high)"}),
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
        let payload = json!({ "chat_id": chat_id, "text": text });
        if let Err(e) = self.call("sendMessage", payload).await {
            tracing::warn!("sendMessage failed: {e:#}");
        }
    }

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

    async fn send_reply_keyboard(&self, chat_id: i64, html: &str, keyboard: Value) {
        let payload = json!({
            "chat_id": chat_id,
            "text": html,
            "parse_mode": "HTML",
            "reply_markup": keyboard,
        });
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
