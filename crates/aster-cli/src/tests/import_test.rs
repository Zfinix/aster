use super::*;

fn server(name: &str) -> FoundServer {
    FoundServer {
        name: name.to_string(),
        scope: Scope::Global,
        value: json!({
            "command": "npx",
            "args": ["-y", "pkg"],
            "env": {"KEY": "v1"},
            "type": "stdio",
            "timeout": 30,
        }),
    }
}

#[test]
fn merge_creates_the_file_and_keeps_the_fields_aster_reads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".aster/mcp.json");
    let github = server("github");
    let names = merge_mcp_json(&path, &[&github]).unwrap();
    assert_eq!(names, ["github"]);

    let written: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(written["mcpServers"]["github"]["command"], json!("npx"));
    assert_eq!(
        written["mcpServers"]["github"]["args"],
        json!(["-y", "pkg"])
    );
    assert_eq!(written["mcpServers"]["github"]["env"]["KEY"], json!("v1"));
    assert_eq!(written["mcpServers"]["github"]["type"], json!("stdio"));
    // Fields aster does not read are dropped rather than copied blindly.
    assert!(written["mcpServers"]["github"].get("timeout").is_none());
}

#[test]
fn merge_leaves_existing_entries_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mcp.json");
    std::fs::write(
        &path,
        r#"{"mcpServers": {"github": {"command": "bun"}}, "other": 1}"#,
    )
    .unwrap();
    let github = server("github");
    let magic = server("magic");
    let names = merge_mcp_json(&path, &[&github, &magic]).unwrap();
    assert_eq!(names, ["magic"]);

    let written: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(written["mcpServers"]["github"]["command"], json!("bun"));
    assert_eq!(written["mcpServers"]["magic"]["command"], json!("npx"));
    assert_eq!(written["other"], json!(1));
}

#[test]
fn claude_message_content_keeps_text_and_drops_injected_wrappers() {
    let content = serde_json::json!([
        {"type": "text", "text": "<ide_opened_file>x</ide_opened_file>"},
        {"type": "text", "text": "real question"},
        {"type": "tool_use", "id": "t1", "name": "Read", "input": {}},
    ]);
    assert_eq!(content_text(&content), "real question");
    assert_eq!(content_text(&serde_json::json!("plain")), "plain");
    assert_eq!(
        content_text(&serde_json::json!([{"type": "input_text", "text": "codex q"}])),
        "codex q"
    );
}
