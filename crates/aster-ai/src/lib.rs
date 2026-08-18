#![forbid(unsafe_code)]
//! Provider-agnostic chat client for any OpenAI-compatible `/chat/completions`
//! endpoint. Point `ASTER_BASE_URL` / `ASTER_API_KEY` / `ASTER_MODEL` at anything
//! that speaks the OpenAI schema.

use std::collections::BTreeMap;
use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use tokio::sync::OnceCell;

pub mod retry;
use retry::RetryWithBackoff;

mod effort;
pub use effort::Effort;

mod inline_tools;
use inline_tools::{TokenGate, split_inline_tool_calls};

mod repetition;
pub use repetition::{DEGENERATE_MSG, DegenerateOutput, RepetitionGuard, is_degenerate};

mod wire;
use wire::{carries_images, fold_system_chat, fold_system_notes, strip_image_parts};

mod models;
pub use models::{
    Annotation, AssistantMessage, ChatMessage, ContentPart, IMAGE_OMITTED, ImageUrl,
    MessageContent, ReasoningDetail, ToolCall, ToolCallFunction, UrlCitation, WebSearchPlugin,
};
use models::{
    ChatRequest, ChatResponse, ChatStreamChunk, Reasoning, StreamOptions, ToolChatRequest,
    ToolChatResponse, Usage,
};

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const CONNECT_TIMEOUT_SECS: u64 = 10;
const DEFAULT_MAX_RETRIES: u32 = 3;
const DEFAULT_DEADLINE_SECS: u64 = 180;
// Assumed $/million tokens (roughly gpt-4o-mini) when no pricing is configured;
// override with ASTER_PRICE_PROMPT_PER_M / ASTER_PRICE_COMPLETION_PER_M.
const DEFAULT_PRICE_PROMPT_PER_M: f64 = 0.15;
const DEFAULT_PRICE_COMPLETION_PER_M: f64 = 0.60;

/// Running token totals, shared behind an `Arc` so cloned clients report against
/// the same counters.
#[derive(Default)]
struct UsageCounter {
    prompt_tokens: AtomicU64,
    completion_tokens: AtomicU64,
    requests: AtomicU64,
    // Set when any request's tokens were estimated, so the snapshot is labeled honestly.
    estimated: std::sync::atomic::AtomicBool,
}

/// Rough token estimate from char count (~4 chars/token) when a provider omits usage.
fn estimate_tokens(chars: usize) -> u64 {
    (chars as u64).div_ceil(4)
}

/// A point-in-time view of token spend.
#[derive(Debug, Clone, Copy)]
pub struct UsageSnapshot {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub requests: u64,
    /// Always populated; uses configured pricing when available, else a default.
    pub estimated_cost_usd: Option<f64>,
    /// True when the cost uses default pricing or estimated tokens.
    pub cost_is_estimate: bool,
    /// True if any token count was estimated because the provider returned no usage.
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
    /// When true, non-tool requests carry the OpenRouter `web` plugin so the
    /// provider runs a web search once per request. Tool-calling requests use
    /// the `openrouter:web_search` server tool instead (see `chat.rs`).
    web_search: bool,
    /// Whether `model` takes image input, asked of the catalog once and only
    /// when a request actually carries one.
    images: Arc<OnceCell<bool>>,
    /// Extra headers to attach to every chat-completions request, used for
    /// provider app attribution (e.g. OpenRouter's `HTTP-Referer` and
    /// `X-OpenRouter-Title`). Leave empty to send a bare request.
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

    /// Build from env: `ASTER_API_KEY` (required), `ASTER_BASE_URL`, `ASTER_MODEL`,
    /// `ASTER_TIMEOUT_SECS`, `ASTER_MAX_RETRIES`.
    pub fn from_env() -> Result<Self> {
        let api_key = env::var("ASTER_API_KEY")
            .or_else(|_| env::var("OPEN_ROUTER_API_KEY"))
            .context("set ASTER_API_KEY (or OPEN_ROUTER_API_KEY)")?;
        let base_url = env::var("ASTER_BASE_URL")
            .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string());
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
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
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
                None => Some(8000),
            },
            effort: env::var("ASTER_EFFORT")
                .or_else(|_| env::var("ASTER_REASONING_EFFORT"))
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or_default(),
            web_search: env_truthy("ASTER_WEB_SEARCH"),
            images: Arc::new(OnceCell::new()),
            attribution_headers: Vec::new(),
        }
    }

    /// Builder form of [`AiClient::set_effort`], for a client built from config.
    pub fn with_effort(mut self, effort: Effort) -> Self {
        self.effort = effort;
        self
    }

    /// Builder form of [`AiClient::set_web_search`], for a client built from config.
    pub fn with_web_search(mut self, web_search: bool) -> Self {
        self.web_search = web_search;
        self
    }

    /// Builder form of [`AiClient::set_attribution_headers`]-friendly for a
    /// client built from config. Headers like `HTTP-Referer` and
    /// `X-OpenRouter-Title` attribute usage to this app on a provider's
    /// rankings and analytics.
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
        temperature: f32,
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
        temperature: f32,
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

    /// The OpenRouter `web` plugin: a forced search on every request, so only
    /// the tool-less paths take it. Tool-calling requests carry the model-invoked
    /// `openrouter:web_search` server tool instead (see `chat.rs`).
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
    pub async fn complete(&self, system: &str, user: &str, temperature: f32) -> Result<String> {
        self.complete_with(&self.model, system, user, temperature)
            .await
    }

    pub async fn complete_messages(
        &self,
        messages: &[ChatMessage],
        temperature: f32,
    ) -> Result<String> {
        let prompt_chars: usize = messages.iter().map(|m| m.content.chars()).sum();
        let mut messages = messages.to_vec();
        let images = messages.iter().any(|m| m.content.has_images());
        if images && !self.supports_images().await {
            messages.iter_mut().for_each(|m| m.content.strip_images());
        }
        let mut request = self.build_request_from(&self.model, messages, temperature, false);

        let response = match self.send_with_retry(&request, "chat request").await {
            Err(err) if images && rejected_images(&err) => {
                strip_images(&mut request.messages);
                self.send_with_retry(&request, "chat request").await?
            }
            result => result?,
        };
        let body = response.text().await.context("reading response body")?;

        let parsed: ChatResponse =
            serde_json::from_str(&body).with_context(|| format!("parsing response: {body}"))?;
        let content = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content.text().into_owned())
            .context("no choices in model response")?;
        self.record_usage(parsed.usage, prompt_chars, content.len());
        Ok(content)
    }

    pub async fn complete_with(
        &self,
        model: &str,
        system: &str,
        user: &str,
        temperature: f32,
    ) -> Result<String> {
        let request = self.build_request(model, system, user, temperature, false);

        let response = self.send_with_retry(&request, "chat request").await?;
        let body = response.text().await.context("reading response body")?;

        let parsed: ChatResponse =
            serde_json::from_str(&body).with_context(|| format!("parsing response: {body}"))?;
        let content = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content.text().into_owned())
            .context("no choices in model response")?;
        self.record_usage(parsed.usage, system.len() + user.len(), content.len());
        Ok(content)
    }

    pub async fn complete_tools(
        &self,
        messages: Vec<serde_json::Value>,
        tools: Vec<serde_json::Value>,
        temperature: f32,
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
        temperature: f32,
    ) -> Result<AssistantMessage> {
        let mut messages = fold_system_notes(messages);
        let images = self.settle_images(&mut messages).await;
        let prompt_chars: usize = messages.iter().map(|m| m.to_string().len()).sum();
        let mut request = ToolChatRequest {
            model: model.to_string(),
            temperature: Some(temperature),
            messages,
            tools,
            stream: false,
            stream_options: None,
            seed: self.seed,
            max_tokens: self.max_tokens,
            reasoning: self.reasoning(),
            plugins: Vec::new(),
        };

        let response = match self.send_with_retry(&request, "tool chat request").await {
            Err(err) if images && rejected_images(&err) => {
                tracing::debug!(model, "endpoint rejected the images; retrying without them");
                strip_image_parts(&mut request.messages);
                self.send_with_retry(&request, "tool chat request").await?
            }
            result => result?,
        };
        let body = response.text().await.context("reading response body")?;

        let parsed: ToolChatResponse =
            serde_json::from_str(&body).with_context(|| format!("parsing response: {body}"))?;
        let mut message = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .context("no choices in model response")?;
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
        if message
            .content
            .as_deref()
            .map(is_degenerate)
            .unwrap_or(false)
        {
            return Err(anyhow::Error::new(DegenerateOutput).context(DEGENERATE_MSG));
        }
        let completion_chars = message.content.as_deref().map(str::len).unwrap_or(0)
            + message
                .tool_calls
                .iter()
                .map(|t| t.function.arguments.len())
                .sum::<usize>();
        self.record_usage(parsed.usage, prompt_chars, completion_chars);
        Ok(message)
    }

    /// Streaming completion. `on_token` is called with each content delta; the
    /// full accumulated text is returned. Assumes SSE (`data: {...}` lines) and
    /// falls back to a non-streaming call when the endpoint yields no deltas.
    pub async fn complete_stream_with(
        &self,
        model: &str,
        system: &str,
        user: &str,
        temperature: f32,
        mut on_token: impl FnMut(&str),
    ) -> Result<String> {
        let request = self.build_request(model, system, user, temperature, true);

        let response = self
            .send_with_retry(&request, "streaming chat request")
            .await?;

        let mut acc = String::new();
        let mut usage: Option<Usage> = None;
        read_sse(response, |data| {
            let Ok(parsed) = serde_json::from_str::<ChatStreamChunk>(data) else {
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
        .await?;

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

    /// Streaming tool-call completion: the same contract as
    /// [`Self::complete_tools_with`], but `on_token` receives each content delta
    /// as it arrives and `on_reasoning` each plaintext thinking fragment, so a
    /// caller can show reasoning live instead of once the round closes.
    /// Tool-call fragments are reassembled by index. Falls back to a
    /// non-streaming call when the endpoint yields nothing.
    pub async fn complete_tools_stream_with(
        &self,
        model: &str,
        messages: Vec<serde_json::Value>,
        tools: Vec<serde_json::Value>,
        temperature: f32,
        mut on_token: impl FnMut(&str),
        mut on_reasoning: impl FnMut(&str),
    ) -> Result<AssistantMessage> {
        let mut messages = fold_system_notes(messages);
        let images = self.settle_images(&mut messages).await;
        let prompt_chars: usize = messages.iter().map(|m| m.to_string().len()).sum();
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
            max_tokens: self.max_tokens,
            reasoning: self.reasoning(),
            plugins: Vec::new(),
        };

        let response = match self
            .send_with_retry(&request, "streaming tool chat request")
            .await
        {
            Err(err) if images && rejected_images(&err) => {
                tracing::debug!(model, "endpoint rejected the images; retrying without them");
                strip_image_parts(&mut request.messages);
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

        read_sse(response, |data| {
            let Ok(parsed) = serde_json::from_str::<ChatStreamChunk>(data) else {
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
                let slot = partials.entry(fragment.index).or_default();
                if let Some(id) = fragment.id.filter(|s| !s.is_empty()) {
                    slot.id = id;
                }
                if let Some(function) = fragment.function {
                    if let Some(name) = function.name.filter(|s| !s.is_empty()) {
                        slot.name = name;
                    }
                    if let Some(args) = function.arguments {
                        slot.arguments.push_str(&args);
                    }
                }
            }
            true
        })
        .await?;
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
            tool_calls,
            annotations,
            reasoning_details,
        })
    }

    /// POST a chat request. Transient failures are retried by the middleware; a
    /// non-success status that survives fails here with the response body.
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
        let url = format!("{}/chat/completions", self.base_url);
        let mut post = self.http.post(&url).bearer_auth(&self.api_key);
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
        let response = post
            .json(request)
            .send()
            .await
            .with_context(|| format!("{ctx} failed"))?;
        let status = response.status();
        tracing::Span::current().record("status", status.as_u16());
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("{}", format_api_error(status, &body));
        }
        Ok(response)
    }

    async fn get_with_retry(&self, path: &str, ctx: &str) -> Result<reqwest::Response> {
        let url = format!("{}{path}", self.base_url);
        let response = self
            .http
            .get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await
            .with_context(|| format!("{ctx} failed"))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
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
        let response = self
            .get_with_retry("/models", "fetching model list")
            .await?;
        let body = response.text().await.context("reading models response")?;
        let parsed: ModelListResponse = serde_json::from_str(&body)
            .with_context(|| format!("parsing models response: {body}"))?;
        Ok(parsed.data.into_iter().map(ModelInfo::from).collect())
    }

    /// Drop images the model has already said it cannot take, and report
    /// whether any survive — the caller needs that to know whether a later
    /// rejection is worth retrying without them.
    async fn settle_images(&self, messages: &mut [serde_json::Value]) -> bool {
        if !carries_images(messages) {
            return false;
        }
        if self.supports_images().await {
            return true;
        }
        strip_image_parts(messages);
        false
    }

    /// Whether this client's model takes image input.
    ///
    /// Optimistic by design: only an endpoint that declares its modalities and
    /// leaves images out answers `false`. Most OpenAI-compatible endpoints
    /// declare nothing, and refusing images there would be worse than trying
    /// and falling back on the rejection.
    pub async fn supports_images(&self) -> bool {
        *self
            .images
            .get_or_init(|| async {
                match self.fetch_model_catalog().await {
                    Ok(catalog) => catalog
                        .into_iter()
                        .find(|m| m.id == self.model)
                        .and_then(|m| m.takes_images)
                        .unwrap_or(true),
                    Err(err) => {
                        tracing::debug!(%err, "model catalog unavailable; assuming image input");
                        true
                    }
                }
            })
            .await
    }
}

/// An endpoint that refuses an image says so in the error body. Its catalog
/// declared nothing, or declared wrongly, so the images go and the turn is
/// tried once more rather than failing outright.
fn rejected_images(err: &anyhow::Error) -> bool {
    let text = err.to_string().to_lowercase();
    ["image", "vision", "multimodal"]
        .iter()
        .any(|word| text.contains(word))
}

fn strip_images(messages: &mut [ChatMessage]) {
    for message in messages.iter_mut() {
        message.content.strip_images();
    }
}

/// One model the endpoint serves, with what it says about its own inputs.
#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub id: String,
    /// `None` when the endpoint declares no modalities, which is not the same
    /// as declaring text only.
    pub takes_images: Option<bool>,
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
        }
    }
}

/// Model-list entry from `GET /models`. Everything past `id` is OpenRouter's
/// extension and absent elsewhere, so all of it defaults.
#[derive(serde::Deserialize)]
struct ModelEntry {
    id: String,
    #[serde(default)]
    architecture: Option<Architecture>,
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

/// A tool call being reassembled from streamed fragments.
#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

/// Feed each SSE `data:` payload to `on_data`, skipping keep-alives and the
/// terminating `[DONE]`. Lines are split on the byte buffer so a multibyte
/// codepoint straddling two network chunks is never decoded until whole.
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

fn format_api_error(status: reqwest::StatusCode, body: &str) -> String {
    let label = match status.as_u16() {
        429 => "rate limited",
        401 | 403 => "authentication failed (check your API key)",
        400 => "bad request",
        404 => "model or endpoint not found",
        500..=599 => "provider error",
        _ => "request failed",
    };
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v["error"]["metadata"]["raw"]
                .as_str()
                .or_else(|| v["error"]["message"].as_str())
                .or_else(|| v["message"].as_str())
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

fn env_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

fn env_f64(key: &str) -> Option<f64> {
    env::var(key).ok().and_then(|v| v.trim().parse().ok())
}

/// Rejoin a streamed reasoning fragment with the block it belongs to. The
/// provider only accepts the sequence back when it matches what it emitted,
/// so fragments with the same kind and index have to become one block again,
/// in arrival order. Fragments without an index merge into the last block
/// of the same kind, since the provider does not give us a way to
/// distinguish them.
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

/// `ASTER_*` env flags: "1", "true", "yes", "on" (case-insensitive) are truthy.
fn env_truthy(key: &str) -> bool {
    matches!(
        env::var(key).ok().as_deref().map(str::trim),
        Some("1" | "true" | "yes" | "on")
    )
}

#[cfg(test)]
#[path = "tests/lib_test.rs"]
mod tests;
