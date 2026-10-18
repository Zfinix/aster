//! Top-level `web_search` and `web_fetch` tools. Small models reach for a tool
//! with that name before they think to look one up through `aster_mcp`, so the
//! built-in web server's search and extract are offered directly as well.

use serde_json::{Value, json};

const SEARCH_TOOL: &str = "web/search";
const FETCH_TOOL: &str = "web/extract";

/// Sent with `aster_mcp` search replies in a session that offers `web_search`.
pub(crate) const SEARCH_NOTE: &str = "These are tools you can call, not answers. Pick one \
     and call aster_mcp with action=execute and its id. To search the internet, call the \
     `web_search` tool instead; to open a URL, call `web_fetch`.";

/// Schemas for the session's tools array, when the web server is connected.
pub(crate) fn schemas(catalog: &aster_mcp::McpCatalog) -> Vec<Value> {
    if catalog.get(SEARCH_TOOL).is_none() {
        return Vec::new();
    }
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "web_search",
                "description": "Search the internet and return each result's title, URL, and text. Use it for anything outside this repository: current versions, docs, errors, how-tos, news.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "What to search for" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 10, "description": "How many results, default 5" }
                    },
                    "required": ["query"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "web_fetch",
                "description": "Open a URL and read the page as Markdown. Use it whenever the user gives a link or asks to open or read a web page; it needs no browser.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": { "type": "string", "description": "Full URL including https://" }
                    },
                    "required": ["url"]
                }
            }
        }),
    ]
}

/// The `aster_mcp` arguments that run a `web_search` or `web_fetch` call, or
/// `None` for any other tool.
pub(crate) fn bridge_arguments(name: &str, arguments: &str) -> Option<String> {
    let tool = match name {
        "web_search" => SEARCH_TOOL,
        "web_fetch" => FETCH_TOOL,
        _ => return None,
    };
    let arguments: Value = serde_json::from_str(arguments).unwrap_or_else(|_| json!({}));
    Some(json!({ "action": "execute", "name": tool, "arguments": arguments }).to_string())
}

#[cfg(test)]
#[path = "tests/web_tools_test.rs"]
mod tests;
