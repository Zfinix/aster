use super::*;

const SAMPLE_YAML: &str = "\
mcp:
  servers:
    # Ships with Chrome.
    chrome:
      command: npx
      disabled: true
    github:
      command: npx
";

#[test]
fn enabling_rewrites_the_disabled_line_and_keeps_comments() {
    let out = toggled(SAMPLE_YAML, "chrome", false).expect("chrome is configured");
    assert!(out.contains("      disabled: false"));
    assert!(out.contains("# Ships with Chrome."));
}

#[test]
fn disabling_a_server_without_the_key_inserts_one_in_its_block() {
    let out = toggled(SAMPLE_YAML, "github", true).expect("github is configured");
    assert!(
        out.ends_with("    github:\n      command: npx\n      disabled: true\n"),
        "{out}"
    );
}

#[test]
fn an_unknown_server_is_not_a_silent_rewrite() {
    assert!(toggled(SAMPLE_YAML, "slack", true).is_none());
}

#[test]
fn removing_a_server_deletes_its_whole_block_and_nothing_else() {
    let out = without_server(SAMPLE_YAML, "chrome").expect("chrome is configured");
    assert!(!out.contains("chrome:"), "{out}");
    assert!(!out.contains("disabled: true"), "{out}");
    assert!(out.contains("github:\n      command: npx\n"), "{out}");
    assert!(without_server(SAMPLE_YAML, "slack").is_none());

    let out = without_server(SAMPLE_YAML, "github").expect("github is configured");
    assert!(!out.contains("github:"), "{out}");
    assert!(out.contains("chrome:"), "{out}");
}

#[test]
fn text_parts_are_joined_and_typed_parts_are_named() {
    let result = json!({
        "content": [
            { "type": "text", "text": "first" },
            { "type": "image", "data": "..." },
            { "type": "text", "text": "second" }
        ]
    });
    assert_eq!(
        render_result(&result),
        "first\n[image content omitted]\nsecond"
    );
}

#[test]
fn an_error_result_says_so_rather_than_reading_as_success() {
    let result =
        json!({ "isError": true, "content": [{ "type": "text", "text": "no such repo" }] });
    assert!(render_result(&result).starts_with("the MCP tool reported an error"));
}

#[test]
fn an_unfamiliar_shape_falls_back_to_the_raw_payload() {
    let result = json!({ "value": 42 });
    assert_eq!(render_result(&result), result.to_string());
}

/// A whole MCP server in one argument, so the transport test needs no
/// fixture file on disk.
const FAKE_SERVER: &str = r#"
import json, sys
TOOLS = [{"name": "create_issue", "description": "Create a GitHub issue",
          "inputSchema": {"type": "object", "properties": {"repo": {"type": "string"}}}},
         {"name": "send_message", "description": "Send a Slack message",
          "inputSchema": {"type": "object"}}]
def reply(i, r): sys.stdout.write(json.dumps({"jsonrpc":"2.0","id":i,"result":r})+"\n"); sys.stdout.flush()
for line in sys.stdin:
    if not line.strip(): continue
    m = json.loads(line)
    if m.get("method") == "initialize": reply(m["id"], {"protocolVersion": "2025-06-18"})
    elif m.get("method") == "tools/list": reply(m["id"], {"tools": TOOLS})
    elif m.get("method") == "tools/call":
        p = m.get("params", {})
        reply(m["id"], {"content": [{"type": "text", "text": "ran " + p.get("name", "")}]})
"#;

/// A 2026-07-28 server: no `initialize`, answers `server/discover`, and
/// rejects any request that arrives without the required `_meta` fields.
const MODERN_SERVER: &str = r#"
import json, sys
TOOLS = [{"name": "create_issue", "description": "Create a GitHub issue",
          "inputSchema": {"type": "object", "properties": {"repo": {"type": "string"}}}},
         {"name": "send_message", "description": "Send a Slack message",
          "inputSchema": {"type": "object"}}]
def reply(i, r): sys.stdout.write(json.dumps({"jsonrpc":"2.0","id":i,"result":r})+"\n"); sys.stdout.flush()
def err(i, c, m, d=None):
    e = {"code": c, "message": m}
    if d is not None: e["data"] = d
    sys.stdout.write(json.dumps({"jsonrpc":"2.0","id":i,"error":e})+"\n"); sys.stdout.flush()
for line in sys.stdin:
    if not line.strip(): continue
    m = json.loads(line)
    meta = (m.get("params") or {}).get("_meta") or {}
    if m.get("method") == "initialize":
        err(m["id"], -32601, "no such method"); continue
    if not meta.get("io.modelcontextprotocol/protocolVersion"):
        err(m["id"], -32602, "missing protocolVersion"); continue
    if "io.modelcontextprotocol/clientCapabilities" not in meta:
        err(m["id"], -32602, "missing clientCapabilities"); continue
    if m.get("method") == "server/discover":
        reply(m["id"], {"resultType": "complete", "supportedVersions": ["2026-07-28"]})
    elif m.get("method") == "tools/list":
        reply(m["id"], {"resultType": "complete", "tools": TOOLS})
    elif m.get("method") == "tools/call":
        p = m.get("params", {})
        reply(m["id"], {"resultType": "complete",
                        "content": [{"type": "text", "text": "ran " + p.get("name", "")}]})
"#;

/// Modern, but speaks only a revision Aster does not prefer: it must answer
/// with -32022 rather than push the client back to `initialize`.
const OLDER_MODERN_SERVER: &str = r#"
import json, sys
def send(o): sys.stdout.write(json.dumps(o)+"\n"); sys.stdout.flush()
for line in sys.stdin:
    if not line.strip(): continue
    m = json.loads(line)
    v = ((m.get("params") or {}).get("_meta") or {}).get("io.modelcontextprotocol/protocolVersion")
    if v != "2025-11-25":
        send({"jsonrpc":"2.0","id":m["id"],"error":{"code":-32022,"message":"Unsupported protocol version",
              "data":{"supported":["2025-11-25"],"requested":v}}})
        continue
    if m.get("method") == "server/discover":
        send({"jsonrpc":"2.0","id":m["id"],"result":{"supportedVersions":["2025-11-25"]}})
    elif m.get("method") == "tools/list":
        send({"jsonrpc":"2.0","id":m["id"],"result":{"tools":[
            {"name":"ping","description":"Ping the server","inputSchema":{"type":"object"}}]}})
"#;

fn settings_for(source: &str) -> McpSettings {
    let mut settings = McpSettings::default();
    settings.servers.insert(
        "fake".into(),
        ServerConfig {
            command: "python3".into(),
            args: vec!["-c".into(), source.into()],
            ..ServerConfig::default()
        },
    );
    settings
}

fn python_settings() -> McpSettings {
    settings_for(FAKE_SERVER)
}

/// Every server carries whatever web tools the environment's provider keys
/// enable, on top of its own. Counting them here keeps the assertions honest
/// regardless of which `WEB_*`/`*_API_KEY` vars happen to be set in a given
/// environment, rather than mutating process env from a test.
fn web_tool_count() -> usize {
    let config = aster_web::WebConfig::from_env();
    let backend = aster_web::WebBackend::from_env(&config);
    aster_web::register_tools(&backend).len()
}

fn has_python() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[tokio::test]
async fn a_server_handshakes_lists_and_answers_a_call() {
    if !has_python() {
        return;
    }
    let (runtime, problems) = McpRuntime::connect(&python_settings()).await;
    assert!(problems.is_empty(), "{problems:?}");
    let runtime = runtime.expect("a runtime");
    assert_eq!(runtime.tool_count(), 2 + web_tool_count());
    let mut server_names = runtime.server_names();
    server_names.retain(|name| name != "web");
    assert_eq!(server_names, vec!["fake".to_string()]);

    let tool = runtime
        .injector()
        .catalog()
        .get("fake/create_issue")
        .expect("the listed tool")
        .clone();
    let result = runtime
        .call(&tool, &json!({ "repo": "aster" }))
        .await
        .unwrap();
    assert_eq!(render_result(&result), "ran create_issue");
    runtime.shutdown().await;
}

#[tokio::test]
async fn a_modern_server_is_driven_without_an_initialize_handshake() {
    if !has_python() {
        return;
    }
    let (runtime, problems) = McpRuntime::connect(&settings_for(MODERN_SERVER)).await;
    assert!(problems.is_empty(), "{problems:?}");
    let runtime = runtime.expect("a runtime");
    assert_eq!(runtime.tool_count(), 2 + web_tool_count());

    let tool = runtime
        .injector()
        .catalog()
        .get("fake/create_issue")
        .expect("the listed tool")
        .clone();
    let result = runtime
        .call(&tool, &json!({ "repo": "aster" }))
        .await
        .unwrap();
    assert_eq!(render_result(&result), "ran create_issue");
    runtime.shutdown().await;
}

#[tokio::test]
async fn an_unsupported_version_retries_modern_instead_of_falling_back() {
    if !has_python() {
        return;
    }
    let (runtime, problems) = McpRuntime::connect(&settings_for(OLDER_MODERN_SERVER)).await;
    assert!(problems.is_empty(), "{problems:?}");
    // The server only answers when `_meta` carries 2025-11-25, so listing
    // succeeding proves the client switched versions and stayed modern.
    assert_eq!(
        runtime.expect("a runtime").tool_count(),
        1 + web_tool_count()
    );
}

#[test]
fn version_choice_prefers_the_newest_shared_revision() {
    assert_eq!(
        pick_version(Some(&json!(["2025-11-25", "2026-07-28"]))).as_deref(),
        Some("2026-07-28")
    );
    assert_eq!(
        pick_version(Some(&json!(["2025-11-25"]))).as_deref(),
        Some("2025-11-25")
    );
    assert_eq!(pick_version(Some(&json!(["1900-01-01"]))), None);
    assert_eq!(pick_version(None), None);
}

/// Advertises three tools one page at a time, so a client that reads only
/// the first page sees one of them.
const PAGINATED_SERVER: &str = r#"
import json, sys
PAGES = {None: ([{"name":"one","description":"First","inputSchema":{"type":"object"}}], "c1"),
         "c1": ([{"name":"two","description":"Second","inputSchema":{"type":"object"}}], "c2"),
         "c2": ([{"name":"three","description":"Third","inputSchema":{"type":"object"}}], None)}
def send(o): sys.stdout.write(json.dumps(o)+"\n"); sys.stdout.flush()
for line in sys.stdin:
    if not line.strip(): continue
    m = json.loads(line)
    if m.get("method") == "server/discover":
        send({"jsonrpc":"2.0","id":m["id"],"result":{"resultType":"complete",
              "supportedVersions":["2026-07-28"],"capabilities":{"tools":{}}}})
    elif m.get("method") == "tools/list":
        tools, nxt = PAGES[(m.get("params") or {}).get("cursor")]
        r = {"resultType":"complete","tools":tools}
        if nxt: r["nextCursor"] = nxt
        send({"jsonrpc":"2.0","id":m["id"],"result":r})
"#;

/// Modern, but serves resources only: it never declares a tools capability.
const NO_TOOLS_SERVER: &str = r#"
import json, sys
def send(o): sys.stdout.write(json.dumps(o)+"\n"); sys.stdout.flush()
for line in sys.stdin:
    if not line.strip(): continue
    m = json.loads(line)
    if m.get("method") == "server/discover":
        send({"jsonrpc":"2.0","id":m["id"],"result":{"resultType":"complete",
              "supportedVersions":["2026-07-28"],"capabilities":{"resources":{}}}})
    else:
        send({"jsonrpc":"2.0","id":m["id"],"error":{"code":-32601,"message":"no tools here"}})
"#;

#[tokio::test]
async fn every_page_of_a_paginated_tool_list_is_read() {
    if !has_python() {
        return;
    }
    let (runtime, problems) = McpRuntime::connect(&settings_for(PAGINATED_SERVER)).await;
    assert!(problems.is_empty(), "{problems:?}");
    let runtime = runtime.expect("a runtime");
    assert_eq!(
        runtime.tool_count(),
        3 + web_tool_count(),
        "pagination stopped early"
    );
    assert!(runtime.injector().catalog().get("fake/three").is_some());
}

#[tokio::test]
async fn a_server_without_a_tools_capability_is_quiet_not_an_error() {
    if !has_python() {
        return;
    }
    let (runtime, problems) = McpRuntime::connect(&settings_for(NO_TOOLS_SERVER)).await;
    // Web tools always register, so a resources-only server still leaves a
    // runtime — it just contributes nothing under `fake/`.
    let runtime = runtime.expect("web tools keep the runtime alive");
    assert!(runtime.injector().catalog().get("fake/ping").is_none());
    assert!(
        problems.is_empty(),
        "a resources-only server is not a fault: {problems:?}"
    );
}

#[test]
fn structured_content_is_used_when_the_server_sends_no_text() {
    let result = json!({ "structuredContent": { "temperature": 22.5 } });
    assert!(render_result(&result).contains("22.5"));
}

#[test]
fn an_incomplete_modern_result_is_not_reported_as_success() {
    let result = json!({ "resultType": "input_required", "content": [] });
    assert!(render_result(&result).contains("unfinished"));
}

#[tokio::test]
async fn discovery_costs_one_tool_no_matter_how_many_servers_there_are() {
    if !has_python() {
        return;
    }
    let (runtime, _) = McpRuntime::connect(&python_settings()).await;
    let injection = runtime.expect("a runtime").injection().expect("injection");
    assert_eq!(injection.bridge_tool["function"]["name"], "aster_mcp");
    // Schemas stay behind `describe`; the prompt never carries them.
    assert!(!injection.prompt.contains("inputSchema"));
    assert!(!injection.prompt.contains("properties"));
}

#[tokio::test]
async fn a_server_that_cannot_start_is_reported_and_skipped() {
    let mut settings = McpSettings::default();
    settings.servers.insert(
        "broken".into(),
        ServerConfig {
            command: "aster-no-such-binary".into(),
            ..ServerConfig::default()
        },
    );
    let (runtime, problems) = McpRuntime::connect(&settings).await;
    // Web tools always register, so the runtime survives; the broken server
    // is reported as a problem and contributes nothing under `broken/`.
    let runtime = runtime.expect("web tools keep the runtime alive");
    assert!(!runtime.server_names().iter().any(|name| name == "broken"));
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(
        problems[0], "broken is not installed (no `aster-no-such-binary` on PATH)",
        "{problems:?}"
    );
}

/// Dies the way a server missing credentials dies: a complaint on stderr,
/// then a non-zero exit before answering anything.
const AUTH_FAILING_SERVER: &str = r#"
import sys
sys.stderr.write("Error: LINKEDIN_ACCESS_TOKEN environment variable is required\n")
sys.exit(1)
"#;

/// An OAuth-style server: prints the auth route the user must open, then dies.
const AUTH_URL_SERVER: &str = r#"
import sys
sys.stderr.write("Not authenticated. Please authorize at https://linkedin-mcp.dev/auth and retry.\n")
sys.exit(1)
"#;

const CRASHING_SERVER: &str = r#"
import sys
sys.stderr.write("Traceback (most recent call last):\n")
sys.stderr.write("ValueError: config file is corrupt\n")
sys.exit(1)
"#;

#[tokio::test]
async fn a_server_dying_for_credentials_is_reported_as_needing_auth_not_offline() {
    if !has_python() {
        return;
    }
    let (runtime, problems) = McpRuntime::connect(&settings_for(AUTH_FAILING_SERVER)).await;
    assert!(runtime.is_some(), "web tools keep the runtime alive");
    assert_eq!(problems.len(), 1, "{problems:?}");
    // One line, the `Error:` prefix stripped, nothing about crashes.
    assert_eq!(
        problems[0], "fake needs auth: LINKEDIN_ACCESS_TOKEN environment variable is required",
        "{problems:?}"
    );
}

#[tokio::test]
async fn a_server_printing_its_auth_route_gets_that_url_into_the_one_liner() {
    if !has_python() {
        return;
    }
    let (runtime, problems) = McpRuntime::connect(&settings_for(AUTH_URL_SERVER)).await;
    assert!(runtime.is_some(), "web tools keep the runtime alive");
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(
        problems[0], "fake needs auth: sign in at https://linkedin-mcp.dev/auth",
        "{problems:?}"
    );
}

#[test]
fn the_auth_url_is_lifted_out_of_prose_without_trailing_punctuation() {
    let tail: VecDeque<String> = [
        "Not authenticated.",
        "Please visit https://mcp.dev/oauth/start, then retry.",
    ]
    .map(String::from)
    .into();
    assert_eq!(auth_url(&tail).unwrap(), "https://mcp.dev/oauth/start");
    assert_eq!(auth_url(&VecDeque::new()), None);
}

#[tokio::test]
async fn a_server_crashing_at_startup_is_reported_offline_with_its_stderr() {
    if !has_python() {
        return;
    }
    let (runtime, problems) = McpRuntime::connect(&settings_for(CRASHING_SERVER)).await;
    assert!(runtime.is_some(), "web tools keep the runtime alive");
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(
        problems[0], "fake crashed: config file is corrupt",
        "{problems:?}"
    );
}

#[test]
fn a_node_crash_dump_line_is_cut_down_to_the_reason() {
    let line = "Error [ERR_MODULE_NOT_FOUND]: Cannot find package '@modelcontextprotocol/sdk' \
                imported from /Users/chizi/projects/work-projects/mcp/linkedin-mcp/build/server.js";
    assert_eq!(
        brief(line),
        "Cannot find package '@modelcontextprotocol/sdk'"
    );
    let long = format!("Error: {}", "x".repeat(200));
    assert!(brief(&long).chars().count() <= 81);
    assert!(brief(&long).ends_with('…'));
    assert_eq!(brief("plain reason"), "plain reason");
}

/// Exits over the probe the way railway's CLI server does: anything before
/// `initialize` is fatal. Only a probe-free respawn can talk to it.
const STRICT_LEGACY_SERVER: &str = r#"
import json, sys
def reply(i, r): sys.stdout.write(json.dumps({"jsonrpc":"2.0","id":i,"result":r})+"\n"); sys.stdout.flush()
first = True
for line in sys.stdin:
    if not line.strip(): continue
    m = json.loads(line)
    if first and m.get("method") != "initialize":
        sys.stderr.write("expect initialized request\n")
        sys.exit(1)
    first = False
    if m.get("method") == "initialize": reply(m["id"], {"protocolVersion": "2025-06-18"})
    elif m.get("method") == "tools/list": reply(m["id"], {"tools": [
        {"name": "ping", "description": "Ping the server", "inputSchema": {"type": "object"}}]})
"#;

#[tokio::test]
async fn a_strict_legacy_server_killed_by_the_probe_is_respawned_and_works() {
    if !has_python() {
        return;
    }
    let (runtime, problems) = McpRuntime::connect(&settings_for(STRICT_LEGACY_SERVER)).await;
    assert!(problems.is_empty(), "{problems:?}");
    let runtime = runtime.expect("a runtime");
    assert_eq!(runtime.tool_count(), 1 + web_tool_count());
    runtime.shutdown().await;
}

#[test]
fn auth_smells_are_told_apart_from_ordinary_crashes() {
    assert!(looks_like_auth_failure("Error: 401 Unauthorized"));
    assert!(looks_like_auth_failure("missing API key"));
    assert!(looks_like_auth_failure("please run `railway login` first"));
    assert!(!looks_like_auth_failure(
        "SyntaxError: unexpected token '}'"
    ));
    assert!(!looks_like_auth_failure("ECONNREFUSED 127.0.0.1:8080"));
}

#[test]
fn the_stderr_headline_is_the_error_line_not_the_last_stack_frame() {
    let node_style: VecDeque<String> = [
        "Error: LINKEDIN_ACCESS_TOKEN is required",
        "    at Object.<anonymous> (/app/index.js:3:9)",
        "    at Module._compile (node:internal/modules/cjs/loader)",
    ]
    .map(String::from)
    .into();
    assert_eq!(
        stderr_headline(&node_style).unwrap(),
        "Error: LINKEDIN_ACCESS_TOKEN is required"
    );

    let python_style: VecDeque<String> = [
        "Traceback (most recent call last):",
        "ValueError: config file is corrupt",
    ]
    .map(String::from)
    .into();
    assert_eq!(
        stderr_headline(&python_style).unwrap(),
        "ValueError: config file is corrupt"
    );
    assert_eq!(stderr_headline(&VecDeque::new()), None);
}

#[test]
fn a_tight_budget_hides_tool_names_so_the_agent_has_to_search() {
    let settings = McpSettings {
        context_tokens: 1_000,
        inventory_percent: 0.1,
        ..McpSettings::default()
    };
    let config = settings.progressive();
    assert_eq!(config.available_context_tokens, 1_000);
    assert!(config.inventory_threshold_percent < 1.0);
}

#[test]
fn a_tool_without_a_description_still_enters_the_catalog() {
    let connection_name = "github";
    let entry = json!({ "name": "create_issue" });
    // `McpCatalog::new` rejects empty descriptions, so the fallback text is
    // what keeps a terse server usable.
    let tool = McpTool {
        server: connection_name.into(),
        name: entry["name"].as_str().unwrap().into(),
        description: "no description provided".into(),
        input_schema: json!({ "type": "object" }),
    };
    assert!(McpCatalog::new(vec![tool]).is_ok());
}

const ACCEPTED: &str = "HTTP/1.1 202 Accepted\r\nconnection: close\r\ncontent-length: 0\r\n\r\n";
const CLOSED_OK: &str = "HTTP/1.1 200 OK\r\nconnection: close\r\ncontent-length: 0\r\n\r\n";

/// Just enough of an MCP server to handshake, list, and answer one call.
/// `None` for a notification, which gets no reply.
fn http_reply(message: &Value) -> Option<Value> {
    let id = message.get("id")?.clone();
    let result = match message.get("method").and_then(Value::as_str)? {
        "server/discover" => json!({
            "resultType": "complete",
            "supportedVersions": ["2026-07-28"],
            "capabilities": { "tools": {} },
        }),
        "tools/list" => json!({
            "resultType": "complete",
            "tools": [{
                "name": "create_issue",
                "description": "Create an issue",
                "inputSchema": { "type": "object" },
            }],
        }),
        "tools/call" => json!({
            "resultType": "complete",
            "content": [{ "type": "text", "text": "ran over http" }],
        }),
        _ => json!({ "resultType": "complete" }),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

/// Read one HTTP request off the socket, returning its head and its body.
async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<(String, String)> {
    use tokio::io::AsyncReadExt;
    let mut raw = Vec::new();
    let mut buffer = [0u8; 1024];
    loop {
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        raw.extend_from_slice(&buffer[..read]);
        let text = String::from_utf8_lossy(&raw).to_string();
        let Some(end) = text.find("\r\n\r\n") else {
            continue;
        };
        let head = text[..end].to_string();
        let length: usize = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().ok())?
            })
            .unwrap_or(0);
        let body_start = end + 4;
        if text.len() - body_start >= length {
            return Some((head, text[body_start..body_start + length].to_string()));
        }
    }
}

/// A Streamable HTTP server on loopback. `as_events` frames each reply as an
/// SSE stream instead of a JSON body; both are legal answers to a POST.
/// Returns its url and the request heads it saw.
async fn serve_streamable(as_events: bool) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
    use tokio::io::AsyncWriteExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let heads = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = Arc::clone(&heads);
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let seen = Arc::clone(&seen);
            // One task per connection: the client keeps several open.
            tokio::spawn(async move {
                let Some((head, body)) = read_request(&mut socket).await else {
                    return;
                };
                seen.lock().unwrap().push(head.clone());
                if head.starts_with("DELETE") {
                    let _ = socket.write_all(CLOSED_OK.as_bytes()).await;
                    return;
                }
                let message: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let Some(reply) = http_reply(&message) else {
                    let _ = socket.write_all(ACCEPTED.as_bytes()).await;
                    return;
                };
                let (kind, payload) = match as_events {
                    true => (
                        "text/event-stream",
                        format!("event: message\r\ndata: {reply}\r\n\r\n"),
                    ),
                    false => ("application/json", reply.to_string()),
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nconnection: close\r\ncontent-type: {kind}\r\nmcp-session-id: s-1\r\ncontent-length: {}\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.flush().await;
            });
        }
    });
    (url, heads)
}

/// A server speaking the deprecated HTTP+SSE binding: one GET stream carries
/// every reply, and POSTed messages are acknowledged with 202.
async fn serve_sse() -> String {
    use tokio::io::AsyncWriteExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let url = format!("http://{address}/sse");
    let (replies, outgoing) = tokio::sync::mpsc::unbounded_channel::<String>();
    let outgoing = Arc::new(tokio::sync::Mutex::new(Some(outgoing)));
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let replies = replies.clone();
            let outgoing = Arc::clone(&outgoing);
            // One task per connection, so holding the event stream open does
            // not stop the listener from accepting the POSTs it answers.
            tokio::spawn(async move {
                let Some((head, body)) = read_request(&mut socket).await else {
                    return;
                };
                if head.starts_with("GET") {
                    let opening = format!(
                        "HTTP/1.1 200 OK\r\nconnection: close\r\ncontent-type: text/event-stream\r\n\r\nevent: endpoint\r\ndata: http://{address}/messages\r\n\r\n"
                    );
                    let _ = socket.write_all(opening.as_bytes()).await;
                    let _ = socket.flush().await;
                    let Some(mut outgoing) = outgoing.lock().await.take() else {
                        return;
                    };
                    while let Some(reply) = outgoing.recv().await {
                        let frame = format!("event: message\r\ndata: {reply}\r\n\r\n");
                        if socket.write_all(frame.as_bytes()).await.is_err() {
                            return;
                        }
                        let _ = socket.flush().await;
                    }
                    return;
                }
                let message: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                if let Some(reply) = http_reply(&message) {
                    let _ = replies.send(reply.to_string());
                }
                let _ = socket.write_all(ACCEPTED.as_bytes()).await;
            });
        }
    });
    url
}

fn remote_settings(url: &str, kind: Transport) -> McpSettings {
    let mut settings = McpSettings::default();
    settings.servers.insert(
        "remote".into(),
        ServerConfig {
            url: url.to_string(),
            kind: Some(kind),
            ..ServerConfig::default()
        },
    );
    settings
}

async fn drive(settings: &McpSettings) -> String {
    let (runtime, problems) = McpRuntime::connect(settings).await;
    assert!(problems.is_empty(), "{problems:?}");
    let runtime = runtime.expect("a runtime");
    let tool = runtime
        .injector()
        .catalog()
        .get("remote/create_issue")
        .expect("the listed tool")
        .clone();
    let result = runtime
        .call(&tool, &json!({ "repo": "aster" }))
        .await
        .unwrap();
    runtime.shutdown().await;
    render_result(&result)
}

#[tokio::test]
async fn a_streamable_http_server_handshakes_lists_and_answers_a_call() {
    let (url, heads) = serve_streamable(false).await;
    let answer = drive(&remote_settings(&url, Transport::StreamableHttp)).await;
    assert_eq!(answer, "ran over http");

    let heads = heads.lock().unwrap();
    let posts: Vec<String> = heads
        .iter()
        .filter(|head| head.starts_with("POST"))
        .map(|head| head.to_lowercase())
        .collect();
    assert!(posts[0].contains("accept: application/json, text/event-stream"));
    assert!(!posts[0].contains("mcp-session-id"), "{posts:?}");
    // The session and revision the handshake settled ride along after it.
    assert!(
        posts[1..]
            .iter()
            .all(|head| head.contains("mcp-session-id: s-1")
                && head.contains("mcp-protocol-version: 2026-07-28")),
        "{posts:?}"
    );
    // The session is ended rather than left dangling on the server.
    let last = heads.last().expect("a request").to_lowercase();
    assert!(
        last.starts_with("delete") && last.contains("mcp-session-id: s-1"),
        "{last}"
    );
}

#[tokio::test]
async fn a_reply_framed_as_an_event_stream_is_read_the_same_way() {
    let (url, _) = serve_streamable(true).await;
    assert_eq!(
        drive(&remote_settings(&url, Transport::StreamableHttp)).await,
        "ran over http"
    );
}

#[tokio::test]
async fn the_legacy_sse_binding_posts_messages_and_reads_replies_off_the_stream() {
    let url = serve_sse().await;
    assert_eq!(
        drive(&remote_settings(&url, Transport::Sse)).await,
        "ran over http"
    );
}

#[tokio::test]
async fn a_refused_remote_server_is_reported_not_fatal() {
    let settings = remote_settings("http://127.0.0.1:1/mcp", Transport::StreamableHttp);
    let (_, problems) = McpRuntime::connect(&settings).await;
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].starts_with("remote is not reachable"),
        "{problems:?}"
    );
}

#[test]
fn a_transport_is_inferred_from_the_fields_a_server_declares() {
    let stdio = ServerConfig {
        command: "npx".into(),
        ..ServerConfig::default()
    };
    assert_eq!(stdio.transport(), Some(Transport::Stdio));

    let remote = ServerConfig {
        url: "https://example.com/mcp".into(),
        ..ServerConfig::default()
    };
    assert_eq!(remote.transport(), Some(Transport::StreamableHttp));

    let legacy = ServerConfig {
        url: "https://example.com/sse".into(),
        kind: Some(Transport::Sse),
        ..ServerConfig::default()
    };
    assert_eq!(legacy.transport(), Some(Transport::Sse));

    // A declared transport with nothing to reach is not a server.
    let mismatched = ServerConfig {
        command: "npx".into(),
        kind: Some(Transport::StreamableHttp),
        ..ServerConfig::default()
    };
    assert_eq!(mismatched.transport(), None);
    assert_eq!(ServerConfig::default().transport(), None);
}

#[test]
fn remote_servers_are_read_from_yaml_in_either_spelling() {
    let settings: crate::settings::Settings = serde_yaml::from_str(
        "mcp:\n  servers:\n    \
         one:\n      url: https://a.example/mcp\n      headers: {X-Tenant: acme}\n    \
         two:\n      type: sse\n      url: https://b.example/sse\n    \
         three:\n      transport: http\n      url: https://c.example/mcp\n",
    )
    .expect("parse");
    let servers = &settings.mcp.servers;
    assert_eq!(servers["one"].headers["X-Tenant"], "acme");
    assert_eq!(servers["one"].transport(), Some(Transport::StreamableHttp));
    assert_eq!(servers["two"].transport(), Some(Transport::Sse));
    assert_eq!(
        servers["three"].transport(),
        Some(Transport::StreamableHttp)
    );
}

#[test]
fn event_frames_are_parsed_across_chunk_boundaries() {
    let mut parser = super::http::Parser::default();
    assert!(parser.feed("event: endpoint\r\ndata: /mes").is_empty());
    let events =
        parser.feed("sages?id=1\r\n\r\n: keepalive\n\nevent: message\ndata: {\"a\":1}\n\n");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].name, "endpoint");
    assert_eq!(events[0].data, "/messages?id=1");
    assert_eq!(events[1].name, "message");
    assert_eq!(events[1].data, "{\"a\":1}");

    // Multi-line data joins with newlines, and an unnamed event still counts.
    let events = parser.feed("data: one\ndata: two\n\n");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name, "");
    assert_eq!(events[0].data, "one\ntwo");
}
