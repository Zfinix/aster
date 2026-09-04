//! Tests run against a fake browser: one TCP listener that answers
//! `/json/list` over HTTP and speaks just enough CDP on the WebSocket to stand
//! in for a page with the shim installed.

use std::sync::{Arc, Mutex};

use aster_mcp::McpTool;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

use super::*;

struct State {
    targets: String,
    tools: String,
    call_result: String,
    fail_calls: bool,
}

impl State {
    fn page(tools: Value) -> Self {
        Self {
            targets: String::new(), // filled with the real socket address later
            tools: tools.to_string(),
            call_result: json!({ "content": [{ "type": "text", "text": "done" }] }).to_string(),
            fail_calls: false,
        }
    }

    fn no_tabs() -> Self {
        Self {
            targets: "[]".to_string(),
            tools: "[]".to_string(),
            call_result: "{}".to_string(),
            fail_calls: false,
        }
    }
}

struct FakeBrowser {
    cdp_url: String,
    state: Arc<Mutex<State>>,
}

async fn start_fake_browser(state: State) -> FakeBrowser {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let state = Arc::new(Mutex::new(state));
    {
        let state = state.clone();
        let ws_url = format!("ws://{addr}/devtools/page/1");
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let state = state.clone();
                let ws_url = ws_url.clone();
                tokio::spawn(async move {
                    // Peek rather than read: the WebSocket handshake must stay
                    // in the stream for accept_async to consume.
                    let mut peek = [0u8; 512];
                    let n = stream.peek(&mut peek).await.expect("peek");
                    let head = String::from_utf8_lossy(&peek[..n]).to_string();
                    if head.contains("/json/list") {
                        let mut request = Vec::new();
                        let mut chunk = [0u8; 1024];
                        loop {
                            let n = stream.read(&mut chunk).await.expect("read");
                            request.extend_from_slice(&chunk[..n]);
                            if request.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                        let body = {
                            let mut state = state.lock().expect("state");
                            if state.targets.is_empty() {
                                state.targets = json!([{
                                    "id": "1",
                                    "type": "page",
                                    "url": "https://example.test/",
                                    "webSocketDebuggerUrl": ws_url,
                                }])
                                .to_string();
                            }
                            state.targets.clone()
                        };
                        let response = format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        stream.write_all(response.as_bytes()).await.expect("write");
                        return;
                    }
                    serve_cdp(stream, state).await;
                });
            }
        });
    }
    FakeBrowser {
        cdp_url: format!("http://{addr}"),
        state,
    }
}

async fn serve_cdp(stream: tokio::net::TcpStream, state: Arc<Mutex<State>>) {
    let mut ws = accept_async(stream).await.expect("websocket upgrade");
    while let Some(Ok(Message::Text(text))) = ws.next().await {
        let Ok(message) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let reply = match message.get("method").and_then(Value::as_str) {
            Some("Page.addScriptToEvaluateOnNewDocument") => {
                json!({ "id": id, "result": { "identifier": "1" } })
            }
            Some("Runtime.evaluate") => {
                let expression = message
                    .pointer("/params/expression")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let state = state.lock().expect("state");
                if expression.contains("__asterWebmcp.list") {
                    json!({ "id": id, "result": { "result": { "type": "string", "value": state.tools } } })
                } else if expression.contains("__asterWebmcp.call") {
                    if state.fail_calls {
                        json!({ "id": id, "result": { "exceptionDetails": {
                            "text": "Uncaught",
                            "exception": { "description": "Error: no WebMCP tool named nope on this page\n    at call (shim.js:80)" },
                        } } })
                    } else {
                        json!({ "id": id, "result": { "result": { "type": "string", "value": state.call_result } } })
                    }
                } else {
                    // The shim injection evaluates to undefined.
                    json!({ "id": id, "result": { "result": { "type": "undefined" } } })
                }
            }
            _ => json!({ "id": id, "result": {} }),
        };
        ws.send(Message::Text(reply.to_string().into()))
            .await
            .expect("reply");
    }
}

fn config_for(browser: &FakeBrowser) -> WebmcpConfig {
    WebmcpConfig {
        enabled: true,
        cdp_url: browser.cdp_url.clone(),
    }
}

#[tokio::test]
async fn the_pages_tools_join_the_catalog() {
    let browser = start_fake_browser(State::page(json!([
        {
            "name": "add_todo",
            "description": "Add a todo item",
            "inputSchema": {
                "type": "object",
                "properties": { "title": { "type": "string" } },
                "required": ["title"],
            },
        },
        { "name": "list_todos", "description": "List todos" },
        { "description": "no name, skipped" },
    ])))
    .await;
    let backend = WebmcpBackend::connect(&config_for(&browser))
        .await
        .expect("connect");

    let tools = backend.list_tools().await.expect("list");

    assert_eq!(
        tools,
        vec![
            McpTool {
                server: "webmcp".to_string(),
                name: "add_todo".to_string(),
                description: "Add a todo item".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": { "title": { "type": "string" } },
                    "required": ["title"],
                }),
            },
            McpTool {
                server: "webmcp".to_string(),
                name: "list_todos".to_string(),
                description: "List todos".to_string(),
                input_schema: json!({ "type": "object" }),
            },
        ]
    );
}

#[tokio::test]
async fn a_tool_call_runs_in_the_page_and_returns_its_content() {
    let browser = start_fake_browser(State::page(json!([]))).await;
    let backend = WebmcpBackend::connect(&config_for(&browser))
        .await
        .expect("connect");

    let result = backend
        .call("add_todo", &json!({ "title": "write tests" }))
        .await
        .expect("call");

    assert_eq!(
        result,
        json!({ "content": [{ "type": "text", "text": "done" }] })
    );
}

#[tokio::test]
async fn a_page_exception_names_the_pages_own_error() {
    let browser = start_fake_browser(State::page(json!([]))).await;
    browser.state.lock().expect("state").fail_calls = true;
    let backend = WebmcpBackend::connect(&config_for(&browser))
        .await
        .expect("connect");

    let error = backend
        .call("nope", &json!({}))
        .await
        .expect_err("the page rejected the call");

    assert_eq!(
        error.to_string(),
        "Error: no WebMCP tool named nope on this page"
    );
}

#[tokio::test]
async fn a_browser_with_no_tab_says_so() {
    let browser = start_fake_browser(State::no_tabs()).await;

    let error = match WebmcpBackend::connect(&config_for(&browser)).await {
        Ok(_) => panic!("no tab to attach to"),
        Err(error) => error,
    };

    assert!(
        error.to_string().contains("no open tab"),
        "unexpected error: {error:#}"
    );
}

#[tokio::test]
async fn an_unreachable_browser_says_where_it_looked() {
    let config = WebmcpConfig {
        enabled: true,
        cdp_url: "http://127.0.0.1:1".to_string(),
    };

    let error = match WebmcpBackend::connect(&config).await {
        Ok(_) => panic!("nothing listens on port 1"),
        Err(error) => error,
    };

    assert!(
        format!("{error:#}").contains("http://127.0.0.1:1"),
        "unexpected error: {error:#}"
    );
}
