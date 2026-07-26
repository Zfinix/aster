//! Retry middleware honoring server backpressure: times waits from `Retry-After` /
//! `x-ratelimit-reset`, treats a rate-limited `403` (GitHub's secondary limit) as
//! transient, and bounds retries by attempt count and a total deadline.
//! (`reqwest-retry`'s default ignores those headers and treats every `403` as fatal.)

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
    // 429 and 403 (GitHub's secondary limit) are retryable only with a hint header.
    // A bare 429 is a permanent condition dressed as a rate limit (e.g. OpenAI
    // `insufficient_quota`) that never succeeds on retry.
    matches!(
        status,
        StatusCode::TOO_MANY_REQUESTS | StatusCode::FORBIDDEN
    ) && has_retry_hint(resp.headers())
}

/// True if a retry could succeed: a `Retry-After` or any `x-ratelimit-*` header.
fn has_retry_hint(headers: &HeaderMap) -> bool {
    headers.contains_key(RETRY_AFTER)
        || headers
            .keys()
            .any(|k| k.as_str().starts_with("x-ratelimit"))
}

/// Server-requested delay: `Retry-After` seconds, or the gap until
/// `x-ratelimit-reset` (epoch seconds). The HTTP-date form is not parsed.
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
    (reset > now).then(|| Duration::from_secs(reset - now))
}

fn backoff(attempt: u32) -> Duration {
    let base = BACKOFF_BASE_MS.saturating_mul(1u64 << (attempt - 1).min(6));
    let jitter = (attempt as u64).wrapping_mul(97) % 250;
    Duration::from_millis(base + jitter)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                reqwest::header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        h
    }

    #[test]
    fn retry_after_seconds_is_honored() {
        let d = header_delay(&headers(&[("retry-after", "5")])).unwrap();
        assert_eq!(d, Duration::from_secs(5));
    }

    #[test]
    fn ratelimit_reset_waits_until_reset() {
        let future = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 30;
        let d = header_delay(&headers(&[("x-ratelimit-reset", &future.to_string())])).unwrap();
        assert!(d.as_secs() > 25 && d.as_secs() <= 30);
    }

    #[test]
    fn http_date_retry_after_falls_through_to_none() {
        let d = header_delay(&headers(&[(
            "retry-after",
            "Wed, 21 Oct 2026 07:28:00 GMT",
        )]));
        assert!(d.is_none());
    }

    #[test]
    fn retry_hint_distinguishes_rate_limit_from_permanent_error() {
        assert!(has_retry_hint(&headers(&[("retry-after", "60")])));
        assert!(has_retry_hint(&headers(&[("x-ratelimit-remaining", "0")])));
        assert!(has_retry_hint(&headers(&[(
            "x-ratelimit-reset-requests",
            "1s"
        )])));
        assert!(!has_retry_hint(&headers(&[])));
        assert!(!has_retry_hint(&headers(&[(
            "content-type",
            "application/json"
        )])));
    }
}
