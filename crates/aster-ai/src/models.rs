use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// Accept a missing field *and* an explicit `null`, which providers send for an
/// empty array when they cut a reply at the output budget. `#[serde(default)]`
/// alone only covers the missing case and fails the whole response otherwise.
fn null_default<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: Default + Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// The provider's own thinking fields, from [`crate::reasoning::fields`].
    #[serde(flatten)]
    pub reasoning: serde_json::Map<String, serde_json::Value>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    /// A thinking model that spends its whole output budget reasoning sends
    /// `content: null` with `finish_reason: "length"`. Read it as empty so the
    /// continuation loop can retry instead of failing the reply outright.
    #[serde(default, deserialize_with = "null_default")]
    pub content: MessageContent,
}

pub const IMAGE_OMITTED: &str = "[image content omitted because this model has no image input]";

pub const IMAGE_MARK: &str = "[image]";

/// A turn's content: plain text, or the parts array multimodal turns need.
/// Untagged, so a text turn still serializes as a bare JSON string for every
/// endpoint that only ever saw strings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

impl Default for MessageContent {
    fn default() -> Self {
        MessageContent::Text(String::new())
    }
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
    /// The provider's own thinking fields, from [`crate::reasoning::fields`].
    #[serde(flatten)]
    pub reasoning: serde_json::Map<String, serde_json::Value>,
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
    /// `stop` or `length` (reply hit the output budget). `length` is what the
    /// continuation loop in the client keys on.
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(
        default,
        deserialize_with = "null_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub tool_calls: Vec<ToolCall>,
    #[serde(
        default,
        deserialize_with = "null_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub annotations: Vec<Annotation>,
    #[serde(
        default,
        deserialize_with = "null_default",
        skip_serializing_if = "Vec::is_empty"
    )]
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
    /// A thinking block from text a provider sent as a bare string.
    pub fn from_text(text: String) -> Self {
        Self {
            kind: "reasoning.text".into(),
            format: None,
            text: Some(text),
            summary: None,
            data: None,
            signature: None,
            id: None,
            index: None,
        }
    }

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
    /// Provider data that must go back with the call, such as Gemini's
    /// `google.thought_signature`, without which the next request is refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_content: Option<serde_json::Value>,
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
    /// `stop`, `length` (reply hit the output budget), or `tool_calls`. `length`
    /// is what the continuation loop in the client keys on.
    #[serde(default)]
    pub finish_reason: Option<String>,
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
    #[serde(default, deserialize_with = "null_default")]
    pub choices: Vec<ChatStreamChoice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Deserialize)]
pub struct ChatStreamChoice {
    pub delta: ChatDelta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Deserialize)]
pub struct ChatDelta {
    #[serde(default)]
    pub content: Option<String>,
    /// Thinking as plain text. Other spellings (`reasoning`, thinking chunks)
    /// are moved here by [`crate::reasoning::normalize`] before parsing.
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub tool_calls: Vec<ToolCallDelta>,
    #[serde(default, deserialize_with = "null_default")]
    pub annotations: Vec<Annotation>,
    #[serde(default, deserialize_with = "null_default")]
    pub reasoning_details: Vec<ReasoningDetail>,
}

#[derive(Deserialize)]
pub struct ToolCallDelta {
    #[serde(default)]
    pub index: usize,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub extra_content: Option<serde_json::Value>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_nulls_in_an_assistant_message_parse_as_empty() {
        let parsed: ToolChatResponse = serde_json::from_str(
            r#"{"choices":[{"index":0,"message":{"role":"assistant","content":null,
            "refusal":null,"annotations":null,"audio":null,"function_call":null,
            "tool_calls":null,"reasoning_content":null,"reasoning_details":null},
            "finish_reason":"length"}]}"#,
        )
        .expect("null array fields must not fail the response");

        let message = parsed.choices.into_iter().next().unwrap().message;
        assert_eq!(message.content, None);
        assert!(message.tool_calls.is_empty());
        assert!(message.annotations.is_empty());
        assert!(message.reasoning_details.is_empty());
    }

    #[test]
    fn a_stream_chunk_tolerates_null_choices_and_arrays() {
        let parsed: ChatStreamChunk = serde_json::from_str(r#"{"choices":null,"usage":null}"#)
            .expect("null choices must not fail a stream chunk");

        assert!(parsed.choices.is_empty());
        assert!(parsed.usage.is_none());
    }

    #[test]
    fn a_delta_with_null_arrays_still_carries_its_text() {
        let parsed: ChatStreamChunk = serde_json::from_str(
            r#"{"choices":[{"delta":{"content":"hi","tool_calls":null,
            "annotations":null,"reasoning_details":null}}]}"#,
        )
        .expect("null delta arrays must not fail a chunk");

        assert_eq!(parsed.choices[0].delta.content, Some("hi".into()));
    }

    #[test]
    fn a_truncated_reply_still_parses_and_reports_length() {
        // The shape that ends a turn at the output budget: no content, null
        // everywhere a value was expected, and `finish_reason: "length"`.
        let parsed: ToolChatResponse = serde_json::from_str(
            r#"{"choices":[{"index":0,"message":{"role":"assistant","content":null,
            "annotations":null,"tool_calls":null,"reasoning_details":null},
            "finish_reason":"length"}],
            "usage":{"prompt_tokens":36173,"completion_tokens":8000,
            "total_tokens":44173}}"#,
        )
        .expect("a truncated reply must parse");

        let choice = parsed.choices.into_iter().next().unwrap();
        assert_eq!(choice.finish_reason.as_deref(), Some("length"));
        assert_eq!(choice.message.content, None);
        assert_eq!(parsed.usage.expect("usage present").completion_tokens, 8000);
    }

    #[test]
    fn a_reply_emptied_by_reasoning_parses_as_blank_text() {
        // A thinking model that burns its whole output budget on
        // `reasoning_content` sends every content field as an explicit null.
        // The plain chat path reads that as empty text so the reply can be
        // retried instead of failing the turn.
        let parsed: ChatResponse = serde_json::from_str(
            r#"{"choices":[{"index":0,"message":{"role":"assistant","content":null,
            "refusal":null,"annotations":null,"audio":null,"function_call":null,
            "tool_calls":null,"reasoning_content":"Now I have the full picture."},
            "finish_reason":"length","stop_reason":null,"token_ids":null}],
            "usage":{"prompt_tokens":36173,"completion_tokens":8000,
            "total_tokens":44173}}"#,
        )
        .expect("null content must not fail a plain chat reply");

        let message = parsed.choices.into_iter().next().unwrap().message;
        assert_eq!(message.content, MessageContent::Text(String::new()));
    }
}
