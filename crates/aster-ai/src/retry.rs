//! Retry middleware honoring server backpressure: waits timed from `Retry-After` /
//! `x-ratelimit-reset`, a rate-limited `403` treated as transient, retries bounded
//! by attempt count and a deadline. `reqwest-retry`'s default does none of it.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use http::Extensions;
use reqwest::header::{HeaderMap, RETRY_AFTER};
use reqwest::{Request, Response, StatusCode};
use reqwest_middleware::{Middleware, Next, Result};

const MAX_WAIT: Duration = Duration::from_secs(60);
const BACKOFF_BASE_MS: u64 = 500;

/// Retries transient failures with header-aware delays, capped by `max_retries`
/// and `total_deadline` of wall-clock across all attempts.
pub struct RetryWithBackoff {
    max_retries: u32,
    total_deadline: Duration,
}

impl RetryWithBackoff {
    pub fn new(max_retries: u32, total_deadline: Duration) -> Self {
        Self {
            max_retries: max_retries.max(1),
            total_deadline,
        }
    }
}

#[async_trait::async_trait]
impl Middleware for RetryWithBackoff {
    async fn handle(&self, req: Request, ext: &mut Extensions, next: Next<'_>) -> Result<Response> {
        self.run(req, next, ext).await
    }
}

impl RetryWithBackoff {
    async fn run<'a>(
        &'a self,
        req: Request,
        next: Next<'a>,
        ext: &'a mut Extensions,
    ) -> Result<Response> {
        let start = SystemTime::now();
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            // A non-cloneable (streaming) body can't be replayed; send it once.
            let Some(dup) = req.try_clone() else {
                return next.run(req, ext).await;
            };
            let result = next.clone().run(dup, ext).await;

            let wanted_delay = match &result {
                Ok(resp) if is_retryable(resp) => {
                    Some(header_delay(resp.headers()).unwrap_or_else(|| backoff(attempt)))
                }
                Ok(_) => None,
                Err(_) => Some(backoff(attempt)),
            };

            let elapsed = start.elapsed().unwrap_or_default();
            match wanted_delay {
                Some(delay) if attempt <= self.max_retries && elapsed < self.total_deadline => {
                    let remaining = self.total_deadline.saturating_sub(elapsed);
                    let delay = delay.min(MAX_WAIT).min(remaining);
                    tracing::warn!(attempt, ?delay, "retrying after backpressure");
                    tokio::time::sleep(delay).await;
                }
                _ => return result,
            }
        }
    }
}

fn is_retryable(resp: &Response) -> bool {
    let status = resp.status();
    if status.as_u16() == 408 || status.is_server_error() {
        return true;
    }
    matches!(
        status,
        StatusCode::TOO_MANY_REQUESTS | StatusCode::FORBIDDEN
    ) && has_retry_hint(resp.headers())
}

fn has_retry_hint(headers: &HeaderMap) -> bool {
    headers.contains_key(RETRY_AFTER)
        || headers
            .keys()
            .any(|k| k.as_str().starts_with("x-ratelimit"))
}

fn header_delay(headers: &HeaderMap) -> Option<Duration> {
    if let Some(secs) = headers
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
    {
        return Some(Duration::from_secs(secs));
    }
    let reset = headers
        .get("x-ratelimit-reset")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    reset_delay(reset, now)
}

fn reset_delay(reset: u64, now: u64) -> Option<Duration> {
    // Anything an order of magnitude past "now" is a finer unit, not the year 55000.
    let reset_secs = if reset > now.saturating_mul(10) {
        reset / 1_000
    } else {
        reset
    };
    if reset_secs > now {
        return Some(Duration::from_secs(reset_secs - now));
    }
    // Below `now` it is not an epoch at all but a relative delay.
    (reset_secs > 0 && reset_secs < 3_600).then(|| Duration::from_secs(reset_secs))
}

fn backoff(attempt: u32) -> Duration {
    let base = BACKOFF_BASE_MS.saturating_mul(1u64 << (attempt - 1).min(6));
    let jitter = (attempt as u64).wrapping_mul(97) % 250;
    Duration::from_millis(base + jitter)
}

#[cfg(test)]
#[path = "tests/retry_test.rs"]
mod tests;
