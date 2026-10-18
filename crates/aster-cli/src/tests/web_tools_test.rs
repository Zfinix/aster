use super::*;

#[test]
fn a_web_search_call_runs_the_built_in_search_through_the_bridge() {
    let bridged = bridge_arguments("web_search", r#"{"query":"tokio latest"}"#).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&bridged).unwrap(),
        json!({ "action": "execute", "name": "web/search", "arguments": { "query": "tokio latest" } })
    );
}

#[test]
fn other_tools_are_not_bridged() {
    assert_eq!(bridge_arguments("read_file", "{}"), None);
}

#[test]
fn the_tools_are_offered_only_with_the_web_server_connected() {
    let web = aster_mcp::McpTool {
        server: "web".into(),
        name: "search".into(),
        description: "Search the web".into(),
        input_schema: json!({ "type": "object" }),
    };
    let with = aster_mcp::McpCatalog::new(vec![web]).unwrap();
    let without = aster_mcp::McpCatalog::new(Vec::new()).unwrap();
    assert_eq!((schemas(&with).len(), schemas(&without).len()), (2, 0));
}
