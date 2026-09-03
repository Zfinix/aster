use super::*;
use serde_json::json;

fn settings_with(command: &str) -> McpSettings {
    let mut settings = McpSettings::default();
    settings.servers.insert(
        "fake".into(),
        ServerConfig {
            command: command.into(),
            ..ServerConfig::default()
        },
    );
    settings
}

fn tool(name: &str) -> McpTool {
    McpTool {
        server: "fake".into(),
        name: name.into(),
        description: format!("Run {name}"),
        input_schema: json!({ "type": "object" }),
    }
}

#[test]
fn a_config_change_invalidates_the_fingerprint() {
    let before = fingerprint(&settings_with("node").servers);
    let after = fingerprint(&settings_with("python3").servers);
    assert_ne!(before, after);
    assert_eq!(before, fingerprint(&settings_with("node").servers));
}

#[test]
fn a_saved_listing_round_trips() {
    let dir = std::env::temp_dir().join(format!("mcp-cache-{}", std::process::id()));
    let path = dir.join("entry.json");
    let settings = settings_with("node");
    let eras = BTreeMap::from([("fake".to_string(), CachedEra::Legacy)]);
    save_at(&path, &settings, vec![tool("create_issue")], eras);

    let hit = load_at(&path, &settings).expect("a matching fingerprint");
    assert_eq!(hit.tools, vec![tool("create_issue")]);
    assert_eq!(hit.eras.get("fake"), Some(&CachedEra::Legacy));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_stale_fingerprint_is_not_served() {
    let dir = std::env::temp_dir().join(format!("mcp-cache-stale-{}", std::process::id()));
    let path = dir.join("entry.json");
    save_at(
        &path,
        &settings_with("node"),
        vec![tool("create_issue")],
        BTreeMap::new(),
    );
    assert!(load_at(&path, &settings_with("python3")).is_none());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_corrupt_file_is_a_miss_not_a_crash() {
    let dir = std::env::temp_dir().join(format!("mcp-cache-corrupt-{}", std::process::id()));
    let path = dir.join("entry.json");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&path, "not json").unwrap();
    assert!(load_at(&path, &settings_with("node")).is_none());
    let _ = std::fs::remove_file(&path);
}
