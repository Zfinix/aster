use std::borrow::Cow;

use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stream: bool,
    /// Requests a final usage chunk on streamed responses; ignored where unsupported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
    /// With `temperature: 0`, makes output reproducible on providers that honor it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Reasoning-token control for thinking models (OpenRouter shape).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Reasoning>,
    /// OpenRouter web-search plugin. Only serializes when set, so non-OpenRouter
    /// endpoints are unaffected.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<WebSearchPlugin>,
}

/// OpenRouter `web` plugin entry. Serialized as `{"id": "web"}` (plus optional
/// engine/domain filters) inside the request's `plugins` array.
#[derive(Serialize)]
pub struct WebSearchPlugin {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub include_domains: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude_domains: Vec<String>,
}

#[derive(Serialize)]
pub struct StreamOptions {
    pub include_usage: bool,
}

/// `effort` is "low"/"medium"/"high"; `enabled: false` turns reasoning off.
#[derive(Serialize)]
pub struct Reasoning {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: MessageContent,
}

/// What a model that cannot see an image is sent in its place.
pub const IMAGE_OMITTED: &str = "[image content omitted because this model has no image input]";

/// How an attached image reads wherever a turn is rendered as text: the
/// transcript, a title, a summary. The mention beside it names the file.
pub const IMAGE_MARK: &str = "[image]";

/// A turn's content: plain text, or the parts array multimodal turns need.
///
/// Untagged, so a text turn still serializes as a bare JSON string and every
/// endpoint that only ever saw strings sees the same bytes it always did.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

/// One part of a multimodal turn, in the OpenAI shape OpenRouter normalizes
/// every upstream provider to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

/// `url` is a `data:` URL: images are always inlined, never fetched by the
/// provider on our behalf.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageUrl {
    pub url: String,
}

impl MessageContent {
    /// The turn as text, with each image standing in as [`IMAGE_OMITTED`]. What
    /// summarizing, titling, and the transcript read.
    pub fn text(&self) -> Cow<'_, str> {
        match self {
            Self::Text(text) => Cow::Borrowed(text),
            Self::Parts(parts) => Cow::Owned(
                parts
                    .iter()
                    .map(|part| match part {
                        ContentPart::Text { text } => text.as_str(),
                        ContentPart::ImageUrl { .. } => IMAGE_MARK,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        }
    }

    pub fn has_images(&self) -> bool {
        matches!(self, Self::Parts(parts) if parts
            .iter()
            .any(|p| matches!(p, ContentPart::ImageUrl { .. })))
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::Text(text) => text.is_empty(),
            Self::Parts(parts) => parts.is_empty(),
        }
    }

    /// Append to the trailing text, or start a new text part. Keeps a folded
    /// system note beside the turn it belongs to without disturbing its images.
    pub fn push_str(&mut self, extra: &str) {
        match self {
            Self::Text(text) => text.push_str(extra),
            Self::Parts(parts) => match parts.last_mut() {
                Some(ContentPart::Text { text }) => text.push_str(extra),
                _ => parts.push(ContentPart::Text {
                    text: extra.to_string(),
                }),
            },
        }
    }

    /// Images stand in at a flat rate: their real cost is in tokens the provider
    /// charges for pixels, which the base64 length says nothing about.
    pub fn chars(&self) -> usize {
        match self {
            Self::Text(text) => text.len(),
            Self::Parts(parts) => parts
                .iter()
                .map(|part| match part {
                    ContentPart::Text { text } => text.len(),
                    ContentPart::ImageUrl { .. } => IMAGE_CHARS,
                })
                .sum(),
        }
    }

    /// Replace every image with [`IMAGE_OMITTED`], for a model that cannot take
    /// one. A turn left with nothing but text collapses back to a string.
    pub fn strip_images(&mut self) {
        let Self::Parts(parts) = self else {
            return;
        };
        for part in parts.iter_mut() {
            if matches!(part, ContentPart::ImageUrl { .. }) {
                *part = ContentPart::Text {
                    text: IMAGE_OMITTED.to_string(),
                };
            }
        }
        *self = Self::Text(self.text().into_owned());
    }
}

/// Roughly 1500 tokens at four characters each, the flat cost the majority of
/// providers bill an image at.
const IMAGE_CHARS: usize = 6_000;

impl From<String> for MessageContent {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for MessageContent {
    fn from(text: &str) -> Self {
        Self::Text(text.to_string())
    }
}

impl std::fmt::Display for MessageContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text())
    }
}

#[derive(Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<ChatChoice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

/// Chat request carrying tool definitions and tool-call turns. Messages are raw
/// JSON because tool turns have shapes (`tool_calls`, `tool_call_id`) that
/// [`ChatMessage`] does not model.
#[derive(Serialize)]
pub struct ToolChatRequest {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    pub messages: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Reasoning>,
    /// OpenRouter web-search plugin. Only serializes when set, so non-OpenRouter
    /// endpoints are unaffected.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<WebSearchPlugin>,
}

#[derive(Deserialize)]
pub struct ToolChatResponse {
    pub choices: Vec<ToolChatChoice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Deserialize)]
pub struct ToolChatChoice {
    pub message: AssistantMessage,
}

/// An assistant turn that may answer in text, request tool calls, or both.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Source citations attached by the OpenRouter web-search plugin. Each
    /// annotation carries a URL and optional title the UI can render as a
    /// source link below the answer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
    /// The turn's thinking, replayed on the next request so a reasoning model
    /// resumes its chain of thought after a tool result instead of restarting.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasoning_details: Vec<ReasoningDetail>,
}

/// One block of a reasoning turn. `kind` is `reasoning.text`,
/// `reasoning.encrypted`, or `reasoning.summary`; `format` names the provider
/// dialect that produced it, such as `anthropic-claude-v1`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReasoningDetail {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
}

impl ReasoningDetail {
    /// Whether a different model may be shown this block. Encrypted payloads
    /// and signed text are sealed against the model that produced them, so
    /// replaying either after a `/model` switch is rejected upstream.
    pub fn portable(&self) -> bool {
        self.kind != "reasoning.encrypted" && self.signature.is_none()
    }

    /// The block as prose, for the paths that can only carry text.
    pub fn plain(&self) -> Option<&str> {
        self.text
            .as_deref()
            .or(self.summary.as_deref())
            .map(str::trim)
            .filter(|t| !t.is_empty())
    }
}

/// A citation annotation on an assistant message. OpenRouter returns these as
/// `{"type": "url_citation", "url_citation": {"url": "...", "title": "..."}}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotation {
    #[serde(rename = "type")]
    pub kind: String,
    pub url_citation: UrlCitation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlCitation {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolCallFunction,
}

/// `arguments` is a JSON string, per the OpenAI schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Deserialize)]
pub struct ChatChoice {
    pub message: ChatMessage,
}

#[derive(Deserialize, Default, Clone, Copy)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
}

#[derive(Deserialize)]
pub struct ChatStreamChunk {
    #[serde(default)]
    pub choices: Vec<ChatStreamChoice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Deserialize)]
pub struct ChatStreamChoice {
    pub delta: ChatDelta,
}

#[derive(Deserialize)]
pub struct ChatDelta {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallDelta>,
    /// Citations arrive on the final streaming chunk, not per-delta.
    #[serde(default)]
    pub annotations: Vec<Annotation>,
    #[serde(default)]
    pub reasoning_details: Vec<ReasoningDetail>,
}

#[derive(Deserialize)]
pub struct ToolCallDelta {
    #[serde(default)]
    pub index: usize,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub function: Option<ToolCallFunctionDelta>,
}

#[derive(Deserialize)]
pub struct ToolCallFunctionDelta {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}
