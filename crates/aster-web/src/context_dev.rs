//! Context.dev HTTP client: crawl, search, sitemap, and screenshot endpoints.
//! See <https://docs.context.dev/api-reference/web-scraping/crawl>.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use aster_mcp::McpTool;
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    CrawlOptions, CrawlResult, ExtractedPage, PageMetadata, Screenshot, SitemapResult, WebCrawl,
    WebExtract,
};

const BASE_URL: &str = "https://api.context.dev/v1";
const MAX_RETRIES: u32 = 3;

#[derive(Debug, Clone)]
pub struct ContextDevClient {
    key: String,
    client: reqwest::Client,
}

impl ContextDevClient {
    pub fn new(key: String, timeout_ms: u64) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .build()
            .expect("reqwest::Client always builds");
        Self { key, client }
    }

    fn headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        let value = HeaderValue::from_str(&format!("Bearer {}", self.key))
            .context("building authorization header")?;
        headers.insert(AUTHORIZATION, value);
        Ok(headers)
    }
}

#[async_trait]
impl WebExtract for ContextDevClient {
    async fn extract(&self, url: &str) -> Result<ExtractedPage> {
        let result = self
            .crawl(
                url,
                &CrawlOptions {
                    max_pages: 1,
                    max_depth: Some(0),
                    ..Default::default()
                },
            )
            .await?;
        result
            .pages
            .into_iter()
            .next()
            .context("no pages returned for single-page extract")
    }
}

#[async_trait]
impl WebCrawl for ContextDevClient {
    async fn crawl(&self, url: &str, opts: &CrawlOptions) -> Result<CrawlResult> {
        let body = CrawlRequest {
            url: url.to_string(),
            max_pages: Some(opts.max_pages),
            max_depth: opts.max_depth,
            stop_after_ms: Some(opts.stop_after_ms),
            follow_subdomains: Some(opts.follow_subdomains),
            pdf: Some(PdfOptions {
                should_parse: opts.parse_pdfs,
                ocr: false,
                start: None,
                end: None,
            }),
            url_regex: opts.url_regex.clone(),
            use_main_content_only: Some(opts.use_main_content_only),
            include_links: Some(opts.include_links),
            include_images: Some(false),
            max_age_ms: Some(0),
        };

        let response = retry(MAX_RETRIES, || async {
            let res = self
                .client
                .post(format!("{BASE_URL}/web/crawl"))
                .headers(self.headers()?)
                .json(&body)
                .send()
                .await
                .context("sending crawl request")?;
            read_body(res, "crawl").await
        })
        .await?;

        let parsed: CrawlResponse =
            serde_json::from_str(&response).context("parsing crawl response")?;
        let pages = parsed
            .results
            .into_iter()
            .map(|r| ExtractedPage {
                markdown: r.markdown,
                metadata: PageMetadata {
                    url: r.metadata.url,
                    title: r.metadata.title,
                    crawl_depth: Some(r.metadata.crawl_depth),
                    status_code: Some(r.metadata.status_code),
                    success: r.metadata.success,
                    description: r.metadata.description,
                    language: r.metadata.language,
                },
            })
            .collect();

        Ok(CrawlResult {
            num_urls: parsed.metadata.num_urls,
            num_succeeded: parsed.metadata.num_succeeded,
            num_failed: parsed.metadata.num_failed,
            max_crawl_depth: parsed.metadata.max_crawl_depth,
            pages,
            credits_consumed: parsed.key_metadata.credits_consumed,
        })
    }
}

impl ContextDevClient {
    /// Search the web; each result is scraped to Markdown in the same call.
    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<ExtractedPage>> {
        let body = SearchRequest {
            query: query.to_string(),
            // The API refuses numResults below 10; `take(limit)` trims after.
            num_results: limit.clamp(10, 100),
            markdown_options: MarkdownOptions { enabled: true },
        };

        let response = retry(MAX_RETRIES, || async {
            let res = self
                .client
                .post(format!("{BASE_URL}/web/search"))
                .headers(self.headers()?)
                .json(&body)
                .send()
                .await
                .context("sending search request")?;
            read_body(res, "search").await
        })
        .await?;

        let parsed: SearchResponse =
            serde_json::from_str(&response).context("parsing search response")?;
        Ok(parsed
            .results
            .into_iter()
            .take(limit as usize)
            .map(|r| {
                let scraped = r.markdown.as_ref().is_some_and(|m| m.code == "SUCCESS");
                ExtractedPage {
                    markdown: r.markdown.and_then(|m| m.markdown).unwrap_or_default(),
                    metadata: PageMetadata {
                        url: r.url,
                        title: r.title,
                        crawl_depth: Some(0),
                        status_code: None,
                        success: scraped,
                        description: r.description,
                        language: None,
                    },
                }
            })
            .collect())
    }

    /// List sitemap URLs for a domain without scraping the pages.
    pub async fn sitemap(
        &self,
        domain: &str,
        max_links: u32,
        url_regex: Option<&str>,
    ) -> Result<SitemapResult> {
        let mut query = vec![
            ("domain", domain.to_string()),
            ("maxLinks", max_links.to_string()),
        ];
        if let Some(regex) = url_regex {
            query.push(("urlRegex", regex.to_string()));
        }

        let response = retry(MAX_RETRIES, || async {
            let res = self
                .client
                .get(format!("{BASE_URL}/web/scrape/sitemap"))
                .headers(self.headers()?)
                .query(&query)
                .send()
                .await
                .context("sending sitemap request")?;
            read_body(res, "sitemap").await
        })
        .await?;

        let parsed: SitemapResponse =
            serde_json::from_str(&response).context("parsing sitemap response")?;
        Ok(SitemapResult {
            domain: parsed.domain,
            urls: parsed.urls,
        })
    }

    /// Capture a rendered-page screenshot; returns a CDN URL for the PNG.
    pub async fn screenshot(&self, url: &str, full_page: bool) -> Result<Screenshot> {
        let query = [
            ("directUrl", url.to_string()),
            ("fullScreenshot", full_page.to_string()),
        ];

        let response = retry(MAX_RETRIES, || async {
            let res = self
                .client
                .get(format!("{BASE_URL}/web/screenshot"))
                .headers(self.headers()?)
                .query(&query)
                .send()
                .await
                .context("sending screenshot request")?;
            read_body(res, "screenshot").await
        })
        .await?;

        let parsed: ScreenshotResponse =
            serde_json::from_str(&response).context("parsing screenshot response")?;
        Ok(Screenshot {
            url: parsed.screenshot,
            screenshot_type: parsed.screenshot_type,
            width: parsed.width,
            height: parsed.height,
        })
    }
}

async fn read_body(res: reqwest::Response, what: &str) -> Result<String> {
    match res.status().as_u16() {
        408 => {
            tracing::debug!("Context.dev cold hit (408) on {what}; retrying");
            bail!("retry: cold hit")
        }
        429 => {
            let after = res
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(2);
            tracing::debug!(after, "Context.dev rate limited (429); waiting");
            tokio::time::sleep(Duration::from_secs(after)).await;
            bail!("retry: rate limited")
        }
        401 => bail!("Context.dev API key is invalid or missing"),
        403 => {
            let body = res.text().await.unwrap_or_default();
            bail!("Context.dev access forbidden: {body}")
        }
        500..=599 => bail!("retry: server error"),
        _ => {}
    }

    let status = res.status();
    let text = res
        .text()
        .await
        .with_context(|| format!("reading {what} response body"))?;
    if !status.is_success() {
        bail!("Context.dev {what} failed ({status}): {text}");
    }
    Ok(text)
}

async fn retry<F, Fut>(max_retries: u32, f: F) -> Result<String>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<String>>,
{
    let mut attempt = 0;
    loop {
        attempt += 1;
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) if attempt < max_retries && e.to_string().contains("retry:") => {
                tracing::debug!(attempt, error = %e, "retrying Context.dev request");
            }
            Err(e) => return Err(e),
        }
    }
}

#[derive(Debug, Serialize)]
struct CrawlRequest {
    url: String,
    #[serde(rename = "maxPages", skip_serializing_if = "Option::is_none")]
    max_pages: Option<u32>,
    #[serde(rename = "maxDepth", skip_serializing_if = "Option::is_none")]
    max_depth: Option<u32>,
    #[serde(rename = "stopAfterMs", skip_serializing_if = "Option::is_none")]
    stop_after_ms: Option<u32>,
    #[serde(rename = "followSubdomains", skip_serializing_if = "Option::is_none")]
    follow_subdomains: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pdf: Option<PdfOptions>,
    #[serde(rename = "urlRegex", skip_serializing_if = "Option::is_none")]
    url_regex: Option<String>,
    #[serde(rename = "useMainContentOnly", skip_serializing_if = "Option::is_none")]
    use_main_content_only: Option<bool>,
    #[serde(rename = "includeLinks", skip_serializing_if = "Option::is_none")]
    include_links: Option<bool>,
    #[serde(rename = "includeImages", skip_serializing_if = "Option::is_none")]
    include_images: Option<bool>,
    #[serde(rename = "maxAgeMs", skip_serializing_if = "Option::is_none")]
    max_age_ms: Option<u32>,
}

#[derive(Debug, Serialize)]
struct PdfOptions {
    #[serde(rename = "shouldParse")]
    should_parse: bool,
    ocr: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    start: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct CrawlResponse {
    results: Vec<PageResult>,
    metadata: CrawlMetadata,
    key_metadata: KeyMetadata,
}

#[derive(Debug, Deserialize)]
struct PageResult {
    markdown: String,
    metadata: PageResultMetadata,
}

#[derive(Debug, Deserialize)]
struct PageResultMetadata {
    url: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(rename = "crawlDepth")]
    crawl_depth: u32,
    #[serde(rename = "statusCode")]
    status_code: u16,
    success: bool,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    language: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CrawlMetadata {
    #[serde(rename = "numUrls")]
    num_urls: u32,
    #[serde(rename = "numSucceeded")]
    num_succeeded: u32,
    #[serde(rename = "numFailed")]
    num_failed: u32,
    #[serde(rename = "maxCrawlDepth")]
    max_crawl_depth: u32,
}

#[derive(Debug, Deserialize)]
struct KeyMetadata {
    #[serde(default)]
    credits_consumed: Option<u32>,
}

#[derive(Debug, Serialize)]
struct SearchRequest {
    query: String,
    #[serde(rename = "numResults")]
    num_results: u32,
    #[serde(rename = "markdownOptions")]
    markdown_options: MarkdownOptions,
}

#[derive(Debug, Serialize)]
struct MarkdownOptions {
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<SearchResultItem>,
}

#[derive(Debug, Deserialize)]
struct SearchResultItem {
    url: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    markdown: Option<SearchMarkdown>,
}

#[derive(Debug, Deserialize)]
struct SearchMarkdown {
    #[serde(default)]
    markdown: Option<String>,
    code: String,
}

#[derive(Debug, Deserialize)]
struct SitemapResponse {
    domain: String,
    #[serde(default)]
    urls: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ScreenshotResponse {
    screenshot: String,
    #[serde(rename = "screenshotType", default)]
    screenshot_type: Option<String>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
}

#[cfg(test)]
#[path = "tests/context_dev_test.rs"]
mod tests;

pub fn extract_tool() -> McpTool {
    aster_mcp::McpTool {
        server: "web".into(),
        name: "extract".into(),
        description: "Extract a single web page as clean Markdown via Context.dev. Handles JavaScript-heavy pages. Use for documentation, blog posts, or any page you need in LLM-ready format.".into(),
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

pub fn search_tool() -> McpTool {
    aster_mcp::McpTool {
        server: "web".into(),
        name: "search".into(),
        description: "Search the web via Context.dev; each result carries title, snippet, and full page content as Markdown. Use for research, fact-checking, or finding documentation.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query; natural language and Google-style operators both work"
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

pub fn sitemap_tool() -> McpTool {
    aster_mcp::McpTool {
        server: "web".into(),
        name: "sitemap".into(),
        description: "List a domain's sitemap URLs via Context.dev without scraping any pages. Fast way to discover what exists on a site before extracting or crawling.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "domain": {
                    "type": "string",
                    "description": "Domain without protocol, e.g. docs.rs"
                },
                "max_links": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 100000,
                    "default": 500,
                    "description": "Maximum URLs to return"
                },
                "url_regex": {
                    "type": "string",
                    "description": "Only return URLs matching this regex"
                }
            },
            "required": ["domain"]
        }),
    }
}

pub fn screenshot_tool() -> McpTool {
    aster_mcp::McpTool {
        server: "web".into(),
        name: "screenshot".into(),
        description: "Capture a PNG screenshot of a rendered page via Context.dev and return its CDN URL. Use full_page to capture the entire scrollable height.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL of the page to capture, including https://"
                },
                "full_page": {
                    "type": "boolean",
                    "default": false,
                    "description": "Capture the full scrollable height instead of one viewport"
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
        description: "Crawl a website starting from a URL and return clean Markdown for every page discovered. Use max_pages, max_depth, and url_regex to scope the crawl.".into(),
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
                    "description": "Link depth from start URL; 0 = only the start page"
                },
                "stop_after_ms": {
                    "type": "integer",
                    "minimum": 10000,
                    "maximum": 110000,
                    "default": 80000,
                    "description": "Soft time budget for the crawl"
                },
                "follow_subdomains": {
                    "type": "boolean",
                    "default": false,
                    "description": "Follow links to subdomains"
                },
                "url_regex": {
                    "type": "string",
                    "description": "Regex filter; only matching URLs are scraped"
                }
            },
            "required": ["url"]
        }),
    }
}
