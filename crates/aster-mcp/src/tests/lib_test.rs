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
