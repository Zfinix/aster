use super::*;

fn call(name: &str, args: Value) -> ToolCall {
    ToolCall {
        id: "call_1".into(),
        kind: "function".into(),
        function: aster_ai::ToolCallFunction {
            name: name.into(),
            arguments: args.to_string(),
        },
        extra_content: None,
    }
}

fn canonical(name: &str, args: Value) -> (String, Value) {
    let mut call = call(name, args);
    canonicalize(&mut call);
    let args = serde_json::from_str(&call.function.arguments).unwrap();
    (call.function.name, args)
}

#[test]
fn write_tools_create_through_edit_file() {
    for name in ["write_file", "Write", "create_file", "write_to_file"] {
        assert_eq!(
            canonical(name, json!({ "file_path": "a.txt", "content": "hi\n" })),
            (
                "edit_file".into(),
                json!({ "path": "a.txt", "replace": "hi\n" })
            ),
            "{name}"
        );
    }
}

#[test]
fn a_write_never_turns_into_a_replace() {
    assert_eq!(
        canonical(
            "write_file",
            json!({ "path": "a.txt", "old_string": "x", "content": "hi" })
        ),
        (
            "edit_file".into(),
            json!({ "path": "a.txt", "replace": "hi" })
        )
    );
}

#[test]
fn replace_tools_map_old_and_new_strings() {
    for name in ["Edit", "replace", "str_replace", "replace_in_file"] {
        assert_eq!(
            canonical(
                name,
                json!({ "file_path": "a.rs", "old_string": "a", "new_string": "b" })
            ),
            (
                "edit_file".into(),
                json!({ "path": "a.rs", "search": "a", "replace": "b" })
            ),
            "{name}"
        );
    }
}

#[test]
fn the_anthropic_text_editor_routes_by_command() {
    assert_eq!(
        canonical(
            "str_replace_based_edit_tool",
            json!({ "command": "view", "path": "a.rs", "view_range": [3, -1] })
        ),
        (
            "read_file".into(),
            json!({ "command": "view", "path": "a.rs", "start_line": 3 })
        )
    );
    assert_eq!(
        canonical(
            "str_replace_editor",
            json!({ "command": "create", "path": "a.rs", "file_text": "x" })
        ),
        (
            "edit_file".into(),
            json!({ "command": "create", "path": "a.rs", "replace": "x" })
        )
    );
    assert_eq!(
        canonical("str_replace_editor", json!({ "command": "undo_edit" })).0,
        "str_replace_editor"
    );
}

#[test]
fn reads_take_offset_and_limit() {
    assert_eq!(
        canonical(
            "Read",
            json!({ "file_path": "a.rs", "offset": 10, "limit": 5 })
        ),
        (
            "read_file".into(),
            json!({ "path": "a.rs", "start_line": 10, "end_line": 14 })
        )
    );
    assert_eq!(
        canonical(
            "read_file",
            json!({ "target_file": "a.rs", "start_line_one_indexed": "2" })
        ),
        (
            "read_file".into(),
            json!({ "path": "a.rs", "start_line": 2 })
        )
    );
}

#[test]
fn search_list_and_find_aliases() {
    assert_eq!(
        canonical("Grep", json!({ "pattern": "fn main", "path": "src" })),
        (
            "search_files".into(),
            json!({ "query": "fn main", "dir": "src" })
        )
    );
    assert_eq!(
        canonical("Glob", json!({ "pattern": "*.rs", "path": "src" })),
        (
            "find_files".into(),
            json!({ "pattern": "*.rs", "dir": "src" })
        )
    );
    assert_eq!(
        canonical("list_dir", json!({ "relative_workspace_path": "src" })),
        ("list_files".into(), json!({ "dir": "src" }))
    );
}

#[test]
fn shells_become_run_command() {
    assert_eq!(
        canonical(
            "Bash",
            json!({ "command": "ls -la", "description": "List" })
        ),
        (
            "run_command".into(),
            json!({ "command": "ls -la", "description": "List" })
        )
    );
    assert_eq!(
        canonical("shell", json!({ "command": ["bash", "-lc", "make test"] })),
        (
            "run_command".into(),
            json!({ "command": "bash", "args": ["-lc", "make test"] })
        )
    );
}

#[test]
fn todo_lists_become_plan_steps() {
    assert_eq!(
        canonical(
            "TodoWrite",
            json!({ "todos": [
                { "content": "Read", "status": "completed", "activeForm": "Reading" },
                { "content": "Fix", "status": "in_progress" },
                { "content": "Drop", "status": "cancelled" },
            ] })
        ),
        (
            "update_plan".into(),
            json!({ "steps": [
                { "label": "Read", "status": "done", "activeForm": "Reading" },
                { "label": "Fix", "status": "in_progress" },
                { "label": "Drop", "status": "skipped" },
            ] })
        )
    );
}

#[test]
fn ask_user_question_takes_the_first_question() {
    assert_eq!(
        canonical(
            "AskUserQuestion",
            json!({ "questions": [{
                "question": "Which?",
                "header": "Pick",
                "options": [{ "label": "A", "description": "a" }, { "label": "B" }],
            }] })
        ),
        (
            "ask_user".into(),
            json!({ "question": "Which?", "header": "Pick", "options": ["A", "B"] })
        )
    );
}

#[test]
fn provider_wrappers_are_stripped() {
    for name in [
        "functions.read_file",
        "default_api:read_file",
        "read_file<|channel|>commentary",
        "ReadFile",
        "read-file",
    ] {
        assert_eq!(
            canonical(name, json!({ "path": "a.rs" })),
            ("read_file".into(), json!({ "path": "a.rs" })),
            "{name}"
        );
    }
}

#[test]
fn a_call_in_asters_own_shape_is_left_byte_for_byte() {
    let raw = r#"{"search":"a","replace":"b","path":"x.rs"}"#;
    let mut call = call("edit_file", json!({}));
    call.function.arguments = raw.into();
    canonicalize(&mut call);
    assert_eq!(
        (
            call.function.name.as_str(),
            call.function.arguments.as_str()
        ),
        ("edit_file", raw)
    );
}

#[test]
fn unknown_tools_keep_their_name() {
    assert_eq!(
        canonical("web_fetch", json!({ "url": "x" })),
        ("web_fetch".into(), json!({ "url": "x" }))
    );
}
