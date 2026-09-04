//! Turn a session transcript into a bounded, model-ready digest: user turns,
//! assistant text, and terminal tool results, truncated to a hard cap so a
//! consolidation call never carries the raw transcript.

use chrono::{DateTime, Utc};

pub const MAX_DIGEST_CHARS: usize = 24_000;

/// One durable-signal message pulled from a transcript.
#[derive(Debug, Clone)]
pub struct DigestMessage {
    pub role: String,
    pub content: String,
    pub ts: DateTime<Utc>,
}

/// A bounded digest of one session: its id, title, the project root it ran in,
/// its user-turn count, and the durable-signal messages truncated to
/// [`MAX_DIGEST_CHARS`].
pub struct SessionDigest {
    pub session_id: String,
    pub title: Option<String>,
    pub repo_root: String,
    pub user_turn_count: usize,
    pub messages: Vec<DigestMessage>,
}

/// Build a digest from a loaded transcript, or `None` when the session has no
/// meaningful content (no user turns, or nothing durable after filtering).
pub fn build(transcript: &aster_persist::SessionTranscript) -> Option<SessionDigest> {
    let user_turn_count = transcript.user_turn_count();
    if user_turn_count == 0 {
        return None;
    }
    let mut messages: Vec<DigestMessage> =
        transcript.messages().filter_map(extract_durable).collect();
    if messages.is_empty() {
        return None;
    }
    // Cap the newest durable messages to the budget, keeping the tail of the
    // conversation (the ending is what a session end most needs to learn from).
    truncate_to_budget(&mut messages);
    Some(SessionDigest {
        session_id: transcript.meta.id.clone(),
        title: transcript.title().map(str::to_string),
        repo_root: transcript.meta.repo_root.clone(),
        user_turn_count,
        messages,
    })
}

fn extract_durable(m: &aster_persist::MessageEvent) -> Option<DigestMessage> {
    match m.role.as_str() {
        "user" | "assistant" => Some(DigestMessage {
            role: m.role.clone(),
            content: m.content.clone()?,
            ts: m.ts,
        }),
        // A tool result only carries signal when it is the terminal step of a
        // round (a read result, a command output) and non-empty.
        "tool" => {
            let content = m.content.as_deref()?.trim();
            if content.is_empty() {
                return None;
            }
            let truncated: String = content.chars().take(600).collect();
            Some(DigestMessage {
                role: "tool".into(),
                content: truncated,
                ts: m.ts,
            })
        }
        _ => None,
    }
}

fn truncate_to_budget(messages: &mut Vec<DigestMessage>) {
    let total: usize = messages.iter().map(|m| m.content.len()).sum();
    if total <= MAX_DIGEST_CHARS {
        return;
    }
    let mut dropped = 0usize;
    while dropped < messages.len() {
        let remaining: usize = messages[dropped..].iter().map(|m| m.content.len()).sum();
        if remaining <= MAX_DIGEST_CHARS {
            break;
        }
        dropped += 1;
    }
    if dropped > 0 {
        messages.drain(..dropped);
    }
}

/// Render a digest as the user-content block of a consolidation call.
pub fn render(digest: &SessionDigest) -> String {
    let mut out = String::new();
    out.push_str(&format!("Session id: {}\n", digest.session_id));
    if let Some(title) = &digest.title {
        out.push_str(&format!("Session title: {title}\n"));
    }
    out.push_str(&format!(
        "Project: {}\nUser turns: {}\n\n",
        digest.repo_root, digest.user_turn_count
    ));
    for m in &digest.messages {
        let role = match m.role.as_str() {
            "tool" => "result",
            other => other,
        };
        out.push_str(&format!("<{role}>\n{}\n</{role}>\n\n", m.content.trim()));
    }
    out
}

#[cfg(test)]
#[path = "digest_tests.rs"]
mod tests;
