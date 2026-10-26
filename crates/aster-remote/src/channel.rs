//! Shared session core for remote channels: per-chat state, saved overrides,
//! and skill commands. Channel-specific rendering stays in each provider.

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::task::AbortHandle;

use crate::bridge::WireMessage;

pub const MODES: &[&str] = &["plan", "manual", "auto", "edit", "yolo"];

/// How a working directory is named to a remote user. A basename on its own is
/// ambiguous between two repos called `api` and says nothing at all on Android,
/// where every app's storage ends in `files`, so the path is shown whole and
/// only the home prefix is folded to `~`.
pub fn workspace_name(root: &std::path::Path) -> String {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|h| !h.as_os_str().is_empty());
    match home
        .as_deref()
        .and_then(|home| root.strip_prefix(home).ok())
    {
        Some(rel) if rel.as_os_str().is_empty() => "~".to_string(),
        Some(rel) => format!("~/{}", rel.display()),
        None => root.display().to_string(),
    }
}

pub enum Pending {
    Approval {
        respond: tokio::sync::oneshot::Sender<crate::bridge::Answer>,
    },
    Question(tokio::sync::oneshot::Sender<Option<String>>),
}

#[derive(Default)]
pub struct ChatState {
    pub history: Vec<WireMessage>,
    pub pending: Option<Pending>,
    pub running: Option<AbortHandle>,
    pub stopped: bool,
    /// Side turns running next to the main turn, one slot per entry.
    pub workers: Vec<Option<WorkerTurn>>,
    pub mode: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub loaded: bool,
}

pub type Chats = Arc<std::sync::Mutex<HashMap<i64, ChatState>>>;

/// One side turn running next to the chat's main turn.
pub struct WorkerTurn {
    pub abort: AbortHandle,
}

/// Each mid-turn message runs on its own agent process and session, up to
/// this many at once.
pub const MAX_WORKERS: usize = 8;

pub const WORKER_NOTE: &str = "Another task is running in this repo at the same time. Keep your \
     changes scoped to exactly what was asked, and do not touch files the other task might be \
     editing. If you need an approval or an answer, work around it rather than waiting.";

/// First free slot: 0 is the main turn, 1.. are workers.
pub fn next_free_slot(chats: &Chats, channel: &str, chat_id: i64) -> Option<usize> {
    chat_state(chats, channel, chat_id, |state| {
        if state.running.is_none() {
            return Some(0);
        }
        while state.workers.len() < MAX_WORKERS {
            state.workers.push(None);
        }
        state
            .workers
            .iter()
            .position(Option::is_none)
            .map(|slot| slot + 1)
    })
}

/// Claim a worker slot; false if another turn took it first.
pub fn claim_worker_slot(
    chats: &Chats,
    channel: &str,
    chat_id: i64,
    slot: usize,
    abort: AbortHandle,
) -> bool {
    chat_state(chats, channel, chat_id, |state| {
        while state.workers.len() < MAX_WORKERS {
            state.workers.push(None);
        }
        if state.workers[slot].is_none() {
            state.workers[slot] = Some(WorkerTurn { abort });
            true
        } else {
            false
        }
    })
}

pub fn clear_worker_slot(chats: &Chats, channel: &str, chat_id: i64, slot: usize) {
    chat_state(chats, channel, chat_id, |state| {
        if slot < state.workers.len() {
            state.workers[slot] = None;
        }
    });
}

/// Abort every worker slot; returns how many were running.
pub fn abort_workers(chats: &Chats, channel: &str, chat_id: i64) -> usize {
    chat_state(chats, channel, chat_id, |state| {
        let running = state.workers.iter().flatten().count();
        for worker in state.workers.iter_mut().flatten() {
            worker.abort.abort();
        }
        state.workers.clear();
        running
    })
}

pub fn chat_state<T>(
    chats: &Chats,
    channel: &str,
    chat_id: i64,
    act: impl FnOnce(&mut ChatState) -> T,
) -> T {
    let mut chats = chats.lock().expect("chats lock");
    let state = chats.entry(chat_id).or_default();
    if !state.loaded {
        let (mode, model, effort) = load_settings(channel, chat_id);
        state.mode = mode;
        state.model = model;
        state.effort = effort;
        state.loaded = true;
    }
    act(state)
}

fn settings_path(channel: &str, chat_id: i64) -> Option<PathBuf> {
    let home = env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".aster/remote")
            .join(format!("{channel}-{chat_id}.json")),
    )
}

fn load_settings(channel: &str, chat_id: i64) -> (Option<String>, Option<String>, Option<String>) {
    let Some(path) = settings_path(channel, chat_id) else {
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

pub fn save_settings(chats: &Chats, channel: &str, chat_id: i64) {
    let Some(path) = settings_path(channel, chat_id) else {
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

pub struct SkillCommand {
    pub name: String,
}

pub type Skills = Arc<HashMap<String, SkillCommand>>;

pub fn discover_skill_commands(repo_root: &std::path::Path) -> HashMap<String, SkillCommand> {
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
        });
    }
    commands
}

pub fn set_override(
    chats: &Chats,
    channel: &str,
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
    chat_state(chats, channel, chat_id, |state| {
        *slot(state) = Some(arg.to_string());
    });
    save_settings(chats, channel, chat_id);
    Some(format!("Set to {arg}."))
}

pub fn get_override<T>(
    chats: &Chats,
    channel: &str,
    chat_id: i64,
    read: impl FnOnce(&ChatState) -> T,
) -> T {
    chat_state(chats, channel, chat_id, |state| read(state))
}

/// An approval preview as a chat shows it: the command alone, or the file
/// with what is done to it, and apart from it the rule that asked. A preview
/// that is not the policy's own passes through as written.
pub struct ApprovalView {
    pub subject: String,
    pub rule: Option<String>,
}

impl ApprovalView {
    pub fn new(preview: &str) -> Self {
        let preview = preview.trim();
        let (head, rule) = match preview
            .strip_suffix(')')
            .and_then(|rest| rest.rsplit_once(" ("))
        {
            Some((head, rule)) => (head, Some(rule.to_string())),
            None => (preview, None),
        };
        if let Some(command) = head.strip_prefix("run `").and_then(|c| c.strip_suffix('`')) {
            return Self {
                subject: command.to_string(),
                rule,
            };
        }
        if head.starts_with("edit ") || head.starts_with("read ") {
            return Self {
                subject: head.to_string(),
                rule,
            };
        }
        Self {
            subject: preview.to_string(),
            rule: None,
        }
    }

    /// The view as plain text, the rule on a line of its own.
    pub fn text(&self) -> String {
        match &self.rule {
            Some(rule) => format!("{}\nRule: {rule}", self.subject),
            None => self.subject.clone(),
        }
    }
}

/// Text as messages of at most `limit` bytes, every line kept as written.
/// Approvals and questions go this way: what the person agrees to has to be
/// exactly what runs, fences, asterisks and indentation included.
pub(crate) fn verbatim_chunks(text: &str, limit: usize) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    pack_lines(text.lines().map(str::to_string), limit)
}

/// Lines packed into chunks of at most `limit` bytes with nothing dropped:
/// joining the chunks with newlines gives the lines back, except that a line
/// longer than a chunk is split across several.
pub(crate) fn pack_lines(lines: impl Iterator<Item = String>, limit: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current: Option<String> = None;
    for line in lines {
        if line.len() > limit {
            chunks.extend(current.take());
            chunks.extend(split_long(&line, limit));
            continue;
        }
        current = match current.take() {
            Some(mut chunk) if chunk.len() + 1 + line.len() <= limit => {
                chunk.push('\n');
                chunk.push_str(&line);
                Some(chunk)
            }
            Some(chunk) => {
                chunks.push(chunk);
                Some(line)
            }
            None => Some(line),
        };
    }
    chunks.extend(current);
    chunks
}

/// A line cut into pieces of at most `limit` bytes that concatenate back to
/// it, each cut just after a space where one follows the indentation.
fn split_long(line: &str, limit: usize) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut rest = line;
    while rest.len() > limit {
        let mut cut = limit;
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        // A limit narrower than one character still moves on by one.
        if cut == 0 {
            cut = rest.chars().next().map_or(rest.len(), char::len_utf8);
        }
        if let Some(space) = rest[..cut]
            .rfind(' ')
            .filter(|at| !rest[..*at].trim().is_empty())
        {
            cut = space + 1;
        }
        pieces.push(rest[..cut].to_string());
        rest = &rest[cut..];
    }
    if !rest.is_empty() {
        pieces.push(rest.to_string());
    }
    pieces
}

pub fn truncate(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut cut = limit;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &text[..cut])
}

pub fn console_text(text: &str, limit: usize) -> String {
    let flattened = text.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate(&flattened, limit)
}

pub fn console_tool(name: &str, arguments: &str) -> String {
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

pub fn skill_prompt(skill: &SkillCommand, input: &str) -> String {
    let mut prompt = format!(
        "Load the skill `{}` with the read_skill tool and follow its instructions.",
        skill.name
    );
    if !input.is_empty() {
        prompt.push_str(&format!(" Input: {input}"));
    }
    prompt
}

#[cfg(test)]
#[path = "tests/channel_test.rs"]
mod tests;
