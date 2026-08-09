use super::{extract_gifs, tool_line, truncate};

#[test]
fn extract_gifs_removes_bare_url_lines() {
    let reply = "Here you go!\nhttps://media.giphy.com/media/abc/giphy.gif";
    let (text, gifs) = extract_gifs(reply);
    assert_eq!(text, "Here you go!");
    assert_eq!(gifs, vec!["https://media.giphy.com/media/abc/giphy.gif"]);
}

#[test]
fn extract_gifs_unwraps_markdown_images() {
    let reply = "![party](https://media.tenor.com/xyz/party.gif)";
    let (text, gifs) = extract_gifs(reply);
    assert!(text.is_empty());
    assert_eq!(gifs, vec!["https://media.tenor.com/xyz/party.gif"]);
}

#[test]
fn extract_gifs_keeps_inline_mentions_in_text() {
    let reply = "see https://x.com/a.gif for the vibe";
    let (text, gifs) = extract_gifs(reply);
    assert_eq!(text, reply);
    assert_eq!(gifs, vec!["https://x.com/a.gif"]);
}

#[test]
fn extract_gifs_ignores_plain_replies() {
    let (text, gifs) = extract_gifs("no media here, just https://docs.rs");
    assert_eq!(text, "no media here, just https://docs.rs");
    assert!(gifs.is_empty());
}

#[test]
fn tool_line_labels_known_tools() {
    let line = tool_line("read_file", r#"{"path":"src/main.rs"}"#);
    assert_eq!(line, "📖 <b>Read</b> <code>main.rs</code>");
}

#[test]
fn tool_line_truncates_long_commands() {
    let command = format!(r#"{{"command":"{}"}}"#, "x".repeat(200));
    let line = tool_line("run_command", &command);
    assert!(line.len() < 140);
    assert!(line.contains('…'));
}

#[test]
fn tool_line_escapes_html_in_arguments() {
    let line = tool_line("read_file", r#"{"path":"a<b>.rs"}"#);
    assert!(line.contains("a&lt;b&gt;.rs"));
}

#[test]
fn approval_subject_unwraps_the_run_preview() {
    assert_eq!(super::approval_subject("run `git status`"), "git status");
}

#[test]
fn approval_subject_keeps_trailing_notes() {
    assert_eq!(
        super::approval_subject("run `rm -rf dist` (risky command)"),
        "rm -rf dist (risky command)"
    );
}

#[test]
fn approval_subject_passes_through_edit_previews() {
    assert_eq!(
        super::approval_subject("edit src/lib.rs (protected path)"),
        "edit src/lib.rs (protected path)"
    );
}

#[test]
fn plan_message_marks_the_step_in_flight() {
    let args = r#"{"steps":[
        {"label":"read the code","status":"done"},
        {"label":"write the fix","status":"in_progress"},
        {"label":"run tests","status":"pending"}]}"#;
    let plan = super::plan_message(args).expect("a plan");
    assert!(plan.contains("✅ read the code"));
    assert!(plan.contains("▶️ <b>write the fix</b>"));
    assert!(plan.contains("▫️ run tests"));
    assert!(plan.contains("1/3 done"));
}

#[test]
fn plan_message_is_none_without_steps() {
    assert!(super::plan_message(r#"{"steps":[]}"#).is_none());
    assert!(super::plan_message("not json").is_none());
}

#[test]
fn tool_line_shortens_deep_paths() {
    let line = tool_line(
        "read_file",
        r#"{"path":"crates/aster-policy/src/grants.rs"}"#,
    );
    assert_eq!(line, "📖 <b>Read</b> <code>grants.rs</code>");
}

#[test]
fn tool_line_compresses_regex_alternations() {
    let line = tool_line(
        "search_files",
        r#"{"query":"request_approval|Answer::Always|fn allowed"}"#,
    );
    assert_eq!(
        line,
        "🔎 <b>Search</b> <code>request_approval +2 more</code>"
    );
}

#[test]
fn tool_line_falls_back_to_name() {
    assert_eq!(tool_line("mystery", "{}"), "⚙️ <b>mystery</b>");
}

#[test]
fn truncate_respects_char_boundaries() {
    let text = "é".repeat(50);
    let cut = truncate(&text, 41);
    assert!(cut.ends_with('…'));
    assert!(cut.len() <= 44);
}
