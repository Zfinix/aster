//! Telegram adapter: long-polls the Bot API, runs one agent turn per incoming
//! message, and relays approval prompts as inline keyboards. Tool calls stream
//! into a live-edited activity message so the chat mirrors the CLI.

use std::collections::HashMap;
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

/// Telegram caps messages at 4096 chars; leave headroom for tags.
const CHUNK_LIMIT: usize = 4000;

/// Steps one activity message holds before the next batch starts its own, so a
/// long turn reads as progress in the chat instead of one mutating block.
const ACTIVITY_WINDOW: usize = 6;

/// Minimum gap between edits of the activity message (Telegram rate limit).
const ACTIVITY_EDIT_GAP: Duration = Duration::from_millis(1500);

/// Reply-keyboard row that answers an agent question with "no answer".
const SKIP_LABEL: &str = "Skip";

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

/// A prompt waiting for the user. Approvals resolve via inline buttons;
/// questions resolve via the next text message (reply-keyboard tap or typed).
enum Pending {
    Approval {
        /// What is being approved, e.g. `git status`, kept to render the outcome.
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
    /// Per-chat overrides set with /mode, /model, and /effort.
    mode: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    /// Saved settings are read from disk the first time a chat is touched.
    loaded: bool,
    /// A drafted commit message awaiting confirmation.
    pending_commit: Option<PendingCommit>,
}

struct PendingCommit {
    message: String,
    /// Nothing was staged when the draft was made, so committing stages first.
    stage_all: bool,
}

/// Get a chat's state, restoring its saved settings on first use.
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

/// Where a chat's mode/model/effort live between bridge restarts.
fn settings_path(chat_id: i64) -> Option<PathBuf> {
    let home = env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".aster/remote")
            .join(format!("telegram-{chat_id}.json")),
    )
}

/// Load a chat's saved settings, if any. Missing or unreadable files are
/// simply "no overrides".
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

/// Persist a chat's settings so a bridge restart does not silently drop the
/// user back to the default mode.
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

/// An installed skill surfaced as a Telegram /command.
struct SkillCommand {
    name: String,
    description: String,
}

type Skills = Arc<HashMap<String, SkillCommand>>;

/// Discover skills from the repo and global roots, keyed by a valid Telegram
/// command name (lowercase, `a-z0-9_`, hyphens folded to underscores).
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

/// The provider's model catalog, fetched once per process for /model search.
async fn model_catalog() -> Result<&'static Vec<String>> {
    static MODEL_CACHE: OnceLock<Vec<String>> = OnceLock::new();
    if let Some(models) = MODEL_CACHE.get() {
        return Ok(models);
    }
    let base = env::var("ASTER_BASE_URL").unwrap_or_else(|_| "https://openrouter.ai/api/v1".into());
    let key = env::var("ASTER_API_KEY")
        .or_else(|_| env::var("OPEN_ROUTER_API_KEY"))
        .ok();
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
    start_turn(api, cfg, chats, chat_id, message_id, trimmed).await;
}

/// One /command, mirroring the TUI's command set where it makes sense remotely.
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
                start_turn(api, cfg, chats, chat_id, message_id, &prompt).await;
            } else {
                api.send_text(chat_id, "Unknown command; /help lists what I know.")
                    .await;
            }
        }
    }
}

const MODES: &[&str] = &["plan", "manual", "auto", "edit", "yolo"];
const EFFORTS: &[&str] = &["off", "low", "medium", "high"];

/// Models shown per page of the /model picker.
const MODEL_PAGE: usize = 8;

/// Skills listed in Telegram's command menu; the rest live behind /skills.
const SKILL_MENU_LIMIT: usize = 15;

/// Skills shown per page of the /skills picker.
const SKILL_PAGE: usize = 8;

/// Browse installed skills as buttons; tapping one runs it.
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

/// How much diff the commit-message prompt carries.
const COMMIT_DIFF_LIMIT: usize = 12_000;

/// Draft a commit message from the current diff in one model call, then offer
/// to commit it. A full agent turn would spend rounds re-reading the diff.
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

/// Stage if needed and commit, reporting what git said.
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

/// One tool-free model call in the bridge's repo.
async fn aster_remote_ask(cfg: &Arc<TelegramConfig>, prompt: &str) -> Result<String> {
    crate::bridge::ask_once(&cfg.bin, &cfg.repo_root, prompt).await
}

/// The prompt that runs one skill.
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

/// Show the model catalog as tappable buttons, paged. `filter` narrows the
/// list; `edit` replaces an existing picker message instead of sending a new one.
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
    chat_state(chats, chat_id, |state| {
        *slot(state) = Some(arg.to_string());
    });
    save_settings(chats, chat_id);
    Some(format!("Set to {arg}."))
}

fn get_override<T>(chats: &Chats, chat_id: i64, read: impl FnOnce(&ChatState) -> T) -> T {
    chat_state(chats, chat_id, |state| read(state))
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
    let repo_root = cfg.repo_root.clone();
    tokio::spawn(async move {
        let result = drive_turn(&api, &chats, chat_id, events_rx, turn_task).await;
        finish_turn(&api, &chats, chat_id, Some(&repo_root), result).await;
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
            }
            TurnEvent::ToolResult { id, error } => {
                activity.complete(&id, error);
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
                set_pending(chats, chat_id, Pending::Approval { subject, respond });
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
    result
}

/// Record the outcome and report it back to the chat.
async fn finish_turn(
    api: &Api,
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
        start_turn(api, cfg, chats, chat_id, 0, &prompt).await;
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

/// The live "what the agent is doing" message, edited in place as tools run.
struct Activity {
    api: Api,
    chat_id: i64,
    message_id: Option<i64>,
    lines: Vec<Step>,
    last_flush: Instant,
}

/// One step in the activity list, ticked off when its tool call returns.
/// The leading emoji is stored apart from the label so a finished step swaps
/// its tool emoji for a check rather than carrying both.
struct Step {
    /// Tool call id, so the matching result can complete this step.
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

    /// Tick off the step this result belongs to.
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

/// Diff budget for the post-edit message; longer diffs go out as a code page.
const DIFF_INLINE_LIMIT: usize = 3_000;

/// Show what actually changed. A list of file names says an edit happened; the
/// diff says whether it was the right one, which is the thing worth reviewing
/// from a phone.
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
    match crate::mcp_server::publish_telegraph_page("Changes", &diff).await {
        Ok(url) => api.send_text(chat_id, &url).await,
        Err(_) => {
            let text = format!(
                "<pre>{}</pre>",
                markdown::escape(&truncate(&diff, DIFF_INLINE_LIMIT))
            );
            api.send_html_or_plain(chat_id, &text).await;
        }
    }
}

/// Render an `update_plan` call as its own checklist message.
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
        "read_skill" => step("📚", "Skill", &field(&["name"])),
        "update_plan" => "📋 <b>Updating the plan</b>".into(),
        "exit_plan_mode" => "📋 <b>Plan ready</b>".into(),
        "ask_user" => "💬 <b>Asking you</b>".into(),
        other => mcp_line(other, &args, &step),
    }
}

/// Friendly label for an MCP tool id like `telegram/send_code_page`.
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

/// The thing being approved, without the policy's framing: `run \`git status\``
/// becomes `git status`, `edit src/lib.rs (protected path)` keeps its note.
fn approval_subject(preview: &str) -> String {
    let subject = preview.strip_prefix("run ").unwrap_or(preview).trim();
    match subject.strip_prefix('`').and_then(|s| s.split_once('`')) {
        Some((command, rest)) => format!("{command}{rest}"),
        None => subject.to_string(),
    }
}

/// Just the file name; full paths read as noise on a phone.
fn short_path(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_string()
}

/// Regex alternations read as noise in the feed; show the first term and count
/// the rest: `request_approval +2 more`.
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

fn callback_message_ids(callback: &Value) -> Option<(i64, i64)> {
    let message = callback.get("message")?;
    let chat_id = message.get("chat")?.get("id")?.as_i64()?;
    let message_id = message.get("message_id")?.as_i64()?;
    Some((chat_id, message_id))
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

    /// Show /new, /stop, and /help in Telegram's command menu.
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

    /// Remove a prompt once it has served its purpose.
    async fn delete_callback_message(&self, callback: &Value) {
        if let Some((chat_id, message_id)) = callback_message_ids(callback) {
            let payload = json!({ "chat_id": chat_id, "message_id": message_id });
            let _ = self.call("deleteMessage", payload).await;
        }
    }

    /// Replace a message's text and its inline keyboard in one call.
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

    /// Send text with a native reply keyboard (options above the text field).
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

    /// Replace the tapped message's text with the outcome; buttons go away.
    async fn settle_callback_message(&self, callback: &Value, text: &str) {
        if let Some((chat_id, message_id)) = callback_message_ids(callback) {
            let payload = json!({ "chat_id": chat_id, "message_id": message_id, "text": text });
            let _ = self.call("editMessageText", payload).await;
        }
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
        assert_eq!(line, "📖 <b>Read</b> <code>main.rs</code>");
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
    fn approval_subject_unwraps_the_run_preview() {
        assert_eq!(super::approval_subject("run `git status`"), "git status");
    }

    #[test]
    fn approval_subject_keeps_trailing_notes() {
        assert_eq!(
            super::approval_subject("run `rm -rf dist` (risky command)"),
            "rm -rf dist (risky command)"
        );
    }

    #[test]
    fn approval_subject_passes_through_edit_previews() {
        assert_eq!(
            super::approval_subject("edit src/lib.rs (protected path)"),
            "edit src/lib.rs (protected path)"
        );
    }

    #[test]
    fn plan_message_marks_the_step_in_flight() {
        let args = r#"{"steps":[
            {"label":"read the code","status":"done"},
            {"label":"write the fix","status":"in_progress"},
            {"label":"run tests","status":"pending"}]}"#;
        let plan = super::plan_message(args).expect("a plan");
        assert!(plan.contains("✅ read the code"));
        assert!(plan.contains("▶️ <b>write the fix</b>"));
        assert!(plan.contains("▫️ run tests"));
        assert!(plan.contains("1/3 done"));
    }

    #[test]
    fn plan_message_is_none_without_steps() {
        assert!(super::plan_message(r#"{"steps":[]}"#).is_none());
        assert!(super::plan_message("not json").is_none());
    }

    #[test]
    fn tool_line_shortens_deep_paths() {
        let line = tool_line(
            "read_file",
            r#"{"path":"crates/aster-policy/src/grants.rs"}"#,
        );
        assert_eq!(line, "📖 <b>Read</b> <code>grants.rs</code>");
    }

    #[test]
    fn tool_line_compresses_regex_alternations() {
        let line = tool_line(
            "search_files",
            r#"{"query":"request_approval|Answer::Always|fn allowed"}"#,
        );
        assert_eq!(
            line,
            "🔎 <b>Search</b> <code>request_approval +2 more</code>"
        );
    }

    #[test]
    fn tool_line_falls_back_to_name() {
        assert_eq!(tool_line("mystery", "{}"), "⚙️ <b>mystery</b>");
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        let text = "é".repeat(50);
        let cut = truncate(&text, 41);
        assert!(cut.ends_with('…'));
        assert!(cut.len() <= 44);
    }
}
