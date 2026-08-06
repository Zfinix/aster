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
