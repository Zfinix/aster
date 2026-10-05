//! Models carry tool names and argument shapes from the harnesses they were
//! trained on (`write_file`, `Bash`, `TodoWrite`, `old_string`). This maps
//! them onto Aster's tools so a habit never costs the user a failed step.

use aster_ai::ToolCall;
use serde_json::{Map, Value, json};

const PATH_KEYS: &[&str] = &[
    "file_path",
    "filepath",
    "filename",
    "file",
    "target_file",
    "absolute_path",
    "file_name",
];
const DIR_KEYS: &[&str] = &[
    "path",
    "directory",
    "dir_path",
    "target_directory",
    "relative_workspace_path",
    "folder",
];
const SEARCH_KEYS: &[&str] = &[
    "old_string",
    "old_str",
    "old_text",
    "old",
    "find",
    "search_text",
];
const REPLACE_KEYS: &[&str] = &[
    "new_string",
    "new_str",
    "new_text",
    "new",
    "replacement",
    "content",
    "contents",
    "file_text",
    "text",
    "code",
    "data",
];

/// Rewrites one model tool call in place. A call already in Aster's own
/// shape comes back byte for byte, so the history only changes when it must.
pub(crate) fn canonicalize(call: &mut ToolCall) {
    let Ok(Value::Object(args)) = crate::chat::parse_arguments(&call.function.arguments) else {
        call.function.name = tool_name(&call.function.name);
        return;
    };
    let before = args.clone();
    let (name, args) = rewrite(&call.function.name, args);
    if args != before {
        call.function.arguments = Value::Object(args).to_string();
    }
    call.function.name = name;
}

/// The name and arguments Aster's dispatcher expects for this call.
pub(crate) fn rewrite(
    raw_name: &str,
    mut args: Map<String, Value>,
) -> (String, Map<String, Value>) {
    let name = tool_name(raw_name);
    let name = match name.as_str() {
        "str_replace_editor" | "str_replace_based_edit_tool" | "text_editor" => {
            match args.get("command").and_then(Value::as_str) {
                Some("view") => "read_file",
                Some("create" | "str_replace") => "edit_file",
                _ => return (name, args),
            }
        }
        "read" | "view" | "view_file" | "cat" | "open_file" | "read_text_file" => "read_file",
        "write" | "write_file" | "create_file" | "write_to_file" | "new_file" | "save_file"
        | "create" => {
            for key in SEARCH_KEYS.iter().chain(["search"].iter()) {
                args.remove(*key);
            }
            "edit_file"
        }
        "edit" | "multi_edit" | "replace" | "str_replace" | "replace_in_file"
        | "search_replace" | "apply_edit" | "replace_text" => "edit_file",
        "ls" | "list" | "list_dir" | "list_directory" | "listdir" | "list_folder" => "list_files",
        "grep"
        | "rg"
        | "ripgrep"
        | "search"
        | "grep_search"
        | "search_file"
        | "search_file_content"
        | "search_code"
        | "codebase_search" => "search_files",
        "glob" | "find" | "find_file" | "file_search" | "find_by_name" | "glob_search" => {
            "find_files"
        }
        "bash"
        | "sh"
        | "shell"
        | "exec"
        | "execute"
        | "execute_command"
        | "run_shell_command"
        | "run_terminal_cmd"
        | "run_terminal_command"
        | "terminal"
        | "run"
        | "run_bash"
        | "shell_command"
        | "exec_command" => "run_command",
        "todo_write" | "write_todos" | "update_todos" | "todo" | "todos" | "set_plan" => {
            "update_plan"
        }
        "ask_user_question" | "ask_followup_question" | "ask_question" | "ask" => "ask_user",
        "save_memory" | "memory_save" | "store_memory" | "memorize" => "remember",
        "skill" | "load_skill" | "use_skill" => "read_skill",
        other => other,
    }
    .to_string();
    fix_args(&name, &mut args);
    (name, args)
}

/// Strips provider wrappers (`functions.read_file`, `default_api:read_file`,
/// leaked `<|channel|>` tokens) and folds `ReadFile`/`read-file` to snake case.
fn tool_name(raw: &str) -> String {
    let raw = raw.split("<|").next().unwrap_or(raw).trim();
    let raw = raw.rsplit(['.', ':', '/']).next().unwrap_or(raw);
    let mut out = String::with_capacity(raw.len() + 4);
    let mut prev_lower = false;
    for c in raw.chars() {
        match c {
            '-' | ' ' => out.push('_'),
            c if c.is_uppercase() => {
                if prev_lower {
                    out.push('_');
                }
                out.extend(c.to_lowercase());
            }
            c => out.push(c),
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
    }
    out
}

fn fix_args(name: &str, args: &mut Map<String, Value>) {
    match name {
        "read_file" => {
            take(args, "path", PATH_KEYS);
            line_range(args);
        }
        "edit_file" => {
            take(args, "path", PATH_KEYS);
            take(args, "search", SEARCH_KEYS);
            take(args, "replace", REPLACE_KEYS);
        }
        "list_files" => take(args, "dir", DIR_KEYS),
        "search_files" => {
            take(args, "query", &["pattern", "regex", "search", "text", "q"]);
            take(args, "dir", DIR_KEYS);
        }
        "find_files" => {
            take(
                args,
                "pattern",
                &["glob", "glob_pattern", "file_pattern", "name", "query"],
            );
            take(args, "dir", DIR_KEYS);
        }
        "run_command" => {
            take(args, "command", &["cmd", "script", "command_line"]);
            if let Some(Value::Array(argv)) = args.get("command").cloned()
                && let Some((first, rest)) = argv.split_first()
            {
                args.insert("command".into(), first.clone());
                args.insert("args".into(), Value::Array(rest.to_vec()));
            }
        }
        "update_plan" => {
            take(args, "steps", &["todos", "plan", "items", "tasks"]);
            if let Some(Value::Array(steps)) = args.get_mut("steps") {
                steps.iter_mut().for_each(plan_step);
            }
        }
        "ask_user" => ask_user(args),
        "remember" => take(args, "note", &["fact", "content", "text", "memory"]),
        "read_skill" => take(args, "name", &["skill", "skill_name"]),
        _ => {}
    }
}

/// Moves the first present alias into `key` when `key` itself is missing.
fn take(args: &mut Map<String, Value>, key: &str, aliases: &[&str]) {
    if args.get(key).is_some_and(|v| !v.is_null()) {
        return;
    }
    if let Some(value) = aliases.iter().find_map(|alias| args.remove(*alias)) {
        args.insert(key.into(), value);
    }
}

fn line_range(args: &mut Map<String, Value>) {
    take(
        args,
        "start_line",
        &["start_line_one_indexed", "line_start", "from_line", "start"],
    );
    take(
        args,
        "end_line",
        &[
            "end_line_one_indexed_inclusive",
            "line_end",
            "to_line",
            "end",
        ],
    );
    if let Some(Value::Array(range)) = args.remove("view_range") {
        let mut bounds = range.iter().filter_map(Value::as_i64);
        if let Some(start) = bounds.next() {
            args.entry("start_line").or_insert(json!(start.max(1)));
        }
        if let Some(end) = bounds.next().filter(|end| *end > 0) {
            args.entry("end_line").or_insert(json!(end));
        }
    }
    if !args.contains_key("start_line")
        && let Some(offset) = args.remove("offset").as_ref().and_then(number)
    {
        let start = offset.max(1);
        args.insert("start_line".into(), json!(start));
        if let Some(limit) = args
            .remove("limit")
            .as_ref()
            .and_then(number)
            .filter(|l| *l > 0)
        {
            args.entry("end_line").or_insert(json!(start + limit - 1));
        }
    }
    for key in ["start_line", "end_line"] {
        if let Some(n) = args.get(key).and_then(number) {
            args.insert(key.into(), json!(n));
        }
    }
}

fn number(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn plan_step(step: &mut Value) {
    let Value::Object(step) = step else {
        if let Value::String(label) = step {
            *step = json!({ "label": label, "status": "pending" });
        }
        return;
    };
    take(
        step,
        "label",
        &["content", "title", "step", "text", "description", "task"],
    );
    let status = match step.get("status").and_then(Value::as_str) {
        Some("completed" | "complete" | "finished") => "done",
        Some("cancelled" | "canceled") => "skipped",
        Some("in-progress" | "active" | "doing" | "running") => "in_progress",
        Some("todo" | "not_started" | "open") => "pending",
        _ => return,
    };
    step.insert("status".into(), json!(status));
}

fn ask_user(args: &mut Map<String, Value>) {
    if !args.contains_key("question")
        && let Some(Value::Array(questions)) = args.remove("questions")
        && let Some(Value::Object(first)) = questions.into_iter().next()
    {
        args.extend(first);
    }
    take(args, "question", &["prompt", "message", "text"]);
    take(args, "options", &["choices", "suggestions", "follow_up"]);
    if let Some(Value::Array(options)) = args.get_mut("options") {
        for option in options.iter_mut() {
            if let Some(label) = ["label", "text", "answer"]
                .iter()
                .find_map(|key| option.get(key).and_then(Value::as_str))
            {
                *option = json!(label);
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/tool_alias_test.rs"]
mod tests;
