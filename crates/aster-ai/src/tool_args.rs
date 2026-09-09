//! Repair for tool-call arguments that are not valid JSON. Endpoints reject the
//! whole request when one is malformed, and the bad call rides in history for
//! the rest of the thread, so every later turn fails too.

use serde_json::Value;

use crate::models::ToolCall;

/// Rewrite every call's arguments to a valid JSON object, in place.
pub fn heal_calls(calls: &mut [ToolCall]) {
    for call in calls {
        let healed = heal(&call.function.arguments);
        if healed != call.function.arguments {
            tracing::debug!(
                tool = %call.function.name,
                raw = %call.function.arguments,
                "repaired malformed tool-call arguments"
            );
            call.function.arguments = healed;
        }
    }
}

/// A JSON object string for `raw`, whatever shape it arrived in. Unrepairable
/// arguments become `{}`: the tool then reports its missing parameters and the
/// model retries, which beats the endpoint refusing the conversation.
pub fn heal(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "{}".to_string();
    }
    if let Some(object) = as_object(trimmed) {
        return object;
    }
    if let Some(object) = close_open_scopes(trimmed).as_deref().and_then(as_object) {
        return object;
    }
    "{}".to_string()
}

/// Accepts an object, and unwraps the double-encoded string some endpoints send
/// (`"{\"path\":\"a\"}"` rather than `{"path":"a"}`).
fn as_object(text: &str) -> Option<String> {
    match serde_json::from_str::<Value>(text).ok()? {
        value @ Value::Object(_) => Some(value.to_string()),
        Value::String(inner) => as_object(inner.trim()),
        _ => None,
    }
}

/// Shuts unterminated strings and brackets, in the order they were opened. A
/// truncated stream is the usual reason arguments do not parse.
fn close_open_scopes(text: &str) -> Option<String> {
    let mut scopes = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for ch in text.chars() {
        if in_string {
            match ch {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => scopes.push('}'),
            '[' => scopes.push(']'),
            '}' | ']' if scopes.pop() != Some(ch) => return None,
            _ => {}
        }
    }
    if scopes.is_empty() && !in_string {
        return None;
    }
    let mut healed = text.to_string();
    if in_string {
        if escaped {
            healed.pop();
        }
        healed.push('"');
    }
    // A truncated key or a dangling comma leaves the object mid-pair; dropping
    // back to the last complete one is better than inventing a value.
    while scopes.last() == Some(&'}') && needs_value(&healed) {
        match healed.rfind(',') {
            Some(comma) => healed.truncate(comma),
            None => healed.truncate(healed.find('{')? + 1),
        }
    }
    while let Some(closer) = scopes.pop() {
        healed.push(closer);
    }
    Some(healed)
}

/// True when the object's last pair stops before its value, as in `{"a":1,"b":`.
fn needs_value(text: &str) -> bool {
    let tail = text.trim_end();
    tail.ends_with(':') || tail.ends_with(',') || {
        let after_comma = tail.rsplit(['{', ',']).next().unwrap_or("").trim();
        !after_comma.is_empty() && !after_comma.contains(':')
    }
}

#[cfg(test)]
#[path = "tests/tool_args_test.rs"]
mod tests;
