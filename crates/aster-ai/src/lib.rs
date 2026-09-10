#![forbid(unsafe_code)]
//! Provider-agnostic chat client for any OpenAI-compatible `/chat/completions`
//! endpoint. Point `ASTER_BASE_URL` / `ASTER_API_KEY` / `ASTER_MODEL` at anything
//! that speaks the OpenAI schema.

use std::collections::BTreeMap;
use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use tokio::sync::OnceCell;

mod cloudflare;
pub mod codex;
pub mod codex_api;
mod error_log;
pub mod keys;
pub mod pkce;
pub mod router;

pub mod retry;
use retry::RetryWithBackoff;

mod effort;
pub use effort::Effort;

mod inline_tools;
use inline_tools::{TokenGate, split_inline_tool_calls};

mod tool_args;

mod repetition;
pub use repetition::{DEGENERATE_MSG, DegenerateOutput, RepetitionGuard, is_degenerate};

mod wire;
use wire::{
    apply_cache_control, carries_images, fold_system_chat, fold_system_notes, strip_image_parts,
    wants_cache_control,
};

mod models;
pub use models::{
    Annotation, AssistantMessage, ChatMessage, ContentPart, IMAGE_OMITTED, ImageUrl,
    MessageContent, ReasoningDetail, ToolCall, ToolCallFunction, UrlCitation, WebSearchPlugin,
};
use models::{
    ChatRequest, ChatResponse, ChatStreamChunk, Reasoning, StreamOptions, ToolCallDelta,
    ToolChatRequest, ToolChatResponse, Usage,
};

pub const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// Output cap sent with every request when nothing configures one.
pub const DEFAULT_MAX_TOKENS: u32 = 8000;

const DEFAULT_TIMEOUT_SECS: u64 = 300;
const CONNECT_TIMEOUT_SECS: u64 = 10;
const DEFAULT_MAX_RETRIES: u32 = 3;
const DEFAULT_DEADLINE_SECS: u64 = 180;
// Assumed $/million tokens (roughly gpt-4o-mini) when no pricing is configured;
// override with ASTER_PRICE_PROMPT_PER_M / ASTER_PRICE_COMPLETION_PER_M.
const DEFAULT_PRICE_PROMPT_PER_M: f64 = 0.15;
const DEFAULT_PRICE_COMPLETION_PER_M: f64 = 0.60;

// Images described per turn when the session model takes no image input.
// Each description is its own request, so the count is capped.
const MAX_CAPTIONS: usize = 4;

const CAPTION_PROMPT: &str = "Describe this image in detail: what it shows, any visible text, numbers, or interface elements, and anything that looks like an error or problem.";

// When a reply stops because it hit the output budget (finish_reason "length"),
// feed the partial back and ask the model to finish, so a long answer is
// stitched together instead of truncated. Each continuation has its own budget.
const MAX_CONTINUATION_PASSES: usize = 3;

#[derive(Default)]
struct UsageCounter {
    prompt_tokens: AtomicU64,
    completion_tokens: AtomicU64,
    requests: AtomicU64,
    // Set when any request's tokens were estimated, so the snapshot is labeled honestly.
    estimated: std::sync::atomic::AtomicBool,
}

fn estimate_tokens(chars: usize) -> u64 {
    (chars as u64).div_ceil(4)
}

#[derive(Debug, Clone, Copy)]
pub struct UsageSnapshot {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub requests: u64,
    pub estimated_cost_usd: Option<f64>,
    pub cost_is_estimate: bool,
    pub estimated: bool,
}

#[derive(Clone)]
pub struct AiClient {
    http: ClientWithMiddleware,
    base_url: String,
    api_key: String,
    pub model: String,
    usage: Arc<UsageCounter>,
    price_prompt_per_m: Option<f64>,
    price_completion_per_m: Option<f64>,
    seed: Option<u64>,
    max_tokens: Option<u32>,
    effort: Effort,
    web_search: bool,
    info: Arc<OnceCell<Option<ModelInfo>>>,
    attribution_headers: Vec<(String, String)>,
}

impl AiClient {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self::build(
            base_url,
            api_key,
            model,
            DEFAULT_TIMEOUT_SECS,
            DEFAULT_MAX_RETRIES,
            DEFAULT_DEADLINE_SECS,
        )
    }

    /// Build from env: a key for the endpoint (see [`keys::resolve_key`]),
    /// `ASTER_BASE_URL`, `ASTER_MODEL`, `ASTER_TIMEOUT_SECS`, `ASTER_MAX_RETRIES`.
    pub fn from_env() -> Result<Self> {
        let base_url = env::var("ASTER_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        // Resolved against the endpoint, not blindly: a key issued for the last
        // provider is rejected by the next one as an unexplained 401. A ChatGPT
        // subscription login stands in for a key on the Codex backend.
        let (api_key, _) =
            keys::resolve_key(&base_url).with_context(|| match codex_api::is_codex(&base_url) {
                true => "not signed in to ChatGPT; run `aster login codex`".to_string(),
                false => format!("set {}", keys::key_vars(&base_url).join(" or ")),
            })?;
        let model = env::var("ASTER_MODEL").unwrap_or_else(|_| "openai/gpt-4o-mini".to_string());
        let timeout_secs = env_u64("ASTER_TIMEOUT_SECS", DEFAULT_TIMEOUT_SECS);
        let max_retries = env_u64("ASTER_MAX_RETRIES", DEFAULT_MAX_RETRIES as u64) as u32;
        let deadline_secs = env_u64("ASTER_DEADLINE_SECS", DEFAULT_DEADLINE_SECS);
        Ok(Self::build(
            base_url,
            api_key,
            model,
            timeout_secs,
            max_retries,
            deadline_secs,
        ))
    }

    fn build(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        timeout_secs: u64,
        max_retries: u32,
        deadline_secs: u64,
    ) -> Self {
        // Read timeout, not total: a total timeout kills a healthy stream that
        // simply runs long, while a read timeout only fires on silence, which
        // is what a stalled provider actually looks like.
        let client = reqwest::Client::builder()
            .read_timeout(Duration::from_secs(timeout_secs))
            .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
            .build()
            .unwrap_or_default();
        let http = ClientBuilder::new(client)
            .with(RetryWithBackoff::new(
                max_retries,
                Duration::from_secs(deadline_secs),
            ))
            .build();
        Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
            usage: Arc::new(UsageCounter::default()),
            price_prompt_per_m: env_f64("ASTER_PRICE_PROMPT_PER_M"),
            price_completion_per_m: env_f64("ASTER_PRICE_COMPLETION_PER_M"),
            seed: match env::var("ASTER_SEED").ok().as_deref() {
                Some("none") | Some("off") => None,
                Some(v) => v.parse().ok(),
                None => Some(0),
            },
            max_tokens: match env::var("ASTER_MAX_TOKENS").ok().as_deref() {
                Some("0") | Some("none") | Some("off") => None,
                Some(v) => v.parse().ok(),
                None => Some(DEFAULT_MAX_TOKENS),
            },
            effort: env::var("ASTER_EFFORT")
                .or_else(|_| env::var("ASTER_REASONING_EFFORT"))
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or_default(),
            web_search: env_truthy("ASTER_WEB_SEARCH"),
            info: Arc::new(OnceCell::new()),
            attribution_headers: Vec::new(),
        }
    }

    /// Builder form of [`AiClient::set_effort`], for a client built from config.
    pub fn with_effort(mut self, effort: Effort) -> Self {
        self.effort = effort;
        self
    }

    /// Builder form of [`AiClient::set_max_tokens`], for a client built from config.
    pub fn with_max_tokens(mut self, max_tokens: Option<u32>) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Builder form of [`AiClient::set_web_search`], for a client built from config.
    pub fn with_web_search(mut self, web_search: bool) -> Self {
        self.web_search = web_search;
        self
    }

    /// Builder form of [`AiClient::set_attribution_headers`]. `HTTP-Referer` and
    /// `X-OpenRouter-Title` attribute usage to this app on a provider's rankings.
    pub fn with_attribution_headers(
        mut self,
        headers: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        self.set_attribution_headers(headers);
        self
    }

    /// Overwrite the attribution headers for later requests. Clones made before
    /// this call keep the old ones, so set it before handing the client to a task.
    pub fn set_attribution_headers(&mut self, headers: impl IntoIterator<Item = (String, String)>) {
        self.attribution_headers = headers.into_iter().collect();
    }

    /// Change the reasoning budget for later requests. Clones made before this
    /// call keep the old one, so set it before handing the client to a task.
    pub fn set_effort(&mut self, effort: Effort) {
        self.effort = effort;
    }

    pub fn effort(&self) -> Effort {
        self.effort
    }

    /// Change the web-search toggle for later requests. Clones made before this
    /// call keep the old one, so set it before handing the client to a task.
    pub fn set_web_search(&mut self, enabled: bool) {
        self.web_search = enabled;
    }

    /// Change the output cap for later requests. `None` sends no cap at all,
    /// leaving the limit to the provider. Clones made before this call keep the
    /// old one, so set it before handing the client to a task.
    pub fn set_max_tokens(&mut self, max_tokens: Option<u32>) {
        self.max_tokens = max_tokens;
    }

    pub fn max_tokens(&self) -> Option<u32> {
        self.max_tokens
    }

    pub fn web_search(&self) -> bool {
        self.web_search
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Point the client at another OpenAI-compatible endpoint, with the key
    /// that endpoint needs. Clones made before this call keep the old one.
    pub fn set_endpoint(&mut self, base_url: impl Into<String>, api_key: impl Into<String>) {
        self.base_url = base_url.into();
        self.api_key = api_key.into();
    }

    fn build_request(
        &self,
        model: &str,
        system: &str,
        user: &str,
        temperature: f64,
        stream: bool,
    ) -> ChatRequest {
        self.build_request_from(
            model,
            vec![
                ChatMessage {
                    role: "system".into(),
                    content: system.into(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: user.into(),
                },
            ],
            temperature,
            stream,
        )
    }

    fn build_request_from(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
        temperature: f64,
        stream: bool,
    ) -> ChatRequest {
        ChatRequest {
            model: model.to_string(),
            temperature: Some(temperature),
            messages: fold_system_chat(messages),
            stream,
            stream_options: stream.then_some(StreamOptions {
                include_usage: true,
            }),
            seed: self.seed,
            max_tokens: self.max_tokens,
            reasoning: self.reasoning(),
            plugins: self.plugins(),
        }
    }

    fn reasoning(&self) -> Option<Reasoning> {
        // `reasoning` is an OpenRouter-only parameter; Codex translates it to
        // `reasoning_effort`, but every other provider rejects it outright.
        if !self.base_url.contains("openrouter") && !codex_api::is_codex(&self.base_url) {
            return None;
        }
        match self.effort {
            Effort::Off => Some(Reasoning {
                effort: None,
                enabled: Some(false),
            }),
            effort => Some(Reasoning {
                effort: Some(effort.as_str().to_string()),
                enabled: None,
            }),
        }
    }

    fn plugins(&self) -> Vec<WebSearchPlugin> {
        if self.web_search {
            vec![WebSearchPlugin {
                id: "web".to_string(),
                engine: None,
                max_results: None,
                include_domains: Vec::new(),
                exclude_domains: Vec::new(),
            }]
        } else {
            Vec::new()
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn usage_snapshot(&self) -> UsageSnapshot {
        let prompt = self.usage.prompt_tokens.load(Ordering::Relaxed);
        let completion = self.usage.completion_tokens.load(Ordering::Relaxed);
        let priced_by_default =
            self.price_prompt_per_m.is_none() && self.price_completion_per_m.is_none();
        let price_prompt = self
            .price_prompt_per_m
            .unwrap_or(DEFAULT_PRICE_PROMPT_PER_M);
        let price_completion = self
            .price_completion_per_m
            .unwrap_or(DEFAULT_PRICE_COMPLETION_PER_M);
        let cost = prompt as f64 / 1e6 * price_prompt + completion as f64 / 1e6 * price_completion;
        let tokens_estimated = self.usage.estimated.load(Ordering::Relaxed);
        UsageSnapshot {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
            requests: self.usage.requests.load(Ordering::Relaxed),
            estimated_cost_usd: Some(cost),
            cost_is_estimate: priced_by_default || tokens_estimated,
            estimated: tokens_estimated,
        }
    }

    fn record_usage(&self, usage: Option<Usage>, prompt_chars: usize, completion_chars: usize) {
        self.usage.requests.fetch_add(1, Ordering::Relaxed);
        match usage {
            Some(u) => {
                self.usage
                    .prompt_tokens
                    .fetch_add(u.prompt_tokens, Ordering::Relaxed);
                self.usage
                    .completion_tokens
                    .fetch_add(u.completion_tokens, Ordering::Relaxed);
            }
            None => {
                self.usage
                    .prompt_tokens
                    .fetch_add(estimate_tokens(prompt_chars), Ordering::Relaxed);
                self.usage
                    .completion_tokens
                    .fetch_add(estimate_tokens(completion_chars), Ordering::Relaxed);
                self.usage.estimated.store(true, Ordering::Relaxed);
            }
        }
    }

    /// Single-shot completion using the client's default model.
    pub async fn complete(&self, system: &str, user: &str, temperature: f64) -> Result<String> {
        self.complete_with(&self.model, system, user, temperature)
            .await
    }

    pub async fn complete_messages(
        &self,
        messages: &[ChatMessage],
        temperature: f64,
    ) -> Result<String> {
        let prompt_chars: usize = messages.iter().map(|m| m.content.chars()).sum();
        let mut messages = messages.to_vec();
        let images = messages.iter().any(|m| m.content.has_images());
        if images && !self.supports_images().await {
            self.caption_chat_messages(&mut messages).await;
        }
        let mut request = self.build_request_from(&self.model, messages, temperature, false);
        let mut content = String::new();

        for pass in 0..=MAX_CONTINUATION_PASSES {
            let response = match self.send_with_retry(&request, "chat request").await {
                Err(err) if images && pass == 0 && rejected_images(&err) => {
                    tracing::debug!(model = %self.model, "endpoint rejected the images; describing them instead");
                    self.caption_chat_messages(&mut request.messages).await;
                    if request.messages.iter().any(|m| m.content.has_images()) {
                        request
                            .messages
                            .iter_mut()
                            .for_each(|m| m.content.strip_images());
                    }
                    self.send_with_retry(&request, "chat request").await?
                }
                result => result?,
            };
            let body = response.text().await.context("reading response body")?;

            let parsed: ChatResponse =
                serde_json::from_str(&body).with_context(|| format!("parsing response: {body}"))?;
            let choice = parsed
                .choices
                .into_iter()
                .next()
                .context("no choices in model response")?;
            content.push_str(&choice.message.content.text());
            self.record_usage(parsed.usage, prompt_chars, content.len());
            if choice.finish_reason.as_deref() != Some("length") {
                return Ok(content);
            }
            // The reply hit the output budget mid-sentence; feed the partial
            // back so the model finishes it instead of returning a cut reply.
            tracing::debug!(model = %self.model, pass, "reply hit max_tokens; continuing");
            request.messages.push(ChatMessage {
                role: "assistant".into(),
                content: choice.message.content,
            });
        }
        Ok(content)
    }

    pub async fn complete_with(
        &self,
        model: &str,
        system: &str,
        user: &str,
        temperature: f64,
    ) -> Result<String> {
        let mut request = self.build_request(model, system, user, temperature, false);
        let mut content = String::new();

        for _ in 0..=MAX_CONTINUATION_PASSES {
            let response = self.send_with_retry(&request, "chat request").await?;
            let body = response.text().await.context("reading response body")?;

            let parsed: ChatResponse =
                serde_json::from_str(&body).with_context(|| format!("parsing response: {body}"))?;
            let choice = parsed
                .choices
                .into_iter()
                .next()
                .context("no choices in model response")?;
            content.push_str(&choice.message.content.text());
            self.record_usage(parsed.usage, system.len() + user.len(), content.len());
            if choice.finish_reason.as_deref() != Some("length") {
                return Ok(content);
            }
            // The reply hit the output budget mid-sentence; feed the partial
            // back so the model finishes it instead of returning a cut reply.
            tracing::debug!(model, "reply hit max_tokens; continuing");
            request.messages.push(ChatMessage {
                role: "assistant".into(),
                content: choice.message.content,
            });
        }
        Ok(content)
    }

    pub async fn complete_tools(
        &self,
        messages: Vec<serde_json::Value>,
        tools: Vec<serde_json::Value>,
        temperature: f64,
    ) -> Result<AssistantMessage> {
        let model = self.model.clone();
        self.complete_tools_with(&model, messages, tools, temperature)
            .await
    }

    pub async fn complete_tools_with(
        &self,
        model: &str,
        messages: Vec<serde_json::Value>,
        tools: Vec<serde_json::Value>,
        temperature: f64,
    ) -> Result<AssistantMessage> {
        let mut messages = fold_system_notes(messages);
        let images = self.settle_images(&mut messages).await;
        if wants_cache_control(&self.base_url, model) {
            apply_cache_control(&mut messages);
        }
        let prompt_chars: usize = messages.iter().map(|m| m.to_string().len()).sum();
        // Tool schemas are sent with every turn and are large, so the output
        // budget has to count them as prompt.
        let request_chars = prompt_chars + json_chars(&tools);
        let mut request = ToolChatRequest {
            model: model.to_string(),
            temperature: Some(temperature),
            messages,
            tools,
            stream: false,
            stream_options: None,
            seed: self.seed,
            max_tokens: self.output_budget(request_chars).await,
            reasoning: self.reasoning(),
            plugins: Vec::new(),
        };

        let mut request_content = String::new();
        for pass in 0..=MAX_CONTINUATION_PASSES {
            let response = match self.send_with_retry(&request, "tool chat request").await {
                Err(err) if images && pass == 0 && rejected_images(&err) => {
                    tracing::debug!(
                        model,
                        "endpoint rejected the images; describing them instead"
                    );
                    self.caption_values(&mut request.messages).await;
                    if carries_images(&request.messages) {
                        strip_image_parts(&mut request.messages);
                    }
                    self.send_with_retry(&request, "tool chat request").await?
                }
                result => result?,
            };
            let body = response.text().await.context("reading response body")?;

            let parsed: ToolChatResponse =
                serde_json::from_str(&body).with_context(|| format!("parsing response: {body}"))?;
            let choice = parsed
                .choices
                .into_iter()
                .next()
                .context("no choices in model response")?;
            let mut message = choice.message;
            if message.tool_calls.is_empty()
                && let Some(content) = message.content.as_deref()
            {
                let (text, inline) = split_inline_tool_calls(content);
                if !inline.is_empty() {
                    tracing::debug!(model, calls = inline.len(), "recovered inline tool calls");
                    message.content = (!text.is_empty()).then_some(text);
                    message.tool_calls = inline;
                }
            }
            tool_args::heal_calls(&mut message.tool_calls);

            // A tool turn ends the reply; return it as-is (the caller loops).
            if !message.tool_calls.is_empty() {
                let completion_chars = message.content.as_deref().map(str::len).unwrap_or(0)
                    + message
                        .tool_calls
                        .iter()
                        .map(|t| t.function.arguments.len())
                        .sum::<usize>();
                self.record_usage(parsed.usage, prompt_chars, completion_chars);
                return Ok(message);
            }

            // Prose reply: accumulate it, and if it hit the output budget, feed
            // it back so the model finishes it instead of returning a cut reply.
            let fragment = message.content.take().unwrap_or_default();
            request_content.push_str(&fragment);
            if is_degenerate(&request_content) {
                return Err(anyhow::Error::new(DegenerateOutput).context(DEGENERATE_MSG));
            }
            self.record_usage(parsed.usage, prompt_chars, request_content.len());
            if choice.finish_reason.as_deref() != Some("length") {
                return Ok(AssistantMessage {
                    content: Some(request_content),
                    ..message
                });
            }
            tracing::debug!(model, pass, "reply hit max_tokens; continuing");
            request.messages.push(serde_json::json!({
                "role": "assistant",
                "content": request_content,
            }));
        }
        Ok(AssistantMessage {
            content: Some(request_content),
            tool_calls: Vec::new(),
            annotations: Vec::new(),
            reasoning_details: Vec::new(),
            reasoning_content: None,
        })
    }

    /// Streaming completion. `on_token` is called with each content delta; the
    /// full accumulated text is returned. Assumes SSE (`data: {...}` lines) and
    /// falls back to a non-streaming call when the endpoint yields no deltas.
    pub async fn complete_stream_with(
        &self,
        model: &str,
        system: &str,
        user: &str,
        temperature: f64,
        mut on_token: impl FnMut(&str),
    ) -> Result<String> {
        let request = self.build_request(model, system, user, temperature, true);

        let response = self
            .send_with_retry(&request, "streaming chat request")
            .await?;

        let mut acc = String::new();
        let mut usage: Option<Usage> = None;
        let mut adapt = self.sse_adapter();
        let streamed = read_sse(response, |data| {
            let Some(data) = adapt(data) else {
                return true;
            };
            let Ok(parsed) = serde_json::from_str::<ChatStreamChunk>(&data) else {
                return true;
            };
            if let Some(u) = parsed.usage {
                usage = Some(u);
            }
            if let Some(delta) = parsed
                .choices
                .into_iter()
                .next()
                .and_then(|c| c.delta.content)
                && !delta.is_empty()
            {
                acc.push_str(&delta);
                on_token(&delta);
            }
            true
        })
        .await;
        if let Err(e) = streamed {
            // Nothing reached the caller yet, so a fresh request duplicates
            // nothing; a drop after output surfaces instead of re-streaming.
            if acc.is_empty() {
                tracing::debug!(
                    model,
                    "stream died before any content: {e:#}; retrying without streaming"
                );
                return self.complete_with(model, system, user, temperature).await;
            }
            return Err(e.context("the stream dropped mid-reply"));
        }

        // Some endpoints ignore `stream` and return an empty body; fall back to a
        // non-streaming call (which records its own usage).
        if acc.is_empty() {
            tracing::debug!(
                model,
                "stream produced no content; falling back to non-streaming"
            );
            return self.complete_with(model, system, user, temperature).await;
        }
        self.record_usage(usage, system.len() + user.len(), acc.len());
        Ok(acc)
    }

    /// [`Self::complete_tools_with`], streamed: `on_token` gets each content delta and
    /// `on_reasoning` each plaintext thinking fragment. Tool-call fragments are
    /// reassembled by index; falls back to a non-streaming call when nothing yields.
    pub async fn complete_tools_stream_with(
        &self,
        model: &str,
        messages: Vec<serde_json::Value>,
        tools: Vec<serde_json::Value>,
        temperature: f64,
        mut on_token: impl FnMut(&str),
        mut on_reasoning: impl FnMut(&str),
    ) -> Result<AssistantMessage> {
        let mut messages = fold_system_notes(messages);
        let images = self.settle_images(&mut messages).await;
        if wants_cache_control(&self.base_url, model) {
            apply_cache_control(&mut messages);
        }
        let prompt_chars: usize = messages.iter().map(|m| m.to_string().len()).sum();
        let request_chars = prompt_chars + json_chars(&tools);
        let mut request = ToolChatRequest {
            model: model.to_string(),
            temperature: Some(temperature),
            messages: messages.clone(),
            tools: tools.clone(),
            stream: true,
            stream_options: Some(StreamOptions {
                include_usage: true,
            }),
            seed: self.seed,
            max_tokens: self.output_budget(request_chars).await,
            reasoning: self.reasoning(),
            plugins: Vec::new(),
        };

        let response = match self
            .send_with_retry(&request, "streaming tool chat request")
            .await
        {
            Err(err) if images && rejected_images(&err) => {
                tracing::debug!(
                    model,
                    "endpoint rejected the images; describing them instead"
                );
                self.caption_values(&mut request.messages).await;
                if carries_images(&request.messages) {
                    strip_image_parts(&mut request.messages);
                }
                self.send_with_retry(&request, "streaming tool chat request")
                    .await?
            }
            result => result?,
        };

        let mut content = String::new();
        let mut usage: Option<Usage> = None;
        let mut partials: BTreeMap<usize, PartialToolCall> = BTreeMap::new();
        let mut annotations: Vec<Annotation> = Vec::new();
        let mut reasoning_details: Vec<ReasoningDetail> = Vec::new();
        // Some models write their tool calls into the content. The gate keeps
        // that markup off the screen; the block is parsed back out below.
        let mut gate = TokenGate::default();
        // A reply that degenerates into verbatim repetition is cut off before
        // it streams to completion; `degenerate` is set and the stream dropped.
        let mut guard = RepetitionGuard::default();
        let mut degenerate: Option<&'static str> = None;

        let mut adapt = self.sse_adapter();
        let streamed = read_sse(response, |data| {
            let Some(data) = adapt(data) else {
                return true;
            };
            let Ok(parsed) = serde_json::from_str::<ChatStreamChunk>(&data) else {
                return true;
            };
            if let Some(u) = parsed.usage {
                usage = Some(u);
            }
            let Some(choice) = parsed.choices.into_iter().next() else {
                return true;
            };
            if let Some(delta) = choice.delta.content.filter(|d| !d.is_empty()) {
                content.push_str(&delta);
                // The guard sees the raw delta, before the gate strips tool
                // markup, so suppressed markup cannot hide repetition.
                if guard.feed(&delta) {
                    degenerate = Some(DEGENERATE_MSG);
                    return false;
                }
                gate.feed(&delta, &mut on_token);
            }
            if !choice.delta.annotations.is_empty() {
                annotations = choice.delta.annotations;
            }
            if let Some(thinking) = choice
                .delta
                .reasoning_content
                .as_deref()
                .filter(|s| !s.is_empty())
            {
                on_reasoning(thinking);
            }
            for fragment in choice.delta.reasoning_details {
                if let Some(delta) = fragment
                    .text
                    .as_deref()
                    .or(fragment.summary.as_deref())
                    .filter(|s| !s.is_empty())
                {
                    on_reasoning(delta);
                }
                merge_reasoning(&mut reasoning_details, fragment);
            }
            for fragment in choice.delta.tool_calls {
                merge_tool_call(&mut partials, fragment);
            }
            true
        })
        .await;
        if let Err(e) = streamed {
            // Safe to redo only while the caller has seen nothing: after
            // visible output, a retry would duplicate what is on screen.
            if content.is_empty() && partials.is_empty() && reasoning_details.is_empty() {
                tracing::debug!(
                    model,
                    "stream died before any content: {e:#}; retrying without streaming"
                );
                return self
                    .complete_tools_with(model, messages, tools, temperature)
                    .await;
            }
            return Err(e.context("the stream dropped mid-reply"));
        }
        gate.finish(&mut on_token);
        if let Some(msg) = degenerate {
            return Err(anyhow::Error::new(DegenerateOutput).context(msg));
        }

        if content.is_empty() && partials.is_empty() {
            tracing::debug!(
                model,
                "tool stream produced nothing; falling back to non-streaming"
            );
            return self
                .complete_tools_with(model, messages, tools, temperature)
                .await;
        }

        let mut tool_calls: Vec<ToolCall> = partials
            .into_iter()
            .filter(|(_, p)| !p.name.is_empty())
            .map(|(index, p)| ToolCall {
                // Some providers omit the id on streamed fragments; it only has to
                // match results back to calls, so the index serves.
                id: if p.id.is_empty() {
                    format!("call_{index}")
                } else {
                    p.id
                },
                kind: "function".to_string(),
                function: ToolCallFunction {
                    name: p.name,
                    arguments: p.arguments,
                },
            })
            .collect();

        if tool_calls.is_empty() {
            let (text, inline) = split_inline_tool_calls(&content);
            if !inline.is_empty() {
                tracing::debug!(model, calls = inline.len(), "recovered inline tool calls");
                content = text;
                tool_calls = inline;
            }
        }
        tool_args::heal_calls(&mut tool_calls);
        // Second net for a whole response that repeats itself, when the stream
        // guard never saw enough chunks (single-delta or non-streamed replies).
        if is_degenerate(&content) {
            return Err(anyhow::Error::new(DegenerateOutput).context(DEGENERATE_MSG));
        }

        let completion_chars = content.len()
            + tool_calls
                .iter()
                .map(|t| t.function.arguments.len())
                .sum::<usize>();
        self.record_usage(usage, prompt_chars, completion_chars);

        Ok(AssistantMessage {
            content: (!content.is_empty()).then_some(content),
            // Already streamed as reasoning; history replays reasoning_details.
            reasoning_content: None,
            tool_calls,
            annotations,
            reasoning_details,
        })
    }

    /// POST a chat request. A non-success status that survives the retry middleware
    /// fails here with the response body. The Codex backend is translated to the
    /// Responses API and back; streamed events go through [`Self::sse_adapter`].
    #[tracing::instrument(
        name = "model_request",
        skip_all,
        fields(op = ctx, model = %self.model, status = tracing::field::Empty)
    )]
    async fn send_with_retry<R: serde::Serialize>(
        &self,
        request: &R,
        ctx: &str,
    ) -> Result<reqwest::Response> {
        let codex = codex_api::is_codex(&self.base_url);
        let url = if codex {
            codex_api::endpoint(&self.base_url)
        } else {
            format!("{}/chat/completions", self.base_url)
        };
        let mut body = serde_json::to_value(request).context("serializing chat request")?;
        if codex {
            body = codex_api::translate_request(&body);
        }
        // The ChatGPT backend rejects `stream: false` outright (400 "Stream
        // must be set to true"), so a non-streaming caller gets a stream
        // forced on here and reassembled into one body below.
        let assemble = codex && body["stream"] != serde_json::Value::Bool(true);
        if assemble {
            body["stream"] = serde_json::Value::Bool(true);
        }

        let mut bearer = self.bearer().await?;
        let mut response = self
            .post_json(&url, &bearer, codex, &body)
            .await
            .with_context(|| format!("{ctx} failed"))?;
        // The backend can expire a subscription token before its JWT `exp`
        // says so; one forced refresh and resend covers that.
        if codex && response.status() == reqwest::StatusCode::UNAUTHORIZED {
            tracing::info!("codex rejected the access token; refreshing and retrying");
            bearer = codex::refresh_access_token(&home_dir()?).await?;
            response = self
                .post_json(&url, &bearer, codex, &body)
                .await
                .with_context(|| format!("{ctx} failed"))?;
        }
        let status = response.status();
        tracing::Span::current().record("status", status.as_u16());
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            if let Ok(home) = home_dir() {
                error_log::log_request_failure(
                    &home,
                    &self.model,
                    &self.base_url,
                    status.as_u16(),
                    &text,
                    &body,
                );
            }
            if codex && status == reqwest::StatusCode::UNAUTHORIZED {
                anyhow::bail!("{}", codex::LOGIN_EXPIRED);
            }
            anyhow::bail!("{}", format_api_error(status, &text));
        }
        // A forced stream buffers here and hands the caller one translated
        // chat-completions body. Streams the caller asked for pass through
        // untouched: their SSE events are translated per event by
        // [`Self::sse_adapter`], and reading the body here would buffer the
        // whole stream before the first token reaches the caller.
        if assemble {
            let text = response.text().await.context("reading response body")?;
            let translated = codex_api::assemble_stream(&text).with_context(|| {
                let skip = text.chars().count().saturating_sub(400);
                let tail: String = text.chars().skip(skip).collect();
                format!("no completed response in the codex stream: {tail}")
            })?;
            let rebuilt = http::Response::builder()
                .status(status)
                .body(translated.to_string().into_bytes())
                .context("rebuilding translated response")?;
            return Ok(reqwest::Response::from(rebuilt));
        }
        Ok(response)
    }

    async fn post_json(
        &self,
        url: &str,
        bearer: &str,
        codex: bool,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response> {
        let adapted;
        let body = if cloudflare::is_workers_ai(&self.base_url) {
            adapted = cloudflare::adapt_request(body);
            &adapted
        } else {
            body
        };
        let mut post = self.http.post(url).bearer_auth(bearer);
        if codex {
            post = post.header("OpenAI-Beta", "responses=experimental");
            if let Some(account_id) = self.codex_account_id() {
                post = post.header("chatgpt-account-id", account_id);
            }
        }
        if !self.attribution_headers.is_empty() {
            let mut headers = HeaderMap::new();
            for (name, value) in &self.attribution_headers {
                let Ok(name) = HeaderName::try_from(name) else {
                    continue;
                };
                let Ok(value) = HeaderValue::from_str(value) else {
                    continue;
                };
                headers.insert(name, value);
            }
            post = post.headers(headers);
        }
        post.json(body)
            .send()
            .await
            .map_err(|err| network_error(url, err))
    }

    async fn bearer(&self) -> Result<String> {
        match codex_api::is_codex(&self.base_url) {
            false => Ok(self.api_key.clone()),
            true => codex::ensure_access_token(&home_dir()?).await,
        }
    }

    fn codex_account_id(&self) -> Option<String> {
        codex::load(&home_dir().ok()?)
            .and_then(|auth| auth.tokens)
            .and_then(|tokens| tokens.account_id)
    }

    fn sse_adapter(&self) -> SseAdapter<'_> {
        if codex_api::is_codex(&self.base_url) {
            let mut translator = codex_api::StreamTranslator::default();
            Box::new(move |data| translator.event(data))
        } else {
            Box::new(|data| Some(data.to_string()))
        }
    }

    async fn get_with_retry(&self, path: &str, ctx: &str) -> Result<reqwest::Response> {
        self.get_url(&format!("{}{path}", self.base_url), ctx).await
    }

    async fn get_url(&self, url: &str, ctx: &str) -> Result<reqwest::Response> {
        let response = self
            .http
            .get(url)
            .bearer_auth(self.bearer().await?)
            .send()
            .await
            .with_context(|| format!("{ctx} failed"))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            if let Ok(home) = home_dir() {
                error_log::log_request_failure(
                    &home,
                    &self.model,
                    &self.base_url,
                    status.as_u16(),
                    &body,
                    &serde_json::Value::Null,
                );
            }
            anyhow::bail!("{}", format_api_error(status, &body));
        }
        Ok(response)
    }

    /// Fetch available model IDs from the provider's `/models` endpoint, sorted.
    pub async fn fetch_models(&self) -> Result<Vec<String>> {
        let mut ids: Vec<String> = self
            .fetch_model_catalog()
            .await?
            .into_iter()
            .map(|m| m.id)
            .collect();
        ids.sort();
        Ok(ids)
    }

    /// As [`AiClient::fetch_models`], with the capabilities the endpoint declares
    /// alongside each ID. Order is the endpoint's own.
    pub async fn fetch_model_catalog(&self) -> Result<Vec<ModelInfo>> {
        // The Codex backend has no `/models`; the catalog's shortlist stands
        // in so pickers and capability checks work without a doomed probe.
        if codex_api::is_codex(&self.base_url) {
            return Ok(keys::catalog_models(&self.base_url)
                .into_iter()
                .map(|id| ModelInfo {
                    id,
                    takes_images: None,
                    context_window: None,
                })
                .collect());
        }
        if cloudflare::is_workers_ai(&self.base_url) {
            return self.fetch_workers_ai_models().await;
        }
        let response = self
            .get_with_retry("/models", "fetching model list")
            .await?;
        let body = response.text().await.context("reading models response")?;
        let parsed: ModelListResponse = serde_json::from_str(&body)
            .with_context(|| format!("parsing models response: {body}"))?;
        let strip = Self::lists_resource_names(&self.base_url);
        Ok(parsed
            .data
            .into_iter()
            .map(ModelInfo::from)
            .map(|mut m| {
                if strip && let Some(rest) = m.id.strip_prefix("models/") {
                    m.id = rest.to_string();
                }
                m
            })
            .collect())
    }

    /// Gemini lists its models as resource names (`models/gemini-3.1-pro`) but
    /// takes the bare id in a request. Left as listed, every id a picker offers
    /// would look unserved and be filtered away.
    fn lists_resource_names(base_url: &str) -> bool {
        base_url.contains("generativelanguage.googleapis.com")
    }

    /// Workers AI pages its own model list, so it is walked until a short page
    /// says the catalog is done. A search that answers nothing or errors falls
    /// back to the gist catalog, so the picker still has a list.
    async fn fetch_workers_ai_models(&self) -> Result<Vec<ModelInfo>> {
        let mut models = Vec::new();
        for page in 1..=cloudflare::MAX_PAGES {
            let url = cloudflare::models_url(&self.base_url, page);
            let Ok(response) = self.get_url(&url, "fetching model list").await else {
                return Ok(self.remote_catalog().await);
            };
            let body = response.text().await.context("reading models response")?;
            let batch = cloudflare::parse_models(&body)?;
            let done = batch.len() < cloudflare::PER_PAGE;
            models.extend(batch);
            if done {
                break;
            }
        }
        if models.is_empty() {
            return Ok(self.remote_catalog().await);
        }
        Ok(models)
    }

    /// The gist copy of the catalog, so it can be updated without a rebuild.
    async fn remote_catalog(&self) -> Vec<ModelInfo> {
        let Ok(response) = self
            .get_url(cloudflare::GIST_MODELS_URL, "fetching the model catalog")
            .await
        else {
            return Vec::new();
        };
        let Ok(body) = response.text().await else {
            return Vec::new();
        };
        cloudflare::parse_catalog(&body).unwrap_or_default()
    }

    async fn settle_images(&self, messages: &mut [serde_json::Value]) -> bool {
        if !carries_images(messages) {
            return false;
        }
        if self.supports_images().await {
            return true;
        }
        // Captioning swaps the images for text. When it fails the images stay
        // in place: the catalog may simply be wrong, and if the provider does
        // reject them the retry paths caption again and only then strip.
        self.caption_values(messages).await;
        carries_images(messages)
    }

    /// The model to describe images with when the session model takes no image
    /// input. Override with `ASTER_VISION_MODEL`.
    fn vision_model_name() -> String {
        env::var("ASTER_VISION_MODEL")
            .ok()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| "openai/gpt-4o-mini".to_string())
    }

    async fn describe_image(&self, url: &str) -> Result<String> {
        let vision = Self::vision_model_name();
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: MessageContent::Parts(vec![
                ContentPart::Text {
                    text: CAPTION_PROMPT.to_string(),
                },
                ContentPart::ImageUrl {
                    image_url: ImageUrl {
                        url: url.to_string(),
                    },
                },
            ]),
        }];
        let request = self.build_request_from(&vision, messages, 0.0, false);
        let response = self.send_with_retry(&request, "image description").await?;
        let body = response.text().await.context("reading response body")?;
        let parsed: ChatResponse =
            serde_json::from_str(&body).with_context(|| format!("parsing response: {body}"))?;
        let content = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content.text().into_owned())
            .context("no choices in model response")?;
        self.record_usage(
            parsed.usage,
            CAPTION_PROMPT.len() + url.len(),
            content.len(),
        );
        Ok(content)
    }

    async fn caption_chat_messages(&self, messages: &mut [ChatMessage]) {
        let urls: Vec<String> = messages
            .iter()
            .filter_map(|m| match &m.content {
                MessageContent::Parts(parts) => Some(parts),
                MessageContent::Text(_) => None,
            })
            .flatten()
            .filter_map(|p| match p {
                ContentPart::ImageUrl { image_url } => Some(image_url.url.clone()),
                ContentPart::Text { .. } => None,
            })
            .collect();
        let Some(descriptions) = self.describe_all(&urls).await else {
            // The vision model failed; leave the images for the provider to
            // take or reject rather than blanking them out.
            return;
        };
        let mut descriptions = descriptions.into_iter();
        for message in messages.iter_mut() {
            let MessageContent::Parts(parts) = &mut message.content else {
                continue;
            };
            for part in parts.iter_mut() {
                if matches!(part, ContentPart::ImageUrl { .. }) {
                    let text = descriptions.next().unwrap_or_default();
                    *part = ContentPart::Text { text };
                }
            }
            if !message.content.has_images() {
                message.content = MessageContent::Text(message.content.text().into_owned());
            }
        }
    }

    async fn caption_values(&self, messages: &mut [serde_json::Value]) {
        let urls: Vec<String> = messages
            .iter()
            .filter_map(|m| m.get("content")?.as_array())
            .flatten()
            .filter(|p| p.get("type").and_then(serde_json::Value::as_str) == Some("image_url"))
            .filter_map(|p| p.get("image_url")?.get("url")?.as_str().map(str::to_string))
            .collect();
        let Some(descriptions) = self.describe_all(&urls).await else {
            // The vision model failed; leave the images for the provider to
            // take or reject rather than blanking them out.
            return;
        };
        let mut descriptions = descriptions.into_iter();
        for message in messages.iter_mut() {
            let Some(parts) = message
                .get_mut("content")
                .and_then(serde_json::Value::as_array_mut)
            else {
                continue;
            };
            for part in parts.iter_mut() {
                if part.get("type").and_then(serde_json::Value::as_str) == Some("image_url") {
                    let text = descriptions.next().unwrap_or_default();
                    *part = serde_json::json!({"type": "text", "text": text});
                }
            }
        }
    }

    /// Describe each image with the vision model, in order. Images past
    /// [`MAX_CAPTIONS`] become [`IMAGE_OMITTED`]. A failed description returns
    /// [`None`]; callers keep the images in place when that happens, so a
    /// wrong catalog or a dead vision model never blanks an image the
    /// provider could have taken.
    async fn describe_all(&self, urls: &[String]) -> Option<Vec<String>> {
        let mut out = Vec::with_capacity(urls.len());
        for url in urls.iter().take(MAX_CAPTIONS) {
            match self.describe_image(url).await {
                Ok(description) => out.push(format!("[image: {description}]")),
                Err(err) => {
                    tracing::debug!(%err, "vision model failed");
                    return None;
                }
            }
        }
        out.extend(
            urls.iter()
                .skip(MAX_CAPTIONS)
                .map(|_| IMAGE_OMITTED.to_string()),
        );
        Some(out)
    }

    /// Whether this client's model takes image input. Optimistic: only an endpoint
    /// that declares its modalities and leaves images out answers `false`, since most
    /// declare nothing and refusing there is worse than trying.
    pub async fn supports_images(&self) -> bool {
        self.model_info()
            .await
            .as_ref()
            .and_then(|m| m.takes_images)
            .unwrap_or(true)
    }

    /// What the endpoint says about this client's model, asked once. [`None`]
    /// when the endpoint cannot be reached or does not list the model.
    async fn model_info(&self) -> &Option<ModelInfo> {
        self.info
            .get_or_init(|| async {
                match self.fetch_model_catalog().await {
                    Ok(catalog) => catalog.into_iter().find(|m| m.id == self.model),
                    Err(err) => {
                        tracing::debug!(%err, "model catalog unavailable; assuming no limits");
                        None
                    }
                }
            })
            .await
    }

    /// The output cap to send with a prompt of `prompt_chars`. A small context
    /// window is spent mostly on the prompt, and providers count the cap
    /// against the window before generating, so asking for the full default
    /// gets the whole request refused rather than a shorter answer.
    async fn output_budget(&self, prompt_chars: usize) -> Option<u32> {
        let want = self.max_tokens?;
        let Some(window) = self.model_info().await.as_ref()?.context_window else {
            return Some(want);
        };
        let prompt = (prompt_chars / CHARS_PER_TOKEN) as u32;
        let room = window.saturating_sub(prompt).saturating_sub(WINDOW_SLACK);
        Some(want.min(room).max(MIN_OUTPUT_TOKENS))
    }
}

fn json_chars(values: &[serde_json::Value]) -> usize {
    values.iter().map(|v| v.to_string().len()).sum()
}

/// Rough bytes per token, for sizing a request against a context window. Only
/// ever an estimate: no tokenizer here is the provider's.
const CHARS_PER_TOKEN: usize = 4;

/// Held back so an under-counted prompt still leaves the cap inside the window.
const WINDOW_SLACK: u32 = 512;

/// Below this an answer is truncated mid-sentence, which reads worse than the
/// provider's own refusal, so the cap never drops further.
const MIN_OUTPUT_TOKENS: u32 = 512;

fn rejected_images(err: &anyhow::Error) -> bool {
    let text = err.to_string().to_lowercase();
    ["image", "vision", "multimodal"]
        .iter()
        .any(|word| text.contains(word))
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub id: String,
    pub takes_images: Option<bool>,
    /// Total tokens the model reads and writes in one call, when the endpoint
    /// says. [`None`] means unknown, not unlimited.
    pub context_window: Option<u32>,
}

impl From<ModelEntry> for ModelInfo {
    fn from(entry: ModelEntry) -> Self {
        let takes_images = entry
            .architecture
            .filter(|a| !a.input_modalities.is_empty())
            .map(|a| a.input_modalities.iter().any(|m| m == "image"));
        Self {
            id: entry.id,
            takes_images,
            context_window: entry.context_length,
        }
    }
}

#[derive(serde::Deserialize)]
struct ModelEntry {
    id: String,
    #[serde(default)]
    architecture: Option<Architecture>,
    #[serde(default)]
    context_length: Option<u32>,
}

#[derive(serde::Deserialize)]
struct Architecture {
    #[serde(default)]
    input_modalities: Vec<String>,
}

#[derive(serde::Deserialize)]
struct ModelListResponse {
    data: Vec<ModelEntry>,
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

fn merge_tool_call(partials: &mut BTreeMap<usize, PartialToolCall>, fragment: ToolCallDelta) {
    let index = match partials.get(&fragment.index) {
        Some(slot) if !same_call(slot, &fragment) => fresh_index(partials),
        _ => fragment.index,
    };
    let slot = partials.entry(index).or_default();
    if let Some(id) = fragment.id.filter(|s| !s.is_empty()) {
        slot.id = id;
    }
    if let Some(function) = fragment.function {
        if let Some(name) = function.name.filter(|s| !s.is_empty()) {
            slot.name = name;
        }
        if let Some(args) = function.arguments {
            // Some endpoints resend the whole argument string in a later chunk
            // instead of the next piece; a fragment that already begins with
            // everything gathered so far is that snapshot, not more of it.
            if !slot.arguments.is_empty() && args.starts_with(slot.arguments.as_str()) {
                slot.arguments = args;
            } else {
                slot.arguments.push_str(&args);
            }
        }
    }
}

fn same_call(slot: &PartialToolCall, fragment: &ToolCallDelta) -> bool {
    let id_matches = fragment
        .id
        .as_deref()
        .filter(|s| !s.is_empty())
        .is_none_or(|id| slot.id.is_empty() || slot.id == id);
    let name_matches = fragment
        .function
        .as_ref()
        .and_then(|f| f.name.as_deref())
        .filter(|s| !s.is_empty())
        .is_none_or(|name| slot.name.is_empty() || slot.name == name);
    id_matches && name_matches
}

fn fresh_index(partials: &BTreeMap<usize, PartialToolCall>) -> usize {
    partials.last_key_value().map_or(0, |(&key, _)| key + 1)
}

async fn read_sse(
    response: reqwest::Response,
    mut on_data: impl FnMut(&str) -> bool,
) -> Result<()> {
    let mut buf: Vec<u8> = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.context("reading stream chunk")?;
        buf.extend_from_slice(&bytes);

        // Keep trailing partial bytes for the next chunk.
        while let Some(nl) = buf.iter().position(|&b| b == b'\n') {
            let line_bytes: Vec<u8> = buf.drain(..=nl).collect();
            let line = String::from_utf8_lossy(&line_bytes[..nl]);
            let line = line.trim();
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            // The callback reports false to abort: dropping the bytes stream
            // mid-way cancels the in-flight request and stops the token burn.
            if !on_data(data) {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Transport-level failures (DNS, refused, timeout) get one plain sentence
/// instead of the full error chain.
fn network_error(url: &str, err: reqwest_middleware::Error) -> anyhow::Error {
    let reqwest_middleware::Error::Reqwest(inner) = &err else {
        return err.into();
    };
    let host = inner
        .url()
        .and_then(|u| u.host_str())
        .map(str::to_owned)
        .or_else(|| reqwest::Url::parse(url).ok()?.host_str().map(str::to_owned))
        .unwrap_or_else(|| "the provider".into());
    let lead = if inner.is_timeout() {
        format!("The request to {host} timed out. Check your internet connection and try again.")
    } else if inner.is_connect() {
        format!("Couldn't reach {host}. Check your internet connection and try again.")
    } else {
        return err.into();
    };
    anyhow!("{lead}")
}

fn format_api_error(status: reqwest::StatusCode, body: &str) -> String {
    let label = match status.as_u16() {
        429 => "rate limited",
        401 | 403 => "authentication failed (check your API key)",
        400 => "bad request",
        404 => "model or endpoint not found",
        500..=599 => "provider error",
        _ => "request failed",
    };
    // Every wire shape seen in the wild: OpenAI `error.message`, OpenRouter's
    // upstream `error.metadata.raw`, bare `error`/`message` strings, the
    // ChatGPT backend's FastAPI `detail` (a string, or a list of {msg}), and
    // Cloudflare's `errors` envelope.
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v["error"]["metadata"]["raw"]
                .as_str()
                .or_else(|| v["error"]["message"].as_str())
                .or_else(|| v["error"].as_str())
                .or_else(|| v["message"].as_str())
                .or_else(|| v["detail"].as_str())
                .or_else(|| v["detail"][0]["msg"].as_str())
                .or_else(|| v["errors"][0]["message"].as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            let t = body.trim();
            (!t.is_empty()).then(|| t.to_string())
        });
    match detail {
        Some(d) => format!("{label} ({}): {d}", status.as_u16()),
        None => format!("{label} ({})", status.as_u16()),
    }
}

type SseAdapter<'a> = Box<dyn FnMut(&str) -> Option<String> + Send + 'a>;

pub fn home_dir() -> Result<std::path::PathBuf> {
    env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .context("resolving the home directory")
}

fn env_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

fn env_f64(key: &str) -> Option<f64> {
    env::var(key).ok().and_then(|v| v.trim().parse().ok())
}

fn merge_reasoning(out: &mut Vec<ReasoningDetail>, fragment: ReasoningDetail) {
    let existing = out
        .iter_mut()
        .rev()
        .find(|d| d.kind == fragment.kind && d.index == fragment.index);
    let Some(slot) = existing else {
        out.push(fragment);
        return;
    };
    extend(&mut slot.text, fragment.text);
    extend(&mut slot.summary, fragment.summary);
    extend(&mut slot.data, fragment.data);
    if fragment.signature.is_some() {
        slot.signature = fragment.signature;
    }
    if fragment.id.is_some() {
        slot.id = fragment.id;
    }
}

fn extend(slot: &mut Option<String>, more: Option<String>) {
    let Some(more) = more else { return };
    match slot {
        Some(existing) => existing.push_str(&more),
        None => *slot = Some(more),
    }
}

fn env_truthy(key: &str) -> bool {
    matches!(
        env::var(key).ok().as_deref().map(str::trim),
        Some("1" | "true" | "yes" | "on")
    )
}

#[cfg(test)]
#[path = "tests/lib_test.rs"]
mod tests;
