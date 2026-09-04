#![forbid(unsafe_code)]
//! A WebMCP bridge: tools a page registers through `document.modelContext`
//! become MCP tools. It attaches to a tab in the user's browser over CDP, so
//! calls run in the page with the user's session.

mod cdp;
mod serve;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use aster_mcp::McpTool;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Mutex;

pub use serve::serve;

const SHIM: &str = include_str!("shim.js");

const CALL_TIMEOUT: Duration = Duration::from_secs(30);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

const DEFAULT_CDP_URL: &str = "http://127.0.0.1:9222";

/// The `mcp.webmcp` section of aster.yaml.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WebmcpConfig {
    pub enabled: bool,
    pub cdp_url: String,
}

impl Default for WebmcpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cdp_url: DEFAULT_CDP_URL.to_string(),
        }
    }
}

impl WebmcpConfig {
    /// The stdio server has no aster.yaml of its own, so it reads
    /// `ASTER_WEBMCP_CDP_URL` and is always on.
    pub fn from_env() -> Self {
        Self {
            enabled: true,
            cdp_url: std::env::var("ASTER_WEBMCP_CDP_URL")
                .ok()
                .filter(|url| !url.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_CDP_URL.to_string()),
        }
    }
}

/// A live bridge to one browser tab. Cloneable: the stdio server shares it
/// across concurrent requests.
#[derive(Clone)]
pub struct WebmcpBackend {
    tab: Arc<Mutex<cdp::Tab>>,
}

impl WebmcpBackend {
    /// Attach to a tab and inject the shim, both for future navigations and
    /// the document already loaded.
    pub async fn connect(config: &WebmcpConfig) -> Result<Self> {
        let mut tab = cdp::Tab::connect(&config.cdp_url, STARTUP_TIMEOUT).await?;
        tab.command(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": SHIM }),
            STARTUP_TIMEOUT,
        )
        .await?;
        // The shim evaluates to undefined, so only its exceptions are checked.
        let injected = tab
            .command(
                "Runtime.evaluate",
                json!({ "expression": SHIM, "returnByValue": true }),
                STARTUP_TIMEOUT,
            )
            .await?;
        if let Some(details) = injected.get("exceptionDetails") {
            bail!(
                "the bridge shim was rejected by the page: {}",
                page_error(details)
            );
        }
        tracing::debug!(url = %tab.url(), "webmcp bridge attached");
        Ok(Self {
            tab: Arc::new(Mutex::new(tab)),
        })
    }

    /// The page's registered tools as catalog entries under `webmcp/`.
    pub async fn list_tools(&self) -> Result<Vec<McpTool>> {
        let raw = self
            .evaluate("window.__asterWebmcp.list()", STARTUP_TIMEOUT)
            .await?;
        let listed: Vec<Value> = serde_json::from_str(&raw)
            .with_context(|| format!("the page returned an unreadable tool list: {raw}"))?;
        Ok(listed.iter().filter_map(tool_from).collect())
    }

    /// Invoke one tool's `execute` in the page. The page's return value is
    /// already MCP-shaped (`{ content: [...] }`) per the WebMCP draft.
    pub async fn call(&self, tool: &str, arguments: &Value) -> Result<Value> {
        let expression = format!(
            "window.__asterWebmcp.call({}, {})",
            serde_json::to_string(tool)?,
            serde_json::to_string(&arguments.to_string())?,
        );
        let raw = self.evaluate(&expression, CALL_TIMEOUT).await?;
        serde_json::from_str(&raw)
            .with_context(|| format!("{tool} returned an unreadable result: {raw}"))
    }

    async fn evaluate(&self, expression: &str, budget: Duration) -> Result<String> {
        let mut tab = self.tab.lock().await;
        evaluate_on(&mut tab, expression, budget).await
    }
}

async fn evaluate_on(tab: &mut cdp::Tab, expression: &str, budget: Duration) -> Result<String> {
    let result = tab
        .command(
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "awaitPromise": true,
                "returnByValue": true,
            }),
            budget,
        )
        .await?;
    if let Some(details) = result.get("exceptionDetails") {
        bail!("{}", page_error(details));
    }
    result
        .pointer("/result/value")
        .and_then(Value::as_str)
        .map(str::to_string)
        .context("the page returned no value")
}

fn page_error(details: &Value) -> String {
    let reason = details
        .pointer("/exception/description")
        .or(details.pointer("/exception/value"))
        .and_then(Value::as_str)
        .or_else(|| details.get("text").and_then(Value::as_str))
        .unwrap_or("the page script failed");
    reason.lines().next().unwrap_or(reason).to_string()
}

fn tool_from(entry: &Value) -> Option<McpTool> {
    let name = entry.get("name")?.as_str()?.to_string();
    let description = entry
        .get("description")
        .and_then(Value::as_str)
        .filter(|d| !d.trim().is_empty())
        .unwrap_or("no description provided")
        .to_string();
    let input_schema = match entry.get("inputSchema") {
        Some(schema) if schema.is_object() => schema.clone(),
        _ => json!({ "type": "object" }),
    };
    Some(McpTool {
        server: "webmcp".to_string(),
        name,
        description,
        input_schema,
    })
}

#[cfg(test)]
#[path = "tests/lib_test.rs"]
mod tests;
