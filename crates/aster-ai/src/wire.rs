//! Message-list shaping applied to every request, so a conversation a host
//! assembled locally is one a provider will accept.

use serde_json::{Value, json};

use crate::models::{ChatMessage, IMAGE_OMITTED};

/// Wrapper kept around a folded note, so the model reads it as harness state
/// rather than as something the user typed.
const OPEN: &str = "<system-note>";
const CLOSE: &str = "</system-note>";

/// Fold non-leading system messages into the turn beside them: only a leading one
/// is portable, and Anthropic rejects one that follows an assistant reply. The
/// note is kept as user content, merged into the user turn before it.
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
        let note = wrap(&message.content.text());
        match out.last_mut() {
            Some(previous) if previous.role == "user" => {
                previous.content.push_str("\n\n");
                previous.content.push_str(&note);
            }
            _ => out.push(ChatMessage {
                role: "user".into(),
                content: note.into(),
            }),
        }
    }
    out
}

/// Whether any turn carries an image, and so whether the model's modalities
/// matter for this request at all.
pub(crate) fn carries_images(messages: &[Value]) -> bool {
    messages.iter().any(|m| {
        m.get("content")
            .and_then(Value::as_array)
            .is_some_and(|parts| {
                parts
                    .iter()
                    .any(|p| p.get("type").and_then(Value::as_str) == Some("image_url"))
            })
    })
}

/// Replace every image with [`IMAGE_OMITTED`], leaving a turn the model can
/// still read. Collapses back to a plain string, which is the shape a
/// text-only endpoint is likeliest to accept.
pub(crate) fn strip_image_parts(messages: &mut [Value]) {
    for message in messages.iter_mut() {
        let Some(parts) = message.get("content").and_then(Value::as_array) else {
            continue;
        };
        let text = parts
            .iter()
            .map(|part| match part.get("type").and_then(Value::as_str) {
                Some("image_url") => IMAGE_OMITTED,
                _ => part.get("text").and_then(Value::as_str).unwrap_or_default(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(object) = message.as_object_mut() {
            object.insert("content".into(), Value::String(text));
        }
    }
}

/// Anthropic and Gemini via OpenRouter need explicit cache breakpoints;
/// everyone else caches implicitly. `ASTER_PROMPT_CACHE=off` disables it.
pub(crate) fn wants_cache_control(base_url: &str, model: &str) -> bool {
    if matches!(
        std::env::var("ASTER_PROMPT_CACHE").ok().as_deref(),
        Some("0") | Some("off") | Some("false")
    ) {
        return false;
    }
    let model = model.to_ascii_lowercase();
    base_url.contains("openrouter") && (model.contains("claude") || model.contains("gemini"))
}

/// Mark the system prompt and the newest message as cache breakpoints; the
/// moving one turns each round's context into the next round's cache hit.
pub(crate) fn apply_cache_control(messages: &mut [Value]) {
    if let Some(system) = messages.iter_mut().find(|m| role_of(m) == Some("system")) {
        mark_cached(system);
    }
    for message in messages.iter_mut().rev() {
        if mark_cached(message) {
            break;
        }
    }
}

/// False when the message has no text block to mark.
fn mark_cached(message: &mut Value) -> bool {
    let Some(content) = message.get_mut("content") else {
        return false;
    };
    if let Some(text) = content.as_str().map(str::to_owned) {
        if text.is_empty() {
            return false;
        }
        *content = json!([{
            "type": "text",
            "text": text,
            "cache_control": { "type": "ephemeral" },
        }]);
        return true;
    }
    let Some(parts) = content.as_array_mut() else {
        return false;
    };
    for part in parts.iter_mut().rev() {
        if part.get("type").and_then(Value::as_str) == Some("text")
            && let Some(object) = part.as_object_mut()
        {
            object.insert("cache_control".into(), json!({ "type": "ephemeral" }));
            return true;
        }
    }
    false
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
