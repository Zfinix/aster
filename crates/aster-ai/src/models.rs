use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
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
    pub content: String,
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
    pub temperature: Option<f32>,
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
