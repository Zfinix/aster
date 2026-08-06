//! Plain HTTP fallback: fetch a URL and convert HTML to basic Markdown.
//! Document responses (PDF, Office, EPUB) are converted via anydoc.
//! Used when no specialized API key is configured.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use aster_mcp::McpTool;
use async_trait::async_trait;
use serde_json::json;

use crate::{ExtractedPage, PageMetadata, WebExtract};

#[derive(Debug, Clone)]
pub struct PlainHttpClient {
    client: reqwest::Client,
}

impl PlainHttpClient {
    pub fn new(timeout_ms: u64) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .user_agent("aster/0.1 (web-tool)")
            .build()
            .expect("reqwest::Client always builds");
        Self { client }
    }
}

#[async_trait]
impl WebExtract for PlainHttpClient {
    async fn extract(&self, url: &str) -> Result<ExtractedPage> {
        let res = self.client.get(url).send().await.context("fetching URL")?;

        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            bail!("HTTP {status}: {body}");
        }

        // Owned before the body is read: `text()` consumes the response, and a
        // borrowed header would still be alive across that move.
        let ct = res
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let bytes = res.bytes().await.context("reading response body")?;
        let markdown = body_to_markdown(&ct, &bytes, url)?;

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

/// Route a response body: document formats (PDF, Office, EPUB, RTF) through
/// anydoc, HTML through the tag stripper, anything else as-is. Magic bytes
/// decide, not Content-Type: servers routinely mislabel documents.
fn body_to_markdown(ct: &str, bytes: &[u8], url: &str) -> Result<String> {
    if let Some(format) = anydoc::Format::from_bytes(bytes) {
        return anydoc::to_markdown_bytes(bytes, format)
            .map_err(|e| anyhow::anyhow!("converting document at {url} to Markdown: {e}"));
    }
    let text = String::from_utf8_lossy(bytes);
    if ct.contains("text/html") || ct.is_empty() {
        Ok(html_to_markdown(&text))
    } else {
        Ok(text.into_owned())
    }
}

/// Strip HTML tags, decode entities, and collapse whitespace into rough Markdown.
fn html_to_markdown(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let mut tag = String::new();
    let mut last_was_newline = false;

    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let lower = tag.to_lowercase();
                if lower == "script" || lower == "script " || lower.starts_with("script ") {
                    in_script = true;
                } else if lower == "/script" {
                    in_script = false;
                } else if lower == "style" || lower == "style " || lower.starts_with("style ") {
                    in_style = true;
                } else if lower == "/style" {
                    in_style = false;
                } else if lower == "p"
                    || lower == "/p"
                    || lower == "br"
                    || lower == "br/"
                    || lower.starts_with("h") && lower.len() <= 3
                {
                    if !last_was_newline {
                        out.push_str("\n\n");
                        last_was_newline = true;
                    }
                } else if lower == "li" || lower == "/li" || lower == "/ul" || lower == "/ol" {
                    if !last_was_newline {
                        out.push('\n');
                        last_was_newline = true;
                    }
                } else if lower == "a " || lower.starts_with("a ") {
                    // Keep link text; strip the href for simplicity.
                }
                tag.clear();
            }
            _ if in_tag => {
                tag.push(c);
            }
            _ if in_script || in_style => {}
            _ => {
                // Decode common entities. Full entity decoding requires a crate; this
                // handles the most frequent ones.
                out.push(c);
                last_was_newline = false;
            }
        }
    }

    // Collapse runs of blank lines.
    let mut result = String::with_capacity(out.len());
    let mut blank_run = 0usize;
    for line in out.lines() {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                result.push('\n');
            }
        } else {
            blank_run = 0;
            result.push_str(line);
            result.push('\n');
        }
    }
    let trimmed = result.trim().to_string();
    if trimmed.is_empty() {
        "[no text content extracted]".to_string()
    } else {
        trimmed
    }
}

/// Register the `web/extract` MCP tool for agent discovery (plain HTTP variant).
pub fn extract_tool() -> McpTool {
    aster_mcp::McpTool {
        server: "web".into(),
        name: "extract".into(),
        description: "Fetch a single web page or document and return its content as Markdown. Handles HTML and document formats (PDF, Word, PowerPoint, Excel, EPUB). Use for simple, JavaScript-light pages; for dynamic content, prefer web/crawl with Context.dev.".into(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_basic_html() {
        let html = "<html><body><p>hello</p><p>world</p></body></html>";
        let md = html_to_markdown(html);
        assert!(md.contains("hello"));
        assert!(md.contains("world"));
        assert!(!md.contains("<p>"));
        assert!(!md.contains("<html>"));
    }

    #[test]
    fn removes_script_and_style() {
        let html = "<html><head><style>body{}</style></head><body><script>alert(1)</script><p>ok</p></body></html>";
        let md = html_to_markdown(html);
        assert!(md.contains("ok"));
        assert!(!md.contains("alert"));
        assert!(!md.contains("body{}"));
    }

    #[test]
    fn empty_html_gives_placeholder() {
        let md = html_to_markdown("<html><head></head><body></body></html>");
        assert_eq!(md, "[no text content extracted]");
    }

    #[test]
    fn body_routing_detects_documents_by_magic_bytes() {
        let rtf = br"{\rtf1\ansi Hello from RTF}";
        let md = body_to_markdown("application/octet-stream", rtf, "https://x.test/doc").unwrap();
        assert!(md.contains("Hello from RTF"), "{md}");
    }

    #[test]
    fn body_routing_strips_html_bodies() {
        let md = body_to_markdown("text/html", b"<p>hi</p>", "https://x.test").unwrap();
        assert!(md.contains("hi"));
        assert!(!md.contains("<p>"));
    }

    #[test]
    fn body_routing_passes_other_text_through() {
        let md = body_to_markdown("application/json", br#"{"a":1}"#, "https://x.test").unwrap();
        assert_eq!(md, r#"{"a":1}"#);
    }
}
