//! Saved chats as something the model can read. Asked for "the chat history"
//! it used to guess at `aster sessions` and get the syntax wrong, so this is
//! the same store behind a tool.

use std::path::{Path, PathBuf};

use aster_persist::{SessionTranscript, Store};
use chrono::{DateTime, Utc};

use crate::chat::SessionCtx;

/// Enough of a chat to answer from, cut at the older end: the last thing said
/// is what a question about the history is usually about.
const BUDGET: usize = 24_000;
const DEFAULT_LIMIT: usize = 20;

pub(crate) struct HistoryArgs<'a> {
    pub id: Option<&'a str>,
    pub query: Option<&'a str>,
    pub limit: Option<usize>,
    pub all: bool,
}

pub(crate) fn chat_history(ctx: &SessionCtx, repo_root: &Path, args: HistoryArgs<'_>) -> String {
    let Some(store) = ctx.store.as_ref() else {
        return "error: this run keeps no saved chats, so there is no history to read".to_string();
    };
    let current = ctx
        .recorder
        .as_ref()
        .and_then(|r| r.lock().ok().map(|writer| writer.id().to_string()));
    match args.id {
        Some(id) => show(store, repo_root, id, current.as_deref()),
        None => list(store, repo_root, current.as_deref(), &args),
    }
}

fn list(store: &Store, repo_root: &Path, current: Option<&str>, args: &HistoryArgs<'_>) -> String {
    let metas = if args.all {
        store.list_all_sessions()
    } else {
        store.list_sessions(repo_root)
    };
    let Ok(mut metas) = metas else {
        return "error: could not read the saved chats".to_string();
    };
    metas.sort_by_key(|m| std::cmp::Reverse(m.created_at));

    let needle = args.query.map(str::to_lowercase);
    let limit = args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 100);
    let mut rows = Vec::new();
    for meta in metas {
        let owner = PathBuf::from(&meta.repo_root);
        let Ok(transcript) = store.resume(&owner, &meta.id) else {
            continue;
        };
        let hit = needle.as_deref().map(|n| matches(&transcript, n));
        if hit == Some(None) {
            continue;
        }
        rows.push(row(
            &meta.id,
            &meta.created_at,
            &transcript,
            current,
            hit.flatten(),
        ));
        if rows.len() == limit {
            break;
        }
    }

    if rows.is_empty() {
        return match args.query {
            Some(q) => format!("No saved chat mentions {q:?}."),
            None => "No saved chats yet.".to_string(),
        };
    }
    let scope = if args.all {
        "every project"
    } else {
        "this repo"
    };
    format!(
        "{} saved {} in {scope}, newest first.\n\n{}\n\nCall chat_history again with `id` to read one; `current` reads this chat.",
        rows.len(),
        if rows.len() == 1 { "chat" } else { "chats" },
        rows.join("\n")
    )
}

fn row(
    id: &str,
    created: &DateTime<Utc>,
    transcript: &SessionTranscript,
    current: Option<&str>,
    hit: Option<String>,
) -> String {
    let turns = transcript.user_turn_count();
    let title = transcript.display_title().unwrap_or("untitled").trim();
    let mine = if current == Some(id) {
        "  [this chat]"
    } else {
        ""
    };
    let snippet = hit.map(|h| format!("\n    …{h}…")).unwrap_or_default();
    format!(
        "{id}  {}  {turns} {}  {}{mine}{snippet}",
        clip(title, 70),
        if turns == 1 { "turn" } else { "turns" },
        created.format("%Y-%m-%d %H:%M UTC"),
    )
}

/// The first line of a chat that mentions the query, so a search result says
/// why it matched rather than only that it did.
fn matches(transcript: &SessionTranscript, needle: &str) -> Option<String> {
    if transcript
        .display_title()
        .is_some_and(|t| t.to_lowercase().contains(needle))
    {
        return transcript.display_title().map(|t| clip(t.trim(), 90));
    }
    for message in transcript.messages() {
        let Some(content) = message.content.as_deref() else {
            continue;
        };
        let at = content.to_lowercase().find(needle)?;
        let start = content[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line = content[start..].lines().next().unwrap_or("").trim();
        return Some(clip(line, 90));
    }
    None
}

fn show(store: &Store, repo_root: &Path, id: &str, current: Option<&str>) -> String {
    let id = match (id, current) {
        ("current" | "this" | "", Some(mine)) => mine,
        ("current" | "this" | "", None) => {
            return "error: this chat is not being saved, so it has no transcript to read"
                .to_string();
        }
        (asked, _) => asked,
    };
    let Some(owner) = owner_of(store, repo_root, id) else {
        return format!(
            "error: no saved chat with id {id:?}; call chat_history with no `id` to list them"
        );
    };
    let Ok(transcript) = store.resume(&owner, id) else {
        return format!("error: could not read the chat {id:?}");
    };

    let head = format!(
        "Chat {id}  {}  {} turns  started {}\n",
        transcript.display_title().unwrap_or("untitled").trim(),
        transcript.user_turn_count(),
        transcript.meta.created_at.format("%Y-%m-%d %H:%M UTC"),
    );
    let body: Vec<String> = transcript.messages().filter_map(render).collect();
    format!("{head}\n{}", tail(&body, BUDGET))
}

/// One message as a line the model can read back: who said it, what they said,
/// and which tools the turn reached for. Tool results are left out; they are
/// the bulk of a transcript and rarely what the question is about.
fn render(message: &aster_persist::MessageEvent) -> Option<String> {
    if message.role == "tool" || message.role == "system" {
        return None;
    }
    let said = message.content.as_deref().unwrap_or("").trim();
    let calls: Vec<&str> = message
        .tool_calls
        .iter()
        .map(|c| c.function.name.as_str())
        .collect();
    if said.is_empty() && calls.is_empty() {
        return None;
    }
    let mut line = format!("{}: {}", message.role, clip(said, 2_000));
    if !calls.is_empty() {
        line.push_str(&format!("\n  ran {}", calls.join(", ")));
    }
    Some(line)
}

/// The end of a chat, not the start: history questions are about what just
/// happened, and the head is what a long transcript can afford to lose.
fn tail(lines: &[String], budget: usize) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut size = 0;
    for line in lines.iter().rev() {
        size += line.len() + 1;
        if size > budget && !kept.is_empty() {
            kept.reverse();
            return format!("[earlier messages left out]\n\n{}", kept.join("\n\n"));
        }
        kept.push(line);
    }
    kept.reverse();
    kept.join("\n\n")
}

fn owner_of(store: &Store, repo_root: &Path, id: &str) -> Option<PathBuf> {
    if store.resume(repo_root, id).is_ok() {
        return Some(repo_root.to_path_buf());
    }
    store.find_session_repo(id)
}

fn clip(text: &str, max: usize) -> String {
    let flat = text.replace('\n', " ⏎ ");
    if flat.chars().count() <= max {
        return flat;
    }
    let cut: String = flat.chars().take(max).collect();
    format!("{cut}…")
}

#[cfg(test)]
#[path = "tests/chat_history_test.rs"]
mod tests;
