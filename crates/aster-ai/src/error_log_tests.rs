use serde_json::{Value, json};
use std::path::PathBuf;

fn temp_home() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aster-error-log-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn read_log(home: &std::path::Path) -> Vec<serde_json::Value> {
    let text = std::fs::read_to_string(home.join(super::LOG_RELATIVE_PATH)).unwrap();
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn a_failure_is_recorded_with_status_body_and_request_shape() {
    let home = temp_home();
    super::log_request_failure(
        &home,
        "stealth/ox-alpha",
        "https://openrouter.ai/api/v1",
        400,
        "ERROR",
        &json!({
            "model": "stealth/ox-alpha",
            "temperature": 0.4000000059604645,
            "stream": true,
            "messages": [{"role": "user"}, {"role": "assistant"}],
            "tools": [{"function": {"name": "read_file"}}],
        }),
    );
    let entries = read_log(&home);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["status"], 400);
    assert_eq!(entries[0]["response_body"], "ERROR");
    assert_eq!(entries[0]["endpoint_host"], "openrouter.ai");
    let summary = &entries[0]["request_summary"];
    assert_eq!(summary["temperature"], 0.4000000059604645);
    assert_eq!(summary["message_count"], 2);
    assert_eq!(summary["tool_names"], json!(["read_file"]));
    assert!(summary.get("messages").is_none());
}

#[test]
fn message_contents_and_keys_never_reach_the_log() {
    let home = temp_home();
    super::log_request_failure(
        &home,
        "m",
        "https://example.com/v1",
        401,
        "unauthorized",
        &json!({
            "api_key": "sk-super-secret",
            "messages": [{"role": "user", "content": "SECRET CONTENT"}],
        }),
    );
    let text = std::fs::read_to_string(home.join(super::LOG_RELATIVE_PATH)).unwrap();
    assert!(!text.contains("sk-super-secret"));
    assert!(!text.contains("SECRET CONTENT"));
}

#[test]
fn an_oversized_response_body_is_truncated() {
    let home = temp_home();
    let body = "x".repeat(super::MAX_BODY_CHARS + 100);
    super::log_request_failure(&home, "m", "https://e.com", 500, &body, &json!({}));
    let entries = read_log(&home);
    let recorded = entries[0]["response_body"].as_str().unwrap();
    assert_eq!(recorded.chars().count(), super::MAX_BODY_CHARS + 1);
}

#[test]
fn the_log_stays_bounded() {
    let home = temp_home();
    for _ in 0..2000 {
        super::log_request_failure(&home, "m", "https://e.com", 429, "slow down", &Value::Null);
    }
    let path = home.join(super::LOG_RELATIVE_PATH);
    let len = path.metadata().unwrap().len();
    assert!(len <= super::MAX_LOG_BYTES, "log grew to {len}");
    // Truncation restarts rather than corrupts: what remains parses as lines.
    let text = std::fs::read_to_string(&path).unwrap();
    for line in text.lines() {
        assert!(serde_json::from_str::<serde_json::Value>(line).is_ok());
    }
}
