use agent_client_protocol::schema::v1::{PlanEntryStatus, ToolKind};
use serde_json::json;

use super::{Calls, command_line, crlf, exit_code, kind, plan_from_args, title};
use aster_acp::Call;

fn explore_args(tools: &[&str]) -> serde_json::Value {
    let steps: Vec<_> = tools
        .iter()
        .enumerate()
        .map(|(i, tool)| {
            json!({ "tool": tool, "args": { "path": format!("src/f{i}.rs"), "query": "needle" } })
        })
        .collect();
    json!({ "steps": steps })
}

#[test]
fn explore_title_names_each_step() {
    let args = explore_args(&["read_file", "search_files"]);
    assert_eq!(
        title("explore", &args.to_string(), &args),
        "Read src/f0.rs, Searched \u{201c}needle\u{201d}"
    );
}

#[test]
fn explore_title_folds_the_tail() {
    let args = explore_args(&["read_file", "read_file", "read_file", "search_files"]);
    assert_eq!(
        title("explore", &args.to_string(), &args),
        "Read src/f0.rs, Read src/f1.rs, +2 more"
    );
}

#[test]
fn explore_title_without_steps_still_reads() {
    let args = json!({});
    assert_eq!(title("explore", "{}", &args), "Explored the repository");
}

#[test]
fn explore_title_reads_string_steps_and_loose_keys() {
    let steps = r#"[{"name":"read_file","arguments":"{\"path\":\"a.rs\"}"},{"tool":"search_files","input":{"query":"x"}}]"#;
    let args = json!({ "steps": steps });
    assert_eq!(
        title("explore", &args.to_string(), &args),
        "Read a.rs, Searched \u{201c}x\u{201d}"
    );
}

#[test]
fn explore_kind_follows_the_steps() {
    assert_eq!(
        kind("explore", &explore_args(&["read_file", "read_file"])),
        ToolKind::Read
    );
    assert_eq!(
        kind("explore", &explore_args(&["read_file", "find_files"])),
        ToolKind::Search
    );
}

#[test]
fn exit_code_reads_the_trailing_line() {
    assert_eq!(exit_code("stdout:\nhi\nexit code: 3", false), 3);
    assert_eq!(exit_code("stdout:\nhi\nexit code: 0", false), 0);
}

#[test]
fn exit_code_falls_back_on_the_error_flag() {
    assert_eq!(exit_code("error: command needs approval", true), 1);
    assert_eq!(exit_code("(no output)", false), 0);
    assert_eq!(exit_code("exit code: -1", false), 0);
}

#[test]
fn crlf_terminates_every_line_once() {
    assert_eq!(crlf("a\nb\r\nc"), "a\r\nb\r\nc");
}

#[test]
fn command_line_joins_binary_and_args() {
    let args = json!({ "command": "cargo", "args": ["test", "-p", "aster-cli"] });
    assert_eq!(
        command_line(&args).as_deref(),
        Some("cargo test -p aster-cli")
    );
    assert_eq!(
        command_line(&json!({ "command": "ls" })).as_deref(),
        Some("ls")
    );
    assert_eq!(command_line(&json!({})), None);
}

#[test]
fn plan_maps_step_statuses() {
    let args = json!({ "steps": [
        { "label": "a", "status": "done" },
        { "label": "b", "status": "in_progress" },
        { "label": "c" },
        { "status": "done" }
    ] });
    let plan = plan_from_args(&args).expect("plan");
    let statuses: Vec<_> = plan.entries.iter().map(|e| e.status.clone()).collect();
    assert_eq!(
        statuses,
        vec![
            PlanEntryStatus::Completed,
            PlanEntryStatus::InProgress,
            PlanEntryStatus::Pending
        ]
    );
}

#[test]
fn calls_front_is_the_oldest_unfinished() {
    let calls = Calls::default();
    for id in ["1", "2", "3"] {
        calls.push(Call {
            id: id.into(),
            name: "run_command".into(),
        });
    }
    assert_eq!(calls.current().map(|c| c.id).as_deref(), Some("1"));
    calls.finish("1");
    calls.finish("3");
    assert_eq!(calls.current().map(|c| c.id).as_deref(), Some("2"));
    calls.finish("2");
    assert!(calls.current().is_none());
}
