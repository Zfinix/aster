use super::*;

#[test]
fn the_salvage_log_keeps_only_the_most_recent_lines() {
    let log = std::sync::Mutex::new(Vec::new());
    for n in 0..SALVAGE_LINES + 5 {
        push_salvage(&log, &format!("line {n}"));
    }
    let lines = log.lock().unwrap();
    assert_eq!(lines.len(), SALVAGE_LINES);
    assert_eq!(lines.first().map(String::as_str), Some("line 5"));
    assert_eq!(lines.last().map(String::as_str), Some("line 44"));
}

#[test]
fn a_timed_out_task_reports_its_trail_instead_of_nothing() {
    let report = salvage_report(300, "read_file src/chat.rs\nsearch_files queue").unwrap();
    assert!(report.contains("300s time limit"));
    assert!(report.contains("read_file src/chat.rs"));
}

#[test]
fn a_task_that_did_nothing_before_timing_out_has_no_salvage() {
    assert_eq!(salvage_report(300, ""), None);
}

#[test]
fn the_wrap_up_grace_scales_with_the_budget_within_bounds() {
    let secs = |s| std::time::Duration::from_secs(s);
    assert_eq!(wrap_up_grace(secs(30)), secs(15));
    assert_eq!(wrap_up_grace(secs(300)), secs(60));
    assert_eq!(wrap_up_grace(secs(150)), secs(30));
    assert_eq!(wrap_up_grace(secs(3600)), secs(60));
}

#[test]
fn a_tool_line_names_what_the_tool_touched() {
    let ev = serde_json::json!({
        "type": "tool_call",
        "name": "read_file",
        "arguments": "{\"path\":\"src/chat.rs\"}"
    });
    assert_eq!(tool_line(&ev).as_deref(), Some("read_file src/chat.rs"));
}

#[test]
fn a_tool_line_joins_a_command_with_its_args() {
    let ev = serde_json::json!({
        "type": "tool_call",
        "name": "run_command",
        "arguments": "{\"command\":\"cargo\",\"args\":[\"test\",\"-p\",\"aster-cli\"]}"
    });
    assert_eq!(
        tool_line(&ev).as_deref(),
        Some("run_command cargo test -p aster-cli")
    );
}

#[test]
fn a_tool_line_survives_arguments_a_provider_sent_twice() {
    let ev = serde_json::json!({
        "type": "tool_call",
        "name": "search_files",
        "arguments": "{\"query\":\"Usage\"}{\"query\":\"Usage\"}"
    });
    assert_eq!(tool_line(&ev).as_deref(), Some("search_files Usage"));
}

#[test]
fn a_tool_line_without_a_known_argument_is_just_the_name() {
    let ev = serde_json::json!({ "type": "tool_call", "name": "list_files", "arguments": "{}" });
    assert_eq!(tool_line(&ev).as_deref(), Some("list_files"));
}
