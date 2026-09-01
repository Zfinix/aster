//! Firecrawl HTTP client for `/v2/scrape`, `/v2/crawl`, and `/v2/search`.
//! Works without a key on Firecrawl's keyless tier, which serves scrape and
//! search only. See <https://docs.firecrawl.dev/api-reference>.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use aster_mcp::McpTool;
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use serde_json::json;

use crate::{CrawlOptions, CrawlResult, ExtractedPage, PageMetadata, WebCrawl, WebExtract};

const BASE_URL: &str = "https://api.firecrawl.dev";

#[derive(Debug, Clone)]
pub struct FirecrawlClient {
    key: Option<String>,
    client: reqwest::Client,
}

impl FirecrawlClient {
    /// `key` is optional: without one the keyless tier serves scrape and
    /// search, rate-limited per IP.
    pub fn new(key: Option<String>, timeout_ms: u64) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .build()
            .expect("reqwest::Client always builds");
        Self { key, client }
    }

    pub fn is_keyed(&self) -> bool {
        self.key.is_some()
    }

    fn headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        if let Some(ref key) = self.key {
            let value = HeaderValue::from_str(&format!("Bearer {key}"))
                .context("building authorization header")?;
            headers.insert(AUTHORIZATION, value);
        }
        Ok(headers)
    }
}

#[async_trait]
impl WebExtract for FirecrawlClient {
    async fn extract(&self, url: &str) -> Result<ExtractedPage> {
        let body = json!({ "url": url, "formats": ["markdown"] });
        let res = self
            .client
            .post(format!("{BASE_URL}/v2/scrape"))
            .headers(self.headers()?)
            .json(&body)
            .send()
            .await
            .context("sending scrape request")?;

        let status = res.status();
        let text = res.text().await.context("reading scrape response")?;
        if !status.is_success() {
            bail!("Firecrawl scrape failed ({status}): {text}");
        }

        let parsed: ScrapeResponse =
            serde_json::from_str(&text).context("parsing scrape response")?;
        Ok(ExtractedPage {
            markdown: parsed.data.markdown.unwrap_or_default(),
            metadata: PageMetadata {
                url: parsed.data.metadata.url.unwrap_or_else(|| url.to_string()),
                title: parsed.data.metadata.title,
                crawl_depth: Some(0),
                status_code: parsed.data.metadata.status_code,
                success: parsed.success,
                description: parsed.data.metadata.description,
                language: parsed.data.metadata.language,
            },
        })
    }
}

#[async_trait]
impl WebCrawl for FirecrawlClient {
    async fn crawl(&self, url: &str, opts: &CrawlOptions) -> Result<CrawlResult> {
        if !self.is_keyed() {
            bail!(
                "Firecrawl only crawls with an API key. Set FIRECRAWL_API_KEY to crawl a site, or use extract on single pages."
            );
        }
        let body = json!({
            "url": url,
            "maxPages": opts.max_pages,
            "maxDepth": opts.max_depth,
            "scrapeOptions": { "formats": ["markdown"] },
        });
        let res = self
            .client
            .post(format!("{BASE_URL}/v2/crawl"))
            .headers(self.headers()?)
            .json(&body)
            .send()
            .await
            .context("sending crawl request")?;

        let status = res.status();
        let text = res.text().await.context("reading crawl response")?;
        if !status.is_success() {
            bail!("Firecrawl crawl failed ({status}): {text}");
        }

        let start: CrawlStartResponse =
            serde_json::from_str(&text).context("parsing crawl start response")?;
        let job_id = &start.job_id;

        // Poll for results.
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let res = self
                .client
                .get(format!("{BASE_URL}/v2/crawl/{job_id}"))
                .headers(self.headers()?)
                .send()
                .await
                .context("polling crawl status")?;

            let text = res.text().await.context("reading crawl status")?;
            let polled: CrawlPollResponse =
                serde_json::from_str(&text).context("parsing crawl poll response")?;

            // Bound before the match so a terminal state can be named in the
            // error without borrowing `polled` across the move of its pages.
            let state = polled.status.unwrap_or_default();
            match state.as_str() {
                "completed" => {
                    let pages: Vec<ExtractedPage> = polled
                        .data
                        .into_iter()
                        .flatten()
                        .map(|r| ExtractedPage {
                            markdown: r.markdown.unwrap_or_default(),
                            metadata: PageMetadata {
                                url: r.metadata.url.unwrap_or_default(),
                                title: r.metadata.title,
                                crawl_depth: None,
                                status_code: r.metadata.status_code,
                                success: true,
                                description: r.metadata.description,
                                language: r.metadata.language,
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
                "failed" | "cancelled" => {
                    bail!("Firecrawl crawl job {job_id} {state}");
                }
                _ => continue,
            }
        }
        bail!("Firecrawl crawl job {job_id} timed out after 2 minutes");
    }
}

impl FirecrawlClient {
    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<ExtractedPage>> {
        let body =
            json!({ "query": query, "limit": limit, "scrapeOptions": { "formats": ["markdown"] } });
        let res = self
            .client
            .post(format!("{BASE_URL}/v2/search"))
            .headers(self.headers()?)
            .json(&body)
            .send()
            .await
            .context("sending search request")?;

        let status = res.status();
        let text = res.text().await.context("reading search response")?;
        if !status.is_success() {
            bail!("Firecrawl search failed ({status}): {text}");
        }

        let parsed: SearchResponse =
            serde_json::from_str(&text).context("parsing search response")?;
        Ok(parsed
            .data
            .into_iter()
            .map(|r| ExtractedPage {
                markdown: r.markdown.unwrap_or_default(),
                metadata: PageMetadata {
                    url: r.url.unwrap_or_default(),
                    title: r.title,
                    crawl_depth: Some(0),
                    status_code: None,
                    success: true,
                    description: r.description,
                    language: None,
                },
            })
            .collect())
    }
}

#[derive(Debug, Deserialize)]
struct ScrapeResponse {
    success: bool,
    data: ScrapeData,
}

#[derive(Debug, Deserialize)]
struct ScrapeData {
    #[serde(default)]
    markdown: Option<String>,
    metadata: ScrapeMetadata,
}

#[derive(Debug, Deserialize)]
struct ScrapeMetadata {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(rename = "statusCode", default)]
    status_code: Option<u16>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    language: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CrawlStartResponse {
    #[serde(rename = "jobId", alias = "id")]
    job_id: String,
}

#[derive(Debug, Deserialize)]
struct CrawlPollResponse {
    status: Option<String>,
    #[serde(default)]
    data: Option<Vec<ScrapeData>>,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    data: Vec<SearchResultItem>,
}

#[derive(Debug, Deserialize)]
struct SearchResultItem {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    markdown: Option<String>,
}

pub fn search_tool() -> McpTool {
    aster_mcp::McpTool {
        server: "web".into(),
        name: "search".into(),
        description: "Search the web via Firecrawl and return full page content as Markdown for each result. Use for research, fact-checking, or finding documentation.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 10,
                    "default": 5,
                    "description": "Maximum number of results"
                }
            },
            "required": ["query"]
        }),
    }
}

pub fn scrape_tool() -> McpTool {
    aster_mcp::McpTool {
        server: "web".into(),
        name: "extract".into(),
        description: "Extract a single web page as clean Markdown via Firecrawl. Handles JavaScript-heavy pages.".into(),
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
        description: "Crawl a website and return Markdown for every page via Firecrawl. Use max_pages and max_depth to scope the crawl.".into(),
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
                    "maximum": 500,
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
