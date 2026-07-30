//! Jina Reader: converts any URL to clean Markdown. Works without a key
//! (rate-limited) or with `JINA_API_KEY` for higher limits.
//! <https://jina.ai/reader/>

use std::time::Duration;

use anyhow::{Context, Result, bail};
use aster_mcp::McpTool;
use async_trait::async_trait;
use serde_json::json;

use crate::{ExtractedPage, PageMetadata, WebExtract};

#[derive(Debug, Clone)]
pub struct JinaClient {
    client: reqwest::Client,
    api_key: Option<String>,
}

impl JinaClient {
    /// `api_key` is optional — Jina works without one but rate-limits more aggressively.
    pub fn new(api_key: Option<String>, timeout_ms: u64) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .user_agent("aster/0.3 (web-tool)")
            .build()
            .expect("reqwest::Client always builds");
        Self { client, api_key }
    }
}

#[async_trait]
impl WebExtract for JinaClient {
    async fn extract(&self, url: &str) -> Result<ExtractedPage> {
        let jina_url = format!("https://r.jina.ai/{url}");
        let mut req = self
            .client
            .get(&jina_url)
            .header("X-Return-Format", "markdown");

        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }

        let res = req.send().await.context("fetching URL via Jina Reader")?;

        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            bail!("Jina Reader HTTP {status}: {body}");
        }

        let markdown = res.text().await.context("reading Jina response")?;

        Ok(ExtractedPage {
            markdown,
            metadata: PageMetadata {
                url: url.to_string(),
                title: None,
                crawl_depth: Some(0),
                status_code: Some(status.as_u16()),
                success: status.is_success(),
                description: None,
                language: None,
            },
        })
    }
}

pub fn extract_tool() -> McpTool {
    aster_mcp::McpTool {
        server: "web".into(),
        name: "extract".into(),
        description:
            "Fetch a single web page and return its content as clean Markdown via Jina Reader. Handles JavaScript-heavy pages, paywalls, and most sites. Use this as the default web extractor.".into(),
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
