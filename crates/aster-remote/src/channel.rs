//! Shared session core for remote channels: per-chat state, saved overrides,
//! and skill commands. Channel-specific rendering stays in each provider.

use std::collections::{HashMap, VecDeque};
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::task::AbortHandle;

use crate::bridge::WireMessage;

pub const MODES: &[&str] = &["plan", "manual", "auto", "edit", "yolo"];

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
    pub queued: VecDeque<(i64, String)>,
    pub mode: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub loaded: bool,
}

pub type Chats = Arc<std::sync::Mutex<HashMap<i64, ChatState>>>;

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
