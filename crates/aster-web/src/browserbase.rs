//! Browserbase HTTP client for `/v1/fetch` and `/v1/search`. Browser sessions run
//! over CDP/WebSocket, not REST, so interactive automation belongs elsewhere.
//! <https://docs.browserbase.com/reference/api/openapi.v1.yaml>

use std::time::Duration;

use anyhow::{Context, Result, bail};
use aster_mcp::McpTool;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;
use serde_json::json;

use crate::{ExtractedPage, PageMetadata};

const BASE_URL: &str = "https://api.browserbase.com";

#[derive(Debug, Clone)]
pub struct BrowserbaseClient {
    key: String,
    client: reqwest::Client,
}

impl BrowserbaseClient {
    pub fn new(key: String, timeout_ms: u64) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .build()
            .expect("reqwest::Client always builds");
        Self { key, client }
    }

    fn headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        let value = HeaderValue::from_str(&self.key).context("building x-api-key header")?;
        headers.insert("x-api-key", value);
        Ok(headers)
    }

    pub async fn fetch(&self, url: &str) -> Result<ExtractedPage> {
        let body = json!({ "url": url, "format": "markdown" });
        let res = self
            .client
            .post(format!("{BASE_URL}/v1/fetch"))
            .headers(self.headers()?)
            .json(&body)
            .send()
            .await
            .context("sending fetch request")?;

        let status = res.status();
        let text = res.text().await.context("reading fetch response")?;
        if !status.is_success() {
            bail!("Browserbase fetch failed ({status}): {text}");
        }

        // The fetch API returns the raw markdown string.
        Ok(ExtractedPage {
            markdown: text,
            metadata: PageMetadata {
                url: url.to_string(),
                title: None,
                crawl_depth: Some(0),
                status_code: Some(status.as_u16()),
                success: true,
                description: None,
                language: None,
            },
        })
    }

    pub async fn search(&self, query: &str, num_results: u32) -> Result<Vec<ExtractedPage>> {
        let body = json!({ "query": query, "numResults": num_results });
        let res = self
            .client
            .post(format!("{BASE_URL}/v1/search"))
            .headers(self.headers()?)
            .json(&body)
            .send()
            .await
            .context("sending search request")?;

        let status = res.status();
        let text = res.text().await.context("reading search response")?;
        if !status.is_success() {
            bail!("Browserbase search failed ({status}): {text}");
        }

        let parsed: SearchResponse =
            serde_json::from_str(&text).context("parsing search response")?;
        Ok(parsed
            .result
            .into_iter()
            .map(|r| ExtractedPage {
                markdown: r.content.unwrap_or_default(),
                metadata: PageMetadata {
                    url: r.url,
                    title: Some(r.title),
                    crawl_depth: Some(0),
                    status_code: None,
                    success: true,
                    description: Some(r.description),
                    language: None,
                },
            })
            .collect())
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    result: Vec<SearchItem>,
}

#[derive(Debug, Deserialize)]
struct SearchItem {
    url: String,
    title: String,
    description: String,
    #[serde(default)]
    content: Option<String>,
}

pub fn fetch_tool() -> McpTool {
    aster_mcp::McpTool {
        server: "web".into(),
        name: "extract".into(),
        description: "Fetch a web page as Markdown via Browserbase. Handles JavaScript-heavy pages through a proxy network.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL of the page to fetch, including https://"
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
        description: "Search the web via Browserbase and return results with page content. Use for research and fact-checking.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "num_results": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 25,
                    "default": 5,
                    "description": "Number of results"
                }
            },
            "required": ["query"]
        }),
    }
}
