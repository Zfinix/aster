//! Stdio MCP server exposing Telegram chat affordances as agent tools.
//! Spawned per turn by the bridge with TELEGRAM_* env carrying chat context.

use std::env;

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use ulid::Ulid;

use crate::telegram::{Api, REACTIONS};

const PROTOCOL_VERSION: &str = "2026-07-28";
const LEGACY_PROTOCOL_VERSION: &str = "2025-11-25";
const MAX_SCRATCH_STEM: usize = 48;

/// Serve MCP over stdio until the host closes stdin.
pub async fn run_mcp_telegram() -> Result<()> {
    let token = env::var("ASTER_TELEGRAM_TOKEN").context("ASTER_TELEGRAM_TOKEN is not set")?;
    let chat_id: i64 = env::var("TELEGRAM_CHAT_ID")
        .context("TELEGRAM_CHAT_ID is not set")?
        .parse()
        .context("TELEGRAM_CHAT_ID is not a number")?;
    let message_id: Option<i64> = env::var("TELEGRAM_MESSAGE_ID")
        .ok()
        .and_then(|v| v.parse().ok());
    let api = Api::new(&token)?;

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut out = tokio::io::stdout();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        // Requests carry an id; notifications need no reply.
        let Some(id) = message.get("id").cloned() else {
            continue;
        };
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let response = match method {
            "server/discover" => json!({ "jsonrpc": "2.0", "id": id, "result": {
                "supportedVersions": [PROTOCOL_VERSION],
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "telegram", "version": env!("CARGO_PKG_VERSION") },
            }}),
            "initialize" => json!({ "jsonrpc": "2.0", "id": id, "result": {
                "protocolVersion": LEGACY_PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "telegram", "version": env!("CARGO_PKG_VERSION") },
            }}),
            "tools/list" => json!({ "jsonrpc": "2.0", "id": id, "result": {
                "tools": tool_catalog(),
            }}),
            "tools/call" => {
                let params = message.get("params").cloned().unwrap_or_default();
                match dispatch(&api, chat_id, message_id, &params).await {
                    Ok(text) => json!({ "jsonrpc": "2.0", "id": id, "result": {
                        "content": [{ "type": "text", "text": text }],
                    }}),
                    Err(e) => json!({ "jsonrpc": "2.0", "id": id, "result": {
                        "content": [{ "type": "text", "text": format!("error: {e:#}") }],
                        "isError": true,
                    }}),
                }
            }
            _ => json!({ "jsonrpc": "2.0", "id": id, "error": {
                "code": -32601, "message": format!("unknown method {method}"),
            }}),
        };
        let mut line = response.to_string();
        line.push('\n');
        out.write_all(line.as_bytes()).await?;
        out.flush().await?;
    }
    Ok(())
}

fn tool_catalog() -> Value {
    let url = |desc: &str| json!({ "type": "string", "description": desc });
    let caption =
        json!({ "type": "string", "description": "Optional caption shown under the media" });
    json!([
        {
            "name": "react",
            "description": "React to the user's current message with one emoji. Use sparingly, when it genuinely fits (a win, a thanks, something funny), not on every reply.",
            "inputSchema": { "type": "object", "required": ["emoji"], "properties": {
                "emoji": { "type": "string", "enum": REACTIONS,
                           "description": "The reaction emoji; only these are accepted by Telegram" },
            }},
        },
        {
            "name": "send_gif",
            "description": "Send a gif that plays inline in the chat. Pair with the giphy search tools: pass the gif's media URL here instead of pasting it into your reply.",
            "inputSchema": { "type": "object", "required": ["url"], "properties": {
                "url": url("Direct URL to a .gif or MP4 animation"),
                "caption": caption,
            }},
        },
        {
            "name": "send_photo",
            "description": "Send an image that renders inline in the chat.",
            "inputSchema": { "type": "object", "required": ["url"], "properties": {
                "url": url("Direct URL to a JPEG/PNG image"),
                "caption": caption,
            }},
        },
        {
            "name": "send_document",
            "description": "Send a file from the repository to the chat as a downloadable attachment, e.g. a log, patch, or generated report. Max 50 MB.",
            "inputSchema": { "type": "object", "required": ["path"], "properties": {
                "path": { "type": "string", "description": "File path, relative to the repository root" },
                "caption": caption,
            }},
        },
        {
            "name": "send_code_page",
            "description": "Send code or a long report to the chat as a private attachment (a .txt document), instead of pasting more than ~40 lines into the chat. It stays private to this chat and can be deleted; it is never published anywhere public. The rendered file keeps the title as its filename.",
            "inputSchema": { "type": "object", "required": ["title", "code"], "properties": {
                "title": { "type": "string", "description": "Filename for the attachment, e.g. the file path" },
                "code": { "type": "string", "description": "The code or preformatted text to send" },
                "note": { "type": "string", "description": "Optional one-line caption above the document" },
            }},
        },
        {
            "name": "send_poll",
            "description": "Ask the user with a native Telegram poll. Good for quick preference checks; for decisions that block your work, prefer ask_user so you get the answer this turn.",
            "inputSchema": { "type": "object", "required": ["question", "options"], "properties": {
                "question": { "type": "string" },
                "options": { "type": "array", "items": { "type": "string" },
                             "minItems": 2, "maxItems": 10 },
            }},
        },
    ])
}

async fn dispatch(
    api: &Api,
    chat_id: i64,
    message_id: Option<i64>,
    params: &Value,
) -> Result<String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .context("tools/call without a tool name")?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let text = |key: &str| {
        args.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };

    let result = match name {
        "react" => {
            let emoji = text("emoji").context("emoji is required")?;
            ensure!(
                REACTIONS.contains(&emoji.as_str()),
                "unsupported emoji; allowed: {}",
                REACTIONS.join(" ")
            );
            let message_id = message_id.context("there is no user message to react to")?;
            api.call(
                "setMessageReaction",
                json!({
                    "chat_id": chat_id,
                    "message_id": message_id,
                    "reaction": [{ "type": "emoji", "emoji": emoji }],
                }),
            )
            .await?;
            json!({ "ok": true })
        }
        "send_gif" | "send_photo" => {
            let url = text("url").context("url is required")?;
            ensure!(
                url.starts_with("https://") || url.starts_with("http://"),
                "url must be http(s)"
            );
            let (method, field) = match name {
                "send_gif" => ("sendAnimation", "animation"),
                _ => ("sendPhoto", "photo"),
            };
            let mut payload = json!({ "chat_id": chat_id, field: url });
            if let Some(caption) = text("caption") {
                payload["caption"] = caption.into();
            }
            let sent = api.call(method, payload).await?;
            json!({ "ok": true, "message_id": sent.get("message_id") })
        }
        "send_document" => {
            let path = text("path").context("path is required")?;
            let sent = api
                .send_document_file(chat_id, &path, text("caption").as_deref())
                .await?;
            json!({ "ok": true, "message_id": sent.get("message_id") })
        }
        "send_code_page" => {
            let title = text("title").context("title is required")?;
            let code = text("code").context("code is required")?;
            let path = write_scratch_document(&title, &code).await?;
            let result = api
                .send_document_file(chat_id, &path, text("note").as_deref())
                .await;
            let _ = tokio::fs::remove_file(&path).await;
            let sent = result?;
            json!({ "ok": true, "message_id": sent.get("message_id") })
        }
        "send_poll" => {
            let question = text("question").context("question is required")?;
            let options: Vec<Value> = args
                .get("options")
                .and_then(Value::as_array)
                .context("options is required")?
                .iter()
                .filter_map(Value::as_str)
                .map(|opt| json!({ "text": opt }))
                .collect();
            ensure!(
                (2..=10).contains(&options.len()),
                "a poll needs 2 to 10 options"
            );
            let sent = api
                .call(
                    "sendPoll",
                    json!({ "chat_id": chat_id, "question": question, "options": options }),
                )
                .await?;
            json!({ "ok": true, "message_id": sent.get("message_id") })
        }
        other => anyhow::bail!("unknown tool {other}"),
    };
    Ok(result.to_string())
}

/// Write `code` to a throwaway `.txt` file in the system temp dir and return its
/// path, sanitized from `title` so the chat attachment gets a readable name.
/// The file is deleted right after it uploads, leaving no public trace.
pub(crate) async fn write_scratch_document(title: &str, code: &str) -> Result<String> {
    let mut stem = String::with_capacity(title.len());
    for ch in title.chars().take(MAX_SCRATCH_STEM) {
        if ch.is_ascii_alphanumeric() {
            stem.push(ch.to_ascii_lowercase());
        } else if !stem.ends_with('-') {
            stem.push('-');
        }
    }
    let stem = stem.trim_matches('-');
    let stem = if stem.is_empty() { "document" } else { stem };
    let path = std::env::temp_dir().join(format!("{stem}-{}.txt", Ulid::new()));
    tokio::fs::write(&path, code).await?;
    Ok(path.display().to_string())
}
