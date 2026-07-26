#![forbid(unsafe_code)]
//! Provider-agnostic chat client for any OpenAI-compatible `/chat/completions`
//! endpoint. Point `ASTER_BASE_URL` / `ASTER_API_KEY` / `ASTER_MODEL` at anything
//! that speaks the OpenAI schema.

use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};

pub mod retry;
use retry::RetryWithBackoff;

mod models;
pub use models::{AssistantMessage, ChatMessage, ToolCall, ToolCallFunction};
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
    reasoning_effort: Option<String>,
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
            reasoning_effort: match env::var("ASTER_REASONING_EFFORT").ok().as_deref() {
                Some("off") | Some("none") | Some("") => Some("off".to_string()),
                Some(v) => Some(v.to_string()),
                None => Some("low".to_string()),
            },
        }
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
            messages,
            stream,
            stream_options: stream.then_some(StreamOptions {
                include_usage: true,
            }),
            seed: self.seed,
            max_tokens: self.max_tokens,
            reasoning: self.reasoning(),
        }
    }

    fn reasoning(&self) -> Option<Reasoning> {
        match self.reasoning_effort.as_deref() {
            Some("off") => Some(Reasoning {
                effort: None,
                enabled: Some(false),
            }),
            Some(effort @ ("low" | "medium" | "high")) => Some(Reasoning {
                effort: Some(effort.to_string()),
                enabled: None,
            }),
            _ => None,
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
        let prompt_chars: usize = messages.iter().map(|m| m.content.len()).sum();
        let request = self.build_request_from(&self.model, messages.to_vec(), temperature, false);

        let response = self.send_with_retry(&request, "chat request").await?;
        let body = response.text().await.context("reading response body")?;

        let parsed: ChatResponse =
            serde_json::from_str(&body).with_context(|| format!("parsing response: {body}"))?;
        let content = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
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
            .map(|c| c.message.content)
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
        let prompt_chars: usize = messages.iter().map(|m| m.to_string().len()).sum();
        let request = ToolChatRequest {
            model: self.model.clone(),
            temperature: Some(temperature),
            messages,
            tools,
            seed: self.seed,
            max_tokens: self.max_tokens,
            reasoning: self.reasoning(),
        };

        let response = self.send_with_retry(&request, "tool chat request").await?;
        let body = response.text().await.context("reading response body")?;

        let parsed: ToolChatResponse =
            serde_json::from_str(&body).with_context(|| format!("parsing response: {body}"))?;
        let message = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .context("no choices in model response")?;
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
        // Split lines on the byte buffer so a multibyte codepoint straddling two
        // network chunks is never decoded until whole.
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
                let Ok(parsed) = serde_json::from_str::<ChatStreamChunk>(data) else {
                    continue;
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
            }
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

    /// POST a chat request. Transient failures are retried by the middleware; a
    /// non-success status that survives fails here with the response body.
    async fn send_with_retry<R: serde::Serialize>(
        &self,
        request: &R,
        ctx: &str,
    ) -> Result<reqwest::Response> {
        let url = format!("{}/chat/completions", self.base_url);
        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(request)
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

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn format_api_error_prefers_openrouter_upstream_message() {
        let body = r#"{"error":{"message":"Provider returned error","code":429,"metadata":{"raw":"kimi is temporarily rate-limited upstream. Please retry shortly.","provider_name":"Moonshot AI"}}}"#;
        let msg = format_api_error(StatusCode::TOO_MANY_REQUESTS, body);
        assert_eq!(
            msg,
            "rate limited (429): kimi is temporarily rate-limited upstream. Please retry shortly."
        );
    }

    #[test]
    fn format_api_error_falls_back_to_error_message() {
        let body = r#"{"error":{"message":"invalid model id"}}"#;
        assert_eq!(
            format_api_error(StatusCode::BAD_REQUEST, body),
            "bad request (400): invalid model id"
        );
    }

    #[test]
    fn format_api_error_labels_auth_failures() {
        let msg = format_api_error(StatusCode::UNAUTHORIZED, "");
        assert_eq!(msg, "authentication failed (check your API key) (401)");
    }

    #[test]
    fn format_api_error_uses_raw_body_when_not_json() {
        let msg = format_api_error(StatusCode::INTERNAL_SERVER_ERROR, "upstream down");
        assert_eq!(msg, "provider error (500): upstream down");
    }
}
