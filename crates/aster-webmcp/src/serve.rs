//! The MCP server side: JSON-RPC 2.0 over line-delimited stdio, mirroring
//! aster-web's server so `aster mcp serve webmcp` lets any MCP client drive a
//! WebMCP page. Unlike the web server, the tool list is live: `tools/list`
//! re-reads the page's registry on every request.

use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

use crate::{WebmcpBackend, WebmcpConfig};

const PROTOCOL_VERSION: &str = "2026-07-28";
const LEGACY_PROTOCOL_VERSION: &str = "2025-11-25";

const METHOD_NOT_FOUND: i64 = -32601;
const INTERNAL_ERROR: i64 = -32603;

/// Serve until stdin closes. The browser is attached lazily on the first
/// request: a client that only probes `server/discover` should not require a
/// running browser to answer.
pub async fn serve() -> Result<()> {
    let backend = Arc::new(Mutex::new(None));
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
        let (backend, out) = (backend.clone(), out.clone());
        inflight.spawn(async move {
            let response = dispatch(&backend, &message, id).await;
            if let Err(e) = write(&out, &response).await {
                tracing::error!("could not write response: {e}");
            }
        });
    }

    inflight.shutdown().await;
    Ok(())
}

type Shared = Mutex<Option<WebmcpBackend>>;

async fn dispatch(backend: &Shared, message: &Value, id: Value) -> Value {
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));

    match method {
        "server/discover" => result(id, discovery()),
        "initialize" => result(
            id,
            json!({
                "protocolVersion": LEGACY_PROTOCOL_VERSION,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "webmcp", "version": env!("CARGO_PKG_VERSION") },
            }),
        ),
        "ping" => result(id, json!({})),
        "tools/list" => match attach(backend).await {
            Ok(backend) => match backend.list_tools().await {
                Ok(tools) => result(id, json!({ "tools": listed(tools) })),
                Err(e) => error(id, INTERNAL_ERROR, format!("{e:#}")),
            },
            Err(e) => error(id, INTERNAL_ERROR, format!("{e:#}")),
        },
        "tools/call" => call(backend, &params, id).await,
        other => error(id, METHOD_NOT_FOUND, format!("unknown method `{other}`")),
    }
}

/// The attached backend, connecting on first use.
async fn attach(shared: &Shared) -> Result<WebmcpBackend> {
    let mut guard = shared.lock().await;
    if let Some(backend) = guard.as_ref() {
        return Ok(backend.clone());
    }
    let backend = WebmcpBackend::connect(&WebmcpConfig::from_env()).await?;
    *guard = Some(backend.clone());
    Ok(backend)
}

fn listed(tools: Vec<aster_mcp::McpTool>) -> Vec<Value> {
    tools
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
        "serverInfo": { "name": "webmcp", "version": env!("CARGO_PKG_VERSION") },
    })
}

/// A tool that fails reports it in the result, not as a JSON-RPC error: the
/// model should see what went wrong and react, the way the page's user would.
async fn call(shared: &Shared, params: &Value, id: Value) -> Value {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return error(id, INTERNAL_ERROR, "`name` is required".into());
    };
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let backend = match attach(shared).await {
        Ok(backend) => backend,
        Err(e) => return error(id, INTERNAL_ERROR, format!("{e:#}")),
    };
    match backend.call(name, &arguments).await {
        Ok(value) => result(id, value),
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
