//! Message-list shaping applied to every request, so a conversation a host
//! assembled locally is one a provider will accept.

use serde_json::{Value, json};

use crate::models::ChatMessage;

/// Wrapper kept around a folded note, so the model reads it as harness state
/// rather than as something the user typed.
const OPEN: &str = "<system-note>";
const CLOSE: &str = "</system-note>";

/// Fold system messages that are not the leading one into the turn beside them.
///
/// Only a leading system message is portable. Anthropic rejects one that
/// follows an assistant reply, and hosts legitimately record mid-conversation
/// notes: a permission-mode change, an editor's injected context. The note is
/// kept as user content so the model still sees it, merged into the user turn
/// before it when there is one.
pub(crate) fn fold_system_notes(messages: Vec<Value>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::with_capacity(messages.len());
    for message in messages {
        let is_note =
            out.iter().any(|m| role_of(m) != Some("system")) && role_of(&message) == Some("system");
        if !is_note {
            out.push(message);
            continue;
        }
        let Some(text) = message.get("content").and_then(Value::as_str) else {
            // A note with structured content has nothing to merge; carry it as
            // its own user turn.
            let mut message = message;
            if let Some(object) = message.as_object_mut() {
                object.insert("role".into(), Value::String("user".into()));
            }
            out.push(message);
            continue;
        };
        let note = wrap(text);
        match out.last_mut() {
            Some(previous) if merges(previous) => append(previous, &note),
            _ => out.push(json!({ "role": "user", "content": note })),
        }
    }
    out
}

/// [`fold_system_notes`] for the typed message list the non-tool paths use.
pub(crate) fn fold_system_chat(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::with_capacity(messages.len());
    for message in messages {
        let is_note = out.iter().any(|m| m.role != "system") && message.role == "system";
        if !is_note {
            out.push(message);
            continue;
        }
        let note = wrap(&message.content);
        match out.last_mut() {
            Some(previous) if previous.role == "user" => {
                previous.content.push_str("\n\n");
                previous.content.push_str(&note);
            }
            _ => out.push(ChatMessage {
                role: "user".into(),
                content: note,
            }),
        }
    }
    out
}

fn wrap(text: &str) -> String {
    format!("{OPEN}{}{CLOSE}", text.trim())
}

fn role_of(message: &Value) -> Option<&str> {
    message.get("role").and_then(Value::as_str)
}

/// A user turn carrying plain text is the one shape a note can be appended to
/// without disturbing tool calls or content parts.
fn merges(message: &Value) -> bool {
    role_of(message) == Some("user") && message.get("content").is_some_and(Value::is_string)
}

fn append(message: &mut Value, note: &str) {
    let Some(text) = message.get("content").and_then(Value::as_str) else {
        return;
    };
    let joined = format!("{text}\n\n{note}");
    if let Some(object) = message.as_object_mut() {
        object.insert("content".into(), Value::String(joined));
    }
}

#[cfg(test)]
#[path = "tests/wire_test.rs"]
mod tests;
