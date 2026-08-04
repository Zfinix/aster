//! Stdio MCP server exposing Telegram chat affordances as agent tools.
//! Spawned per turn by the bridge with TELEGRAM_* env carrying chat context.

use std::env;

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::telegram::{Api, REACTIONS};

const PROTOCOL_VERSION: &str = "2026-07-28";
const LEGACY_PROTOCOL_VERSION: &str = "2025-11-25";

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
            "description": "Publish code or a long report as a telegra.ph page and send its link; it opens inside Telegram with proper monospace formatting. Use this instead of pasting more than ~40 lines of code into the chat. Pages are unlisted but publicly reachable, so never publish secrets, keys, or proprietary code the user has not asked to share.",
            "inputSchema": { "type": "object", "required": ["title", "code"], "properties": {
                "title": { "type": "string", "description": "Page title, e.g. the file path" },
                "code": { "type": "string", "description": "The code or preformatted text to publish" },
                "note": { "type": "string", "description": "Optional one-line message sent with the link" },
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
            let url = publish_telegraph_page(&title, &code).await?;
            let message = match text("note") {
                Some(note) => format!("{note}\n{url}"),
                None => url.clone(),
            };
            let sent = api
                .call(
                    "sendMessage",
                    json!({ "chat_id": chat_id, "text": message }),
                )
                .await?;
            json!({ "ok": true, "url": url, "message_id": sent.get("message_id") })
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

/// Publish preformatted text on telegra.ph and return the page URL.
/// Anonymous account per call; pages are unlisted but public.
pub(crate) async fn publish_telegraph_page(title: &str, code: &str) -> Result<String> {
    let http = reqwest::Client::new();
    let account: Value = http
        .post("https://api.telegra.ph/createAccount")
        .json(&json!({ "short_name": "aster" }))
        .send()
        .await?
        .json()
        .await?;
    let token = account
        .get("result")
        .and_then(|r| r.get("access_token"))
        .and_then(Value::as_str)
        .context("telegra.ph did not return an access token")?;

    let content = json!([{ "tag": "pre", "children": [code] }]);
    let page: Value = http
        .post("https://api.telegra.ph/createPage")
        .json(&json!({
            "access_token": token,
            "title": title,
            "content": content.to_string(),
        }))
        .send()
        .await?
        .json()
        .await?;
    if !page.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        let error = page
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        anyhow::bail!("telegra.ph createPage failed: {error}");
    }
    page.get("result")
        .and_then(|r| r.get("url"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .context("telegra.ph returned no page url")
}
