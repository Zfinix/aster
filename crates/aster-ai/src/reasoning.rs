//! Each provider's controls for model thinking: the request fields that set the
//! effort, the reply fields thinking arrives in, and the message field that
//! hands earlier thinking back. None of this is in the OpenAI schema.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::Effort;
use crate::codex_api;
use crate::models::{ChatRequest, ReasoningDetail, ToolChatRequest};

/// Told to the user when a reply is all thinking and no answer.
pub const THINKING_EXHAUSTED: &str = "The model used its whole reply length thinking and never \
    wrote an answer. Lower the effort, or raise agent.max_output_tokens so it has room to reply.";

/// How a provider takes the effort setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Knob {
    /// `reasoning: {effort}`, or `reasoning: {enabled: false}` for off.
    OpenRouter,
    /// The Responses translation reads `reasoning.effort`; off is `none`.
    Codex,
    /// Top-level `reasoning_effort`, with `none` for off.
    Effort,
    /// `reasoning: {enabled: false}` for off, `reasoning_effort` for a level.
    Together,
    /// Anthropic's compatibility layer ignores `reasoning_effort`; only off can be said.
    AnthropicThinking,
    /// An `enable_thinking: false` switch for off, `reasoning_effort` for a level.
    EnableThinking,
    /// Ollama reads `reasoning_effort` but only takes `false` for off.
    Ollama,
    /// A chat template flag, `chat_template_kwargs.enable_thinking`, with
    /// `reasoning_effort` beside it for the models that read a level instead.
    ChatTemplate,
    /// `thinking: {type: disabled}`, plus `reasoning_split` so thinking stays out of content.
    MiniMax,
}

/// Where earlier thinking goes when an assistant turn is sent back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Replay {
    /// OpenRouter's `reasoning_details` blocks, verbatim.
    Details,
    /// A `reasoning_content` string beside the content.
    Content,
    /// A `reasoning` string beside the content.
    Reasoning,
    /// Mistral's content list with a leading `thinking` chunk.
    ThinkChunks,
    /// Not sent: the provider rejects reasoning fields on input messages.
    Strip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Dialect {
    pub knob: Knob,
    pub replay: Replay,
}

/// Hosts matched by substring, first match wins. Anything unlisted speaks
/// `reasoning_effort` and takes earlier thinking back as `reasoning_content`.
const HOSTS: &[(&str, Knob, Replay)] = &[
    ("openrouter.ai", Knob::OpenRouter, Replay::Details),
    ("ai-gateway.vercel.sh", Knob::OpenRouter, Replay::Details),
    ("api.anthropic.com", Knob::AnthropicThinking, Replay::Strip),
    ("api.openai.com", Knob::Effort, Replay::Strip),
    ("openai.azure.com", Knob::Effort, Replay::Strip),
    ("api.groq.com", Knob::Effort, Replay::Strip),
    ("api.x.ai", Knob::Effort, Replay::Strip),
    ("api.meta.ai", Knob::Effort, Replay::Strip),
    (
        "generativelanguage.googleapis.com",
        Knob::Effort,
        Replay::Strip,
    ),
    ("api.cerebras.ai", Knob::Effort, Replay::Reasoning),
    ("api.together.xyz", Knob::Together, Replay::Content),
    ("api.sambanova.ai", Knob::ChatTemplate, Replay::Strip),
    (
        "integrate.api.nvidia.com",
        Knob::ChatTemplate,
        Replay::Content,
    ),
    ("api.cloudflare.com", Knob::ChatTemplate, Replay::Content),
    ("api.mistral.ai", Knob::Effort, Replay::ThinkChunks),
    ("dashscope", Knob::EnableThinking, Replay::Content),
    ("api.novita.ai", Knob::EnableThinking, Replay::Content),
    ("api.minimax", Knob::MiniMax, Replay::Details),
    (":11434", Knob::Ollama, Replay::Reasoning),
];

pub(crate) fn dialect(base_url: &str) -> Dialect {
    if codex_api::is_codex(base_url) {
        return Dialect {
            knob: Knob::Codex,
            replay: Replay::Strip,
        };
    }
    let url = base_url.to_ascii_lowercase();
    let (knob, replay) = HOSTS
        .iter()
        .find(|(host, _, _)| url.contains(host))
        .map_or((Knob::Effort, Replay::Content), |&(_, knob, replay)| {
            (knob, replay)
        });
    Dialect { knob, replay }
}

/// The request fields for `effort`; [`None`] sends nothing and leaves the
/// model on its own default.
pub(crate) fn fields(knob: Knob, effort: Option<Effort>) -> Map<String, Value> {
    let mut out = Map::new();
    if knob == Knob::MiniMax {
        out.insert("reasoning_split".into(), json!(true));
    }
    let Some(effort) = effort else {
        return out;
    };
    let off = effort == Effort::Off;
    match knob {
        Knob::OpenRouter | Knob::Together if off => {
            out.insert("reasoning".into(), json!({ "enabled": false }));
        }
        Knob::OpenRouter => {
            out.insert("reasoning".into(), json!({ "effort": level(effort) }));
        }
        Knob::Codex => {
            out.insert("reasoning".into(), json!({ "effort": level(effort) }));
        }
        Knob::EnableThinking if off => {
            out.insert("enable_thinking".into(), json!(false));
        }
        Knob::Ollama if off => {
            out.insert("reasoning_effort".into(), json!(false));
        }
        Knob::Effort | Knob::Together | Knob::EnableThinking | Knob::Ollama => {
            out.insert("reasoning_effort".into(), json!(level(effort)));
        }
        Knob::AnthropicThinking | Knob::MiniMax if off => {
            out.insert("thinking".into(), json!({ "type": "disabled" }));
        }
        Knob::ChatTemplate => {
            out.insert(
                "chat_template_kwargs".into(),
                json!({ "enable_thinking": !off }),
            );
            if !off {
                out.insert("reasoning_effort".into(), json!(level(effort)));
            }
        }
        Knob::AnthropicThinking | Knob::MiniMax => {}
    }
    out
}

fn level(effort: Effort) -> &'static str {
    match effort {
        Effort::Off => "none",
        Effort::Ultra => "max",
        effort => effort.as_str(),
    }
}

/// The next setting to try after a provider refused `effort`: off falls to the
/// lightest thinking, a level above high falls to high, and anything else is
/// dropped so the model runs on its default. [`None`] means nothing is left.
pub(crate) fn relax(effort: Option<Effort>) -> Option<Option<Effort>> {
    match effort? {
        Effort::Off => Some(Some(Effort::Low)),
        Effort::XHigh | Effort::Max | Effort::Ultra => Some(Some(Effort::High)),
        Effort::Low | Effort::Medium | Effort::High => Some(None),
    }
}

/// A 400 or 422 that names the thinking controls.
pub(crate) fn rejected_effort(err: &anyhow::Error) -> bool {
    let text = format!("{err:#}").to_lowercase();
    (text.contains("(400)") || text.contains("(422)"))
        && ["reasoning", "thinking", "effort"]
            .iter()
            .any(|word| text.contains(word))
}

/// A refusal of the thinking carried on earlier assistant messages.
pub(crate) fn rejected_history(err: &anyhow::Error) -> bool {
    let text = format!("{err:#}").to_lowercase();
    ["reasoning_content", "reasoning_details", "'reasoning'"]
        .iter()
        .any(|field| text.contains(field))
        && ["message", "assistant"].iter().any(|w| text.contains(w))
        && !text.contains("missing")
}

/// Rewrite the thinking on assistant turns into the provider's field.
pub(crate) fn replay(messages: &mut [Value], replay: Replay) {
    for message in messages {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(object) = message.as_object_mut() else {
            continue;
        };
        if replay == Replay::Details {
            continue;
        }
        let details: Vec<ReasoningDetail> = object
            .remove("reasoning_details")
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        let text = details
            .iter()
            .filter(|d| d.portable())
            .filter_map(ReasoningDetail::plain)
            .collect::<Vec<_>>()
            .join("\n\n");
        match replay {
            Replay::Details => {}
            Replay::Strip => {
                object.remove("reasoning_content");
                object.remove("reasoning");
            }
            Replay::Content if !text.is_empty() => {
                object.insert("reasoning_content".into(), json!(text));
            }
            Replay::Reasoning if !text.is_empty() => {
                object.insert("reasoning".into(), json!(text));
            }
            Replay::ThinkChunks if !text.is_empty() => {
                let mut chunks = vec![json!({
                    "type": "thinking",
                    "thinking": [{ "type": "text", "text": text }],
                })];
                if let Some(content) = object.get("content").and_then(Value::as_str) {
                    chunks.push(json!({ "type": "text", "text": content }));
                }
                object.insert("content".into(), Value::Array(chunks));
            }
            Replay::Content | Replay::Reasoning | Replay::ThinkChunks => {}
        }
    }
}

/// Bring a response's choices into one shape before parsing: content lists are
/// split into text and thinking, and a bare `reasoning` string becomes
/// `reasoning_content`. `key` is `delta` for stream chunks, `message` otherwise.
pub(crate) fn normalize(body: &mut Value, key: &str) {
    let Some(choices) = body.get_mut("choices").and_then(Value::as_array_mut) else {
        return;
    };
    for choice in choices {
        let Some(message) = choice.get_mut(key).and_then(Value::as_object_mut) else {
            continue;
        };
        let mut thinking = String::new();
        if let Some(Value::Array(parts)) = message.get("content") {
            let mut text = String::new();
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("text") => text.push_str(part["text"].as_str().unwrap_or_default()),
                    Some("thinking") => push_thinking(&mut thinking, &part["thinking"]),
                    _ => {}
                }
            }
            message.insert("content".into(), json!(text));
        }
        if key == "message"
            && let Some(content) = message.get("content").and_then(Value::as_str)
            && content.trim_start().starts_with(THINK_OPEN)
        {
            let (answer, thought) = split_think(content);
            thinking.push_str(&thought);
            message.insert("content".into(), json!(answer));
        }
        if let Some(nested) = message.get("reasoning_content").filter(|v| v.is_object()) {
            let mut text = String::new();
            collect_text(nested, &mut text);
            message.insert("reasoning_content".into(), json!(text));
        }
        let has_details = message
            .get("reasoning_details")
            .and_then(Value::as_array)
            .is_some_and(|d| !d.is_empty());
        if thinking.is_empty()
            && !has_details
            && let Some(reasoning) = message.get("reasoning").and_then(Value::as_str)
            && message.get("reasoning_content").is_none_or(Value::is_null)
        {
            thinking.push_str(reasoning);
        }
        if !thinking.is_empty() {
            let existing = message
                .get("reasoning_content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let merged = format!("{existing}{thinking}");
            message.insert("reasoning_content".into(), json!(merged));
        }
    }
}

/// Bedrock sends `reasoning_content` as `{reasoningContent: {reasoningText: ...}}`.
fn collect_text(value: &Value, out: &mut String) {
    match value {
        Value::String(text) => out.push_str(text),
        Value::Object(fields) => fields
            .iter()
            .filter(|(key, _)| key.as_str() != "signature")
            .for_each(|(_, v)| collect_text(v, out)),
        _ => {}
    }
}

fn push_thinking(out: &mut String, thinking: &Value) {
    match thinking {
        Value::String(text) => out.push_str(text),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .for_each(|text| out.push_str(text)),
        _ => {}
    }
}

/// Settings a provider refused this session, so each refusal costs one retry.
#[derive(Debug, Default)]
pub(crate) struct Memo {
    /// What to send in place of an effort the model refused, per model.
    pub efforts: HashMap<(String, Effort), Option<Effort>>,
    /// Earlier thinking on assistant turns was refused, so none is sent.
    pub strip_history: bool,
}

/// A request whose thinking fields can be rewritten after a refusal.
pub(crate) trait Reasoned: Serialize {
    fn model(&self) -> &str;
    fn reasoning_mut(&mut self) -> &mut Map<String, Value>;
    fn history_mut(&mut self) -> Option<&mut [Value]>;
}

impl Reasoned for ChatRequest {
    fn model(&self) -> &str {
        &self.model
    }
    fn reasoning_mut(&mut self) -> &mut Map<String, Value> {
        &mut self.reasoning
    }
    fn history_mut(&mut self) -> Option<&mut [Value]> {
        None
    }
}

impl Reasoned for ToolChatRequest {
    fn model(&self) -> &str {
        &self.model
    }
    fn reasoning_mut(&mut self) -> &mut Map<String, Value> {
        &mut self.reasoning
    }
    fn history_mut(&mut self) -> Option<&mut [Value]> {
        Some(&mut self.messages)
    }
}

/// Whether any assistant turn still carries thinking to send back.
pub(crate) fn carries_thinking(messages: &[Value]) -> bool {
    messages.iter().any(|m| {
        m.get("role").and_then(Value::as_str) == Some("assistant")
            && ["reasoning_details", "reasoning_content", "reasoning"]
                .iter()
                .any(|field| m.get(*field).is_some_and(|v| !v.is_null()))
    })
}

const THINK_OPEN: &str = "<think>";
const THINK_CLOSE: &str = "</think>";

/// Splits a reply that opens with `<think>` into thinking and answer as the
/// chunks stream in. A reply that opens with anything else passes through.
#[derive(Debug, Default)]
pub(crate) struct ThinkTags {
    state: TagState,
    pending: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
enum TagState {
    #[default]
    Undecided,
    Thinking,
    Answering,
}

impl ThinkTags {
    /// Feed one content chunk; returns the answer text and thinking text in it.
    pub fn feed(&mut self, chunk: &str) -> (String, String) {
        if self.state == TagState::Answering {
            return (chunk.to_string(), String::new());
        }
        self.pending.push_str(chunk);
        if self.state == TagState::Undecided {
            let head = self.pending.trim_start();
            if head.is_empty() || (head.len() < THINK_OPEN.len() && THINK_OPEN.starts_with(head)) {
                return (String::new(), String::new());
            }
            let Some(rest) = head.strip_prefix(THINK_OPEN) else {
                self.state = TagState::Answering;
                return (std::mem::take(&mut self.pending), String::new());
            };
            self.pending = rest.to_string();
            self.state = TagState::Thinking;
        }
        if let Some(end) = self.pending.find(THINK_CLOSE) {
            let thinking = self.pending[..end].to_string();
            let answer = self.pending[end + THINK_CLOSE.len()..]
                .trim_start()
                .to_string();
            self.pending.clear();
            self.state = TagState::Answering;
            return (answer, thinking);
        }
        let keep = (1..THINK_CLOSE.len())
            .rev()
            .find(|&n| self.pending.ends_with(&THINK_CLOSE[..n]))
            .unwrap_or(0);
        let split = self.pending.len() - keep;
        let thinking = self.pending[..split].to_string();
        self.pending.drain(..split);
        (String::new(), thinking)
    }

    /// Whatever was held back when the stream ends.
    pub fn finish(&mut self) -> (String, String) {
        let rest = std::mem::take(&mut self.pending);
        match self.state {
            TagState::Thinking => (String::new(), rest),
            _ => (rest, String::new()),
        }
    }
}

/// [`ThinkTags`] for a whole reply at once.
pub(crate) fn split_think(content: &str) -> (String, String) {
    let mut tags = ThinkTags::default();
    let (mut answer, mut thinking) = tags.feed(content);
    let (rest_answer, rest_thinking) = tags.finish();
    answer.push_str(&rest_answer);
    thinking.push_str(&rest_thinking);
    (answer, thinking)
}

#[cfg(test)]
#[path = "tests/reasoning_test.rs"]
mod tests;
