//! Photon 2.0's REST API: the calls the bridge makes, their retries, and the
//! errors they come back with.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde_json::{Value, json};
use ulid::Ulid;

/// Retries for a send or upload. Polls are not retried in place; the next
/// poll is the retry.
const SEND_ATTEMPTS: usize = 3;

/// The longest a Retry-After is honoured before the next try anyway.
pub(super) const RETRY_AFTER_CAP: Duration = Duration::from_secs(60);

/// The largest inbound file staged for the agent, and the largest screenshot
/// sent back. Both are far above a phone photo or screenshot.
const ATTACHMENT_LIMIT: usize = 20 * 1024 * 1024;

/// Whether the agent shows as typing in a Space.
pub(super) enum Typing {
    Started,
    Stopped,
}

/// The upload body Photon takes: JSON metadata, then the file bytes, as the
/// two parts of one `multipart/related` request.
pub(crate) fn related_body(
    boundary: &str,
    metadata: &Value,
    content_type: &str,
    bytes: &[u8],
) -> Vec<u8> {
    let mut body = Vec::with_capacity(bytes.len() + 256);
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Type: application/json\r\n\r\n{metadata}\r\n").as_bytes(),
    );
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Type: {content_type}\r\n\r\n").as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

/// A file extension for staged media, from the Content-Type it was served as.
pub(crate) fn extension_for(content_type: &str) -> &'static str {
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match essence.as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/heic" => "heic",
        "image/heif" => "heif",
        "application/pdf" => "pdf",
        "text/plain" => "txt",
        "text/csv" => "csv",
        "application/json" => "json",
        "audio/mpeg" => "mp3",
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" => "m4a",
        "video/mp4" => "mp4",
        "video/quicktime" => "mov",
        _ => "bin",
    }
}

/// Seconds from a Retry-After header, capped; the HTTP-date form is rare
/// enough that the caller's own backoff covers it.
pub(crate) fn retry_after(header: Option<&str>) -> Option<Duration> {
    let seconds: u64 = header?.trim().parse().ok()?;
    Some(Duration::from_secs(seconds).min(RETRY_AFTER_CAP))
}

/// A refused Photon call, kept typed so the poll loop can tell a rate limit
/// or a revoked key from a blip.
#[derive(Debug)]
pub(crate) struct ApiError {
    pub op: &'static str,
    pub status: u16,
    pub code: String,
    pub retry_after: Option<Duration>,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "photon {} failed: HTTP {}", self.op, self.status)?;
        if !self.code.is_empty() {
            write!(f, " {}", self.code)?;
        }
        Ok(())
    }
}

impl std::error::Error for ApiError {}

/// The `code` of an RFC 9457 problem body; empty when there is none.
pub(crate) fn problem_code(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|problem| {
            problem
                .get("code")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// Photon 2.0's REST API, by hand on reqwest: eight calls do not justify a
/// generated client of several hundred thousand lines.
pub(super) struct Api {
    http: reqwest::Client,
    base: String,
    key: String,
}

impl Api {
    pub(super) fn new(api_base: &str, project_id: &str, key: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            http,
            base: format!("{}/projects/{project_id}", api_base.trim_end_matches('/')),
            key: key.to_string(),
        })
    }

    /// Send a request, retrying what Photon says is worth retrying. `build`
    /// makes the request afresh each time, so a retry repeats it exactly,
    /// idempotency key included.
    async fn execute(
        &self,
        op: &'static str,
        attempts: usize,
        also_retry: &[u16],
        build: impl Fn() -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let backoff = Duration::from_millis(500 * (1 << attempt.min(5)));
            match build().bearer_auth(&self.key).send().await {
                Ok(response) if response.status().is_success() => return Ok(response),
                Ok(response) => {
                    let status = response.status().as_u16();
                    let wait = retry_after(
                        response
                            .headers()
                            .get(reqwest::header::RETRY_AFTER)
                            .and_then(|value| value.to_str().ok()),
                    );
                    let code = problem_code(&response.text().await.unwrap_or_default());
                    // The status codes Photon's own clients retry.
                    let retryable = matches!(status, 408 | 429 | 502 | 503 | 504)
                        || also_retry.contains(&status);
                    if attempt < attempts && retryable {
                        tokio::time::sleep(wait.unwrap_or(backoff)).await;
                        continue;
                    }
                    return Err(ApiError {
                        op,
                        status,
                        code,
                        retry_after: wait,
                    }
                    .into());
                }
                Err(err) if attempt < attempts && (err.is_timeout() || err.is_connect()) => {
                    tokio::time::sleep(backoff).await;
                }
                Err(err) => return Err(anyhow!(err).context(format!("photon {op}"))),
            }
        }
    }

    pub(super) async fn list_messages(&self, query: &[(&str, &str)]) -> Result<Value> {
        let url = format!("{}/messages", self.base);
        let response = self
            .execute("listMessages", 1, &[], || self.http.get(&url).query(query))
            .await?;
        Ok(response.json().await?)
    }

    pub(super) async fn send(
        &self,
        key: &str,
        space: &str,
        sender: &str,
        parent: Option<&str>,
        content: Value,
    ) -> Result<()> {
        let url = format!("{}/messages", self.base);
        let mut body = json!({ "spaceId": space, "senderId": sender, "content": content });
        if let Some(parent) = parent {
            body["parentMessageId"] = Value::String(parent.to_string());
        }
        self.execute("sendMessage", SEND_ATTEMPTS, &[], || {
            self.http
                .post(&url)
                .header("idempotency-key", key)
                .json(&body)
        })
        .await?;
        Ok(())
    }

    pub(super) async fn typing(&self, space: &str, sender: &str, typing: Typing) -> Result<()> {
        let url = format!("{}/spaces/{space}/typing", self.base);
        let typing = match typing {
            Typing::Started => true,
            Typing::Stopped => false,
        };
        let body = json!({ "senderId": sender, "typing": typing });
        self.execute("setSpaceTyping", 1, &[], || {
            self.http.post(&url).json(&body)
        })
        .await?;
        Ok(())
    }

    /// An attachment's bytes and media type. A fresh one can take a few
    /// seconds to be ready, which Photon reports as a 409 worth retrying.
    pub(super) async fn download(&self, attachment: &str) -> Result<(Vec<u8>, String)> {
        let url = format!("{}/attachments/{attachment}/content", self.base);
        let response = self
            .execute("downloadAttachment", SEND_ATTEMPTS, &[409], || {
                self.http.get(&url)
            })
            .await?;
        if response
            .content_length()
            .is_some_and(|len| len > ATTACHMENT_LIMIT as u64)
        {
            bail!("the file is over {} MB", ATTACHMENT_LIMIT / (1024 * 1024));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let bytes = response.bytes().await?;
        ensure!(
            bytes.len() <= ATTACHMENT_LIMIT,
            "the file is over {} MB",
            ATTACHMENT_LIMIT / (1024 * 1024)
        );
        Ok((bytes.to_vec(), content_type))
    }

    pub(super) async fn upload(
        &self,
        key: &str,
        path: &Path,
        content_type: &str,
    ) -> Result<String> {
        let bytes = tokio::fs::read(path)
            .await
            .with_context(|| format!("reading {}", path.display()))?;
        ensure!(
            bytes.len() <= ATTACHMENT_LIMIT,
            "{} is too large to send",
            path.display()
        );
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("screenshot.png");
        let metadata = json!({
            "filename": filename,
            "contentType": content_type,
            "sizeBytes": bytes.len(),
        });
        let boundary = format!("aster-{}", Ulid::new());
        let body = related_body(&boundary, &metadata, content_type, &bytes);
        let url = format!("{}/attachments", self.base);
        let response = self
            .execute("uploadAttachment", SEND_ATTEMPTS, &[], || {
                self.http
                    .post(&url)
                    .header("idempotency-key", key)
                    .header(
                        reqwest::header::CONTENT_TYPE,
                        format!("multipart/related; boundary={boundary}"),
                    )
                    .body(body.clone())
            })
            .await?;
        let attachment: Value = response.json().await?;
        attachment
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .context("uploadAttachment returned no id")
    }
}
