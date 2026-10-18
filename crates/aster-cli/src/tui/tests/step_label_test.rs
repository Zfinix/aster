use super::step_label;

#[test]
fn a_command_step_names_the_command_it_ran() {
    assert_eq!(
        step_label(
            "run_command",
            r#"{"command":"cargo","args":["test","--all"]}"#
        ),
        "Ran cargo test --all"
    );
    assert_eq!(
        step_label("run_command", r#"{"command":"cargo"}"#),
        "Ran cargo"
    );
}

#[test]
fn a_command_step_prefers_the_models_own_summary() {
    assert_eq!(
        step_label(
            "run_command",
            r#"{"command":"bun","args":["run","build"],"description":"Rebuild the webview bundle"}"#
        ),
        "Rebuild the webview bundle"
    );
}

#[test]
fn a_half_streamed_command_label_does_not_invent_a_name() {
    assert_eq!(
        step_label("run_command", r#"{"args":["test"]}"#),
        "Ran a command"
    );
    assert_eq!(
        step_label("run_command", r#"{"command":"car"#),
        "Ran a command"
    );
}

#[test]
fn every_tool_says_what_it_did_instead_of_its_name() {
    let calls = [
        ("web_search", r#"{"query":"ratatui 0.29"}"#),
        ("web_fetch", r#"{"url":"https://docs.rs"}"#),
        ("run_tests", r#"{"filter":"roundtrip"}"#),
        ("run_tests", "{}"),
        ("command_job", r#"{"action":"wait","job":3}"#),
        ("command_job", r#"{"action":"list"}"#),
        ("update_plan", r#"{"steps":[]}"#),
        ("write_plan", r##"{"content":"# Plan"}"##),
        ("write_plan", r#"{"old_str":"a","new_str":"b"}"#),
        ("ask_user", r#"{"question":"Which branch?"}"#),
        ("security_scan", "{}"),
        ("lsp_diagnostics", r#"{"path":"src/lib.rs"}"#),
        ("aster_mcp", r#"{"action":"search","query":"create issue"}"#),
        (
            "aster_mcp",
            r#"{"action":"describe","name":"github/create_issue"}"#,
        ),
        (
            "aster_mcp",
            r#"{"action":"execute","name":"github/create_issue"}"#,
        ),
        ("recall", r#"{"query":"deploy"}"#),
        ("recall", "{}"),
    ];
    let labels: Vec<String> = calls
        .iter()
        .map(|(name, args)| step_label(name, args))
        .collect();
    assert_eq!(
        labels,
        [
            "Searched the web for \u{201c}ratatui 0.29\u{201d}",
            "Opened https://docs.rs",
            "Ran tests matching roundtrip",
            "Ran the test suite",
            "Waited for background command 3",
            "Checked background commands",
            "Updated the plan",
            "Drafted the plan",
            "Revised the plan",
            "Asked \u{201c}Which branch?\u{201d}",
            "Scanned the project for security issues",
            "Checked src/lib.rs for errors",
            "Searched connected tools for \u{201c}create issue\u{201d}",
            "Checked how create issue works",
            "Used create issue from github",
            "Searched memory for \u{201c}deploy\u{201d}",
            "Listed saved memories",
        ]
    );
}
