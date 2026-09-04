//! Cloudflare Browser Run client for quick actions. Needs an account id and a token
//! with Browser Rendering permission: `CLOUDFLARE_BR_ACCOUNT_ID` and
//! `CLOUDFLARE_BR_API_TOKEN`. <https://developers.cloudflare.com/browser-run/quick-actions/>

use std::time::Duration;

use anyhow::{Context, Result, bail};
use aster_mcp::McpTool;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{CrawlOptions, CrawlResult, ExtractedPage, PageMetadata};

#[derive(Debug, Clone)]
pub struct CloudflareBrClient {
    account_id: String,
    token: String,
    client: reqwest::Client,
}

impl CloudflareBrClient {
    pub fn new(account_id: String, token: String, timeout_ms: u64) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .build()
            .expect("reqwest::Client always builds");
        Self {
            account_id,
            token,
            client,
        }
    }

    fn base(&self) -> String {
        format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/browser-rendering",
            self.account_id
        )
    }

    fn headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        let value = HeaderValue::from_str(&format!("Bearer {}", self.token))
            .context("building authorization header")?;
        headers.insert(AUTHORIZATION, value);
        Ok(headers)
    }

    pub async fn extract(&self, url: &str) -> Result<ExtractedPage> {
        let body = json!({ "url": url });
        let res = self
            .client
            .post(format!("{}/markdown", self.base()))
            .headers(self.headers()?)
            .json(&body)
            .send()
            .await
            .context("sending markdown request")?;

        let status = res.status();
        let text = res.text().await.context("reading markdown response")?;
        if !status.is_success() {
            bail!("Cloudflare markdown failed ({status}): {text}");
        }

        let parsed: BrResponse<String> =
            serde_json::from_str(&text).context("parsing markdown response")?;
        if let Some(detail) = parsed.error_detail() {
            bail!("Cloudflare markdown failed: {detail}");
        }
        Ok(ExtractedPage {
            markdown: parsed.result.unwrap_or_default(),
            metadata: PageMetadata {
                url: url.to_string(),
                title: None,
                crawl_depth: Some(0),
                status_code: Some(200),
                success: parsed.success,
                description: None,
                language: None,
            },
        })
    }

    pub async fn crawl(&self, url: &str, opts: &CrawlOptions) -> Result<CrawlResult> {
        let body = json!({
            "url": url,
            "depth": opts.max_depth,
            "limit": opts.max_pages,
            "formats": ["markdown"],
        });
        let res = self
            .client
            .post(format!("{}/crawl", self.base()))
            .headers(self.headers()?)
            .json(&body)
            .send()
            .await
            .context("sending crawl request")?;

        let status = res.status();
        let text = res.text().await.context("reading crawl response")?;
        if !status.is_success() {
            bail!("Cloudflare crawl failed ({status}): {text}");
        }

        let start: BrResponse<String> =
            serde_json::from_str(&text).context("parsing crawl start response")?;
        if let Some(detail) = start.error_detail() {
            bail!("Cloudflare crawl failed to start: {detail}");
        }
        let job_id = start
            .result
            .filter(|id| !id.is_empty())
            .context("Cloudflare crawl returned no job id")?;

        for _ in 0..90 {
            tokio::time::sleep(Duration::from_secs(3)).await;
            let res = self
                .client
                .get(format!("{}/crawl/{job_id}", self.base()))
                .headers(self.headers()?)
                .send()
                .await
                .context("polling crawl status")?;

            let text = res.text().await.context("reading crawl status")?;
            let polled: BrResponse<CrawlGetResult> =
                serde_json::from_str(&text).context("parsing crawl poll response")?;

            if let Some(result) = polled.result {
                let state = result.status.unwrap_or_default();
                match state.as_str() {
                    "completed" => {
                        let pages: Vec<ExtractedPage> = result
                            .records
                            .into_iter()
                            .map(|r| ExtractedPage {
                                markdown: r.markdown.unwrap_or_default(),
                                metadata: PageMetadata {
                                    url: r.url.clone(),
                                    title: r.metadata.as_ref().and_then(|m| m.title.clone()),
                                    crawl_depth: None,
                                    status_code: r.metadata.as_ref().and_then(|m| m.status),
                                    success: r.status == "completed",
                                    description: None,
                                    language: None,
                                },
                            })
                            .collect();
                        let total = pages.len() as u32;
                        return Ok(CrawlResult {
                            num_urls: total,
                            num_succeeded: total,
                            num_failed: 0,
                            max_crawl_depth: 0,
                            pages,
                            credits_consumed: None,
                        });
                    }
                    "failed" | "cancelled" | "errored" => {
                        bail!("Cloudflare crawl job {job_id} {state}");
                    }
                    _ => continue,
                }
            }
        }
        bail!("Cloudflare crawl job {job_id} timed out");
    }
}

#[derive(Debug, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
struct BrResponse<T> {
    success: bool,
    #[serde(default)]
    result: Option<T>,
    #[serde(default)]
    errors: Vec<Value>,
}

impl<T> BrResponse<T> {
    fn error_detail(&self) -> Option<String> {
        if self.success {
            return None;
        }
        let detail = self
            .errors
            .iter()
            .map(|e| {
                e.get("message")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| e.to_string())
            })
            .collect::<Vec<_>>()
            .join("; ");
        Some(match detail.is_empty() {
            true => "no detail given".to_string(),
            false => detail,
        })
    }
}

#[derive(Debug, Deserialize)]
struct CrawlGetResult {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    records: Vec<CrawlRecord>,
}

#[derive(Debug, Deserialize)]
struct CrawlRecord {
    url: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    markdown: Option<String>,
    #[serde(default)]
    metadata: Option<RecordMetadata>,
}

#[derive(Debug, Deserialize)]
struct RecordMetadata {
    #[serde(default)]
    status: Option<u16>,
    #[serde(default)]
    title: Option<String>,
}

pub fn extract_tool() -> McpTool {
    aster_mcp::McpTool {
        server: "web".into(),
        name: "extract".into(),
        description: "Extract a web page as Markdown via Cloudflare Browser Run. Uses a real browser for accurate rendering.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL of the page to extract, including https://"
                }
            },
            "required": ["url"]
        }),
    }
}

pub fn crawl_tool() -> McpTool {
    aster_mcp::McpTool {
        server: "web".into(),
        name: "crawl".into(),
        description:
            "Crawl a website via Cloudflare Browser Run and return Markdown for each page.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "Starting URL including https://"
                },
                "max_pages": {
                    "type": "integer",
                    "minimum": 1,
                    "default": 100,
                    "description": "Maximum pages to crawl"
                },
                "max_depth": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Link depth from start URL"
                }
            },
            "required": ["url"]
        }),
    }
}
