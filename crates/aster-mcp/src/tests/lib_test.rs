use super::*;

fn tool(server: &str, name: &str, description: &str) -> McpTool {
    McpTool {
        server: server.into(),
        name: name.into(),
        description: description.into(),
        input_schema: json!({
            "type": "object",
            "properties": { "repository": { "type": "string" } }
        }),
    }
}

fn injector(tools: Vec<McpTool>, available_context_tokens: usize) -> Injector {
    Injector::new(
        McpCatalog::new(tools).expect("catalog"),
        ProgressiveConfig {
            available_context_tokens,
            ..ProgressiveConfig::default()
        },
    )
    .expect("injector")
}

#[test]
fn small_catalogue_exposes_names_and_descriptions_but_not_schemas() {
    let injection = injector(
        vec![tool("github", "create_issue", "Create a GitHub issue")],
        10_000,
    )
    .inject()
    .expect("injection");

    assert!(matches!(injection.inventory, Inventory::Tools(_)));
    assert!(injection.prompt.contains("github/create_issue"));
    assert!(!injection.prompt.contains("repository"));
    assert_eq!(
        injection.bridge_tool["function"]["name"],
        Value::String("aster_mcp".into())
    );
}

#[test]
fn large_catalogue_exposes_servers_not_tool_names() {
    let tools = (0..25)
        .map(|n| tool("github", &format!("issue_{n}"), "Create a detailed issue"))
        .collect();
    let injection = injector(tools, 50).inject().expect("injection");

    assert!(matches!(injection.inventory, Inventory::Servers(_)));
    assert!(injection.prompt.contains("github"));
    assert!(!injection.prompt.contains("github/issue_0"));
}

#[test]
fn bridge_surface_is_constant_when_the_catalogue_grows() {
    let one = injector(vec![tool("github", "create_issue", "Create an issue")], 100)
        .inject()
        .expect("injection")
        .bridge_tool;
    let many = injector(
        (0..100)
            .map(|n| tool("github", &format!("issue_{n}"), "Create an issue"))
            .collect(),
        100,
    )
    .inject()
    .expect("injection")
    .bridge_tool;
    assert_eq!(one, many);
}

#[test]
fn search_describe_and_execute_are_scoped_to_the_catalogue() {
    let injector = injector(
        vec![
            tool("github", "create_issue", "Create a GitHub issue"),
            tool("slack", "send_message", "Send a Slack channel message"),
        ],
        1_000,
    );
    let matches = injector
        .route(r#"{"action":"search","query":"open github issue"}"#)
        .expect("search");
    assert!(matches!(
        matches,
        BridgeAction::Search(ref results) if results[0].id == "github/create_issue"
    ));

    let described = injector
        .route(r#"{"action":"describe","name":"github/create_issue"}"#)
        .expect("describe");
    assert!(matches!(described, BridgeAction::Describe(ref tool) if tool.name == "create_issue"));

    let denied =
        injector.route(r#"{"action":"execute","name":"admin/delete_everything","arguments":{}}"#);
    assert!(denied.is_err());
}

struct RecordingInvoker {
    called: Option<String>,
}

impl McpInvoker for RecordingInvoker {
    fn invoke(&mut self, tool: &McpTool, arguments: &Value) -> Result<Value> {
        self.called = Some(tool.id());
        Ok(json!({ "ok": true, "arguments": arguments }))
    }
}

#[test]
fn execute_invokes_the_resolved_real_tool() {
    let injector = injector(
        vec![tool("github", "create_issue", "Create a GitHub issue")],
        1_000,
    );
    let mut invoker = RecordingInvoker { called: None };
    let result = injector
        .handle(
            r#"{"action":"execute","name":"github/create_issue","arguments":{"repository":"aster"}}"#,
            &mut invoker,
        )
        .expect("execute");
    assert_eq!(invoker.called.as_deref(), Some("github/create_issue"));
    assert_eq!(result["ok"], Value::Bool(true));
}

#[test]
fn invalid_configuration_is_rejected() {
    let config = ProgressiveConfig {
        available_context_tokens: 0,
        ..ProgressiveConfig::default()
    };
    assert!(Injector::new(McpCatalog::new(Vec::new()).expect("catalog"), config).is_err());
}

#[test]
fn pinned_servers_stay_listed_when_the_catalogue_overflows() {
    let mut tools: Vec<McpTool> = (0..25)
        .map(|n| tool("github", &format!("issue_{n}"), "Create a detailed issue"))
        .collect();
    tools.push(tool("web", "search", "Search the web"));
    let injection = injector(tools, 50)
        .pin_servers(["web"])
        .inject()
        .expect("injection");

    assert!(matches!(injection.inventory, Inventory::Servers(_)));
    assert_eq!(injection.pinned.len(), 1);
    assert!(injection.prompt.contains("web/search"));
    assert!(!injection.prompt.contains("github/issue_0"));
}

#[test]
fn nothing_is_pinned_when_the_full_manifest_fits() {
    let injection = injector(vec![tool("web", "search", "Search the web")], 10_000)
        .pin_servers(["web"])
        .inject()
        .expect("injection");

    assert!(matches!(injection.inventory, Inventory::Tools(_)));
    assert!(injection.pinned.is_empty());
}

#[test]
fn search_lists_a_short_summary_and_describe_keeps_the_full_text() {
    let long = format!(
        "Updates a document with batch requests.\n\n{}",
        "Each request is one of many shapes. ".repeat(1_000)
    );
    let injector = injector(vec![tool("gdocs", "update_doc", &long)], 10_000);
    let BridgeAction::Search(found) = injector
        .route(r#"{"action":"search","query":"update document"}"#)
        .expect("search")
    else {
        panic!("search did not return matches");
    };
    assert_eq!(
        found,
        vec![ToolMatch {
            id: "gdocs/update_doc".into(),
            server: "gdocs".into(),
            name: "update_doc".into(),
            description: "Updates a document with batch requests.".into(),
            score: found[0].score,
        }]
    );
    let BridgeAction::Describe(described) = injector
        .route(r#"{"action":"describe","name":"gdocs/update_doc"}"#)
        .expect("describe")
    else {
        panic!("describe did not return the tool");
    };
    assert_eq!(described.description, long);
}

#[test]
fn a_long_description_full_of_filler_does_not_outrank_the_right_tool() {
    let filler = "Use this to step through a document and update it by applying a batch \
                  of changes to the text, the tables, and the styles. "
        .repeat(50);
    let injector = injector(
        vec![
            tool("gdocs", "google_update_doc", &filler),
            tool(
                "web",
                "search",
                "Search the web and return each result's title, URL, and snippet.",
            ),
        ],
        10_000,
    );
    let BridgeAction::Search(found) = injector
        .route(r#"{"action":"search","query":"search the web for how to build a castle step by step"}"#)
        .expect("search")
    else {
        panic!("search did not return matches");
    };
    assert_eq!(found[0].id, "web/search");
}

#[test]
fn a_long_first_paragraph_is_capped() {
    let listed = summary(&"word ".repeat(200));
    assert_eq!(listed.chars().count(), MAX_LISTED_DESCRIPTION_CHARS);
    assert!(listed.ends_with('…'));
}

#[test]
fn a_large_pinned_server_is_capped_and_pin_order_comes_first() {
    let long = "Does one browser thing. ".repeat(20);
    let mut tools: Vec<McpTool> = (0..200)
        .map(|i| tool("browser", &format!("action_{i:03}"), &long))
        .collect();
    tools.push(tool("web", "search", "Search the web"));
    let injection = injector(tools, 1_000)
        .pin_servers(["web", "browser"])
        .inject()
        .expect("injection");

    assert_eq!(injection.pinned[0].id, "web/search");
    let chars: usize = injection
        .pinned
        .iter()
        .map(|entry| entry.id.len() + entry.description.len())
        .sum();
    assert!(chars <= MAX_PINNED_CHARS, "{chars}");
    assert!(injection.pinned.len() < 201);
}
