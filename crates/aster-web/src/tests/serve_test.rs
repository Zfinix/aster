#![cfg(test)]

use super::*;

async fn respond(method: &str, params: Value) -> Value {
    let message = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    let backend = WebBackend::from_env(&WebConfig::from_env());
    dispatch(&backend, &message, json!(1)).await
}

#[tokio::test]
async fn discover_offers_a_version_the_client_speaks() {
    let response = respond("server/discover", json!({})).await;
    let versions = response["result"]["supportedVersions"]
        .as_array()
        .expect("supportedVersions is an array");
    assert!(versions.iter().any(|v| v == PROTOCOL_VERSION), "{response}");
    assert!(
        response["result"]["capabilities"]["tools"].is_object(),
        "{response}"
    );
}

#[tokio::test]
async fn initialize_still_answers_a_legacy_client() {
    let response = respond("initialize", json!({"protocolVersion": "2025-11-25"})).await;
    assert_eq!(
        response["result"]["protocolVersion"],
        LEGACY_PROTOCOL_VERSION
    );
    assert_eq!(response["result"]["serverInfo"]["name"], "web");
}

#[tokio::test]
async fn tools_list_offers_the_catalog_and_no_cursor() {
    let response = respond("tools/list", json!({})).await;
    let tools = response["result"]["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    // Both are keyless, so they are there whatever the environment holds.
    assert!(names.contains(&"search"), "{names:?}");
    assert!(names.contains(&"extract"), "{names:?}");
    assert!(response["result"]["nextCursor"].is_null(), "{response}");
}

#[tokio::test]
async fn an_unknown_method_is_a_json_rpc_error() {
    let response = respond("resources/list", json!({})).await;
    assert_eq!(response["error"]["code"], METHOD_NOT_FOUND);
}

#[tokio::test]
async fn a_failing_tool_reports_in_the_result_not_as_an_error() {
    let response = respond("tools/call", json!({ "name": "extract", "arguments": {} })).await;
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], true);
    assert!(
        response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("extract failed"),
        "{response}"
    );
}

#[tokio::test]
async fn a_call_without_a_name_is_a_json_rpc_error() {
    let response = respond("tools/call", json!({ "arguments": {} })).await;
    assert_eq!(response["error"]["code"], INTERNAL_ERROR);
}

#[tokio::test]
async fn every_response_carries_the_request_id() {
    for method in ["server/discover", "tools/list", "nope"] {
        let response = respond(method, json!({})).await;
        assert_eq!(response["id"], 1, "{method}");
        assert_eq!(response["jsonrpc"], "2.0", "{method}");
    }
}
