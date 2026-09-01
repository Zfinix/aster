//! Recovery for models that write tool calls into the message content instead of
//! the API's `tool_calls` field. DeepSeek's blocks are the common case: unparsed,
//! the turn reads as a final answer full of raw markup.

use serde_json::{Map, Value};

use crate::models::{ToolCall, ToolCallFunction};

/// Tag openers that mean the model has started writing a tool call.
const OPENERS: &[&str] = &["tool_calls>", "invoke ", "function_calls>"];

/// Split inline tool calls off `content`, returning the text the user should
/// see and the calls to run. Content without a block comes back untouched.
pub fn split_inline_tool_calls(content: &str) -> (String, Vec<ToolCall>) {
    if !content.contains("invoke name=") {
        return (content.to_string(), Vec::new());
    }
    let normalized = normalize(content);
    let Some(start) = normalized
        .find("<tool_calls>")
        .or_else(|| normalized.find("<function_calls>"))
        .or_else(|| normalized.find("<invoke "))
    else {
        return (content.to_string(), Vec::new());
    };
    let calls = parse_invokes(&normalized[start..]);
    if calls.is_empty() {
        return (content.to_string(), Vec::new());
    }
    (normalized[..start].trim_end().to_string(), calls)
}

/// Strip the fullwidth-bar noise the delimiters are wrapped in, so what is left
/// is plain `<tag ...>` markup.
fn normalize(content: &str) -> String {
    content.replace("\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}", "")
}

fn parse_invokes(block: &str) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    let mut rest = block;
    while let Some(open) = rest.find("<invoke ") {
        let after = &rest[open + "<invoke ".len()..];
        let Some((head, body)) = after.split_once('>') else {
            break;
        };
        let Some(name) = attr(head, "name") else {
            rest = body;
            continue;
        };
        let (body, tail) = match body.split_once("</invoke>") {
            Some((body, tail)) => (body, tail),
            None => (body, ""),
        };
        calls.push(ToolCall {
            id: format!("inline_{}", calls.len()),
            kind: "function".to_string(),
            function: ToolCallFunction {
                name,
                arguments: Value::Object(parse_parameters(body)).to_string(),
            },
        });
        rest = tail;
    }
    calls
}

fn parse_parameters(body: &str) -> Map<String, Value> {
    let mut args = Map::new();
    let mut rest = body;
    while let Some(open) = rest.find("<parameter ") {
        let after = &rest[open + "<parameter ".len()..];
        let Some((head, value)) = after.split_once('>') else {
            break;
        };
        let (value, tail) = match value.split_once("</parameter>") {
            Some((value, tail)) => (value, tail),
            None => (value, ""),
        };
        if let Some(name) = attr(head, "name") {
            args.insert(name, typed(head, value));
        }
        rest = tail;
    }
    args
}

/// `string="false"` marks a number, boolean, or array the model wrote unquoted.
fn typed(head: &str, value: &str) -> Value {
    let value = value.trim();
    if attr(head, "string").as_deref() == Some("false")
        && let Ok(parsed) = serde_json::from_str::<Value>(value)
    {
        return parsed;
    }
    Value::String(value.to_string())
}

fn attr(head: &str, key: &str) -> Option<String> {
    let at = head.find(&format!("{key}=\""))?;
    let after = &head[at + key.len() + 2..];
    after.split_once('"').map(|(value, _)| value.to_string())
}

/// Withholds streamed text once the model starts writing a tool call, so the
/// markup never reaches the UI. One gate per message.
#[derive(Default)]
pub struct TokenGate {
    /// Text held back because it could still turn into a tag opener.
    pending: String,
    closed: bool,
}

impl TokenGate {
    /// Feed one content delta, emitting the part that is safe to display.
    pub fn feed(&mut self, delta: &str, emit: &mut impl FnMut(&str)) {
        if self.closed {
            return;
        }
        self.pending.push_str(delta);
        // The earliest suspicious `<` bounds what is safe to show: a later one
        // may be markup the model has already started writing.
        let found = self
            .pending
            .match_indices('<')
            .map(|(at, _)| (at, classify(&self.pending[at..])))
            .find(|(_, tag)| !matches!(tag, Tag::No));
        match found {
            None => self.emit_through(self.pending.len(), emit),
            Some((at, Tag::Complete)) => {
                self.closed = true;
                let safe = self.pending[..at].trim_end().to_string();
                self.pending.clear();
                emit(&safe);
            }
            Some((at, _)) => self.emit_through(at, emit),
        }
    }

    /// Emit whatever is still held back. Call once the message ends.
    pub fn finish(&mut self, emit: &mut impl FnMut(&str)) {
        if !self.closed {
            self.flush(emit);
        }
    }

    /// Emit `pending` up to `at`, holding back the trailing whitespace: if the
    /// markup starts on the next line, that blank line must not reach the UI.
    fn emit_through(&mut self, at: usize, emit: &mut impl FnMut(&str)) {
        let safe = self.pending[..at].trim_end().len();
        if safe > 0 {
            let text = self.pending[..safe].to_string();
            self.pending = self.pending[safe..].to_string();
            emit(&text);
        }
    }

    fn flush(&mut self, emit: &mut impl FnMut(&str)) {
        if !self.pending.is_empty() {
            let text = std::mem::take(&mut self.pending);
            emit(&text);
        }
    }
}

enum Tag {
    /// A tool call has started: everything from here on is markup.
    Complete,
    /// Could still become one once more text arrives.
    Partial,
    /// Ordinary text.
    No,
}

/// `tag` starts at a `<`. A half-written opener (`<`, `<\u{ff5c}\u{ff5c}DSML`,
/// `<tool_`) counts as partial: the rest of it may be in the next delta.
fn classify(tag: &str) -> Tag {
    let stripped = tag[1..].replace('\u{ff5c}', "");
    if !stripped.is_empty() && "DSML".starts_with(&stripped) {
        return Tag::Partial;
    }
    let body = stripped.strip_prefix("DSML").unwrap_or(&stripped);
    if OPENERS.iter().any(|o| body.starts_with(o)) {
        return Tag::Complete;
    }
    if OPENERS.iter().any(|o| o.starts_with(body)) {
        return Tag::Partial;
    }
    Tag::No
}

#[cfg(test)]
#[path = "tests/inline_tools_test.rs"]
mod tests;
