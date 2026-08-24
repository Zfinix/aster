//! A local record of every failed provider request, so a regression like a
//! wire-format change is diagnosable from disk instead of by diffing binaries
//! against a local server. One JSON line per failure at
//! `~/.aster/logs/provider-errors.jsonl`, holding the status, the response
//! body, and a summary of what was sent. Never the API key and never message
//! contents: only shape metadata that is safe to paste into a bug report.

use std::io::Write as _;
use std::path::Path;

use serde_json::{Value, json};

/// The response body is kept for diagnosis but bounded, since some providers
/// echo whole requests back on validation failures.
pub const MAX_BODY_CHARS: usize = 2048;

/// The whole log is bounded. Past this it is truncated to nothing and starts
/// over: recent failures matter, history does not.
const MAX_LOG_BYTES: u64 = 1024 * 1024;

/// Where the log lives under the home directory.
pub const LOG_RELATIVE_PATH: &str = ".aster/logs/provider-errors.jsonl";

/// Append one failure record. Best effort: logging must never turn a provider
/// error into a different error, so every failure here is silent.
pub fn log_request_failure(
    home: &Path,
    model: &str,
    base_url: &str,
    status: u16,
    body: &str,
    request: &Value,
) {
    let entry = json!({
        "ts": humantime_now(),
        "model": model,
        "endpoint_host": host_only(base_url),
        "status": status,
        "response_body": truncate_body(body),
        "request_summary": summarize_request(request),
    });
    let Ok(line) = serde_json::to_string(&entry) else {
        return;
    };
    let Some(path) = log_path(home) else {
        return;
    };
    if let Err(e) = append_bounded(&path, line.as_bytes()) {
        tracing::debug!("could not write provider error log: {e}");
    }
}

/// The fields of a chat request worth recording when it fails: enough to spot
/// a wrong wire format (a bad temperature, an unexpected top-level key) without
/// carrying message contents or credentials.
fn summarize_request(request: &Value) -> Value {
    if !request.is_object() {
        return Value::Null;
    }
    let mut summary = serde_json::Map::new();
    for key in ["model", "temperature", "stream", "reasoning"] {
        if let Some(value) = request.get(key) {
            summary.insert(key.to_string(), value.clone());
        }
    }
    if let Some(messages) = request.get("messages").and_then(Value::as_array) {
        summary.insert("message_count".into(), json!(messages.len()));
    }
    if let Some(tools) = request.get("tools").and_then(Value::as_array) {
        let names: Vec<_> = tools
            .iter()
            .filter_map(|t| t["function"]["name"].as_str())
            .map(str::to_string)
            .collect();
        summary.insert("tool_names".into(), json!(names));
    }
    Value::Object(summary)
}

fn truncate_body(body: &str) -> String {
    match body.char_indices().nth(MAX_BODY_CHARS) {
        Some((idx, _)) => format!("{}…", &body[..idx]),
        None => body.to_string(),
    }
}

fn host_only(base_url: &str) -> String {
    base_url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(base_url)
        .split('/')
        .next()
        .unwrap_or_default()
        .to_string()
}

fn log_path(home: &Path) -> Option<std::path::PathBuf> {
    let path = home.join(LOG_RELATIVE_PATH);
    path.parent()
        .map(std::fs::create_dir_all)
        .transpose()
        .ok()?;
    Some(path)
}

fn append_bounded(path: &Path, line: &[u8]) -> std::io::Result<()> {
    if path.metadata().map(|m| m.len()).unwrap_or(0) + line.len() as u64 > MAX_LOG_BYTES {
        std::fs::File::create(path)?.write_all(b"")?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line)?;
    file.write_all(b"\n")
}

fn humantime_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    format!("{secs}.{millis:03}")
}

#[cfg(test)]
#[path = "error_log_tests.rs"]
mod tests;
