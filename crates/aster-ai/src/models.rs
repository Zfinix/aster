use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stream: bool,
    /// Ask OpenAI-compatible providers to emit a final usage chunk on streamed
    /// responses. Ignored by providers that do not support it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
    /// Fixed sampling seed. With `temperature: 0` this makes output reproducible
    /// on providers that honor it; ignored elsewhere.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// Upper bound on generated tokens, so a pathological run can't stream
    /// unbounded and blow up latency.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Reasoning-token control for thinking models (OpenRouter shape). Bounds the
    /// hidden reasoning that otherwise makes latency wildly variable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Reasoning>,
}

#[derive(Serialize)]
pub struct StreamOptions {
    pub include_usage: bool,
}

/// Reasoning-effort control. `effort` is "low"/"medium"/"high"; `enabled: false`
/// turns reasoning off entirely. Providers without reasoning ignore the field.
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

/// A chat request that can carry tool definitions and tool-call turns.
/// Messages are raw JSON objects because assistant/tool turns have shapes
/// (`tool_calls`, `tool_call_id`) the plain [`ChatMessage`] does not model.
#[derive(Serialize)]
pub struct ToolChatRequest {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    pub messages: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Reasoning>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolCallFunction,
}

/// The function invocation inside a tool call; `arguments` is a JSON string,
/// as the OpenAI schema specifies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Deserialize)]
pub struct ChatChoice {
    pub message: ChatMessage,
}

/// Token counts as reported by the provider.
#[derive(Deserialize, Default, Clone, Copy)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
}

/// One server-sent chunk from a streaming `/chat/completions` response.
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
}
