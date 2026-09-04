//! The MCP server side: JSON-RPC 2.0 over line-delimited stdio. Answers
//! `server/discover` so a modern client skips the handshake, and still accepts
//! `initialize` so the same package works in a pre-2026 client.

use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

use crate::{WebBackend, WebConfig};

const PROTOCOL_VERSION: &str = "2026-07-28";
const LEGACY_PROTOCOL_VERSION: &str = "2025-11-25";

const METHOD_NOT_FOUND: i64 = -32601;
const INTERNAL_ERROR: i64 = -32603;

/// Serve until stdin closes. Requests are handled concurrently, so a 30-second
/// page fetch does not hold up a search behind it.
pub async fn serve() -> Result<()> {
    let tools = Arc::new(WebBackend::from_env(&WebConfig::from_env()));
    let out = Arc::new(Mutex::new(tokio::io::stdout()));
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut inflight = tokio::task::JoinSet::new();

    while let Some(line) = lines.next_line().await.context("reading stdin")? {
        if line.trim().is_empty() {
            continue;
        }
        let message: Value = match serde_json::from_str(&line) {
            Ok(message) => message,
            Err(e) => {
                tracing::warn!("ignoring unparseable message: {e}");
                continue;
            }
        };
        // No id means a notification: acted on, never answered.
        let Some(id) = message.get("id").cloned() else {
            continue;
        };
        let (tools, out) = (tools.clone(), out.clone());
        inflight.spawn(async move {
            let response = dispatch(&tools, &message, id).await;
            if let Err(e) = write(&out, &response).await {
                tracing::error!("could not write response: {e}");
            }
        });
    }

    inflight.shutdown().await;
    Ok(())
}

async fn dispatch(tools: &WebBackend, message: &Value, id: Value) -> Value {
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));

    match method {
        "server/discover" => result(id, discovery()),
        "initialize" => result(
            id,
            json!({
                "protocolVersion": LEGACY_PROTOCOL_VERSION,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "web", "version": env!("CARGO_PKG_VERSION") },
            }),
        ),
        "ping" => result(id, json!({})),
        "tools/list" => result(id, json!({ "tools": listed(tools) })),
        "tools/call" => call(tools, &params, id).await,
        other => error(id, METHOD_NOT_FOUND, format!("unknown method `{other}`")),
    }
}

fn listed(backend: &WebBackend) -> Vec<Value> {
    crate::register_tools(backend)
        .into_iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "inputSchema": tool.input_schema,
            })
        })
        .collect()
}

fn discovery() -> Value {
    json!({
        "supportedVersions": [PROTOCOL_VERSION, LEGACY_PROTOCOL_VERSION],
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "web", "version": env!("CARGO_PKG_VERSION") },
    })
}

async fn call(tools: &WebBackend, params: &Value, id: Value) -> Value {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return error(id, INTERNAL_ERROR, "`name` is required".into());
    };
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    match tools.call(name, &arguments).await {
        Ok(value) => {
            let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
            result(id, json!({ "content": [{ "type": "text", "text": text }] }))
        }
        Err(e) => result(
            id,
            json!({
                "content": [{ "type": "text", "text": format!("{name} failed: {e:#}") }],
                "isError": true,
            }),
        ),
    }
}

fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

async fn write(out: &Mutex<tokio::io::Stdout>, message: &Value) -> Result<()> {
    let mut line = serde_json::to_vec(message).context("serializing response")?;
    line.push(b'\n');
    let mut out = out.lock().await;
    out.write_all(&line).await.context("writing response")?;
    out.flush().await.context("flushing response")
}

#[cfg(test)]
#[path = "tests/serve_test.rs"]
mod tests;
