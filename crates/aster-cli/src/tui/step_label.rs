use serde_json::Value;

/// Friendly one-line label for a tool call, matching the desktop's stepLabel.
pub(crate) fn step_label(name: &str, args: &str) -> String {
    let parsed: Value = serde_json::from_str(args).unwrap_or(Value::Null);
    let s = |key: &str| parsed.get(key).and_then(Value::as_str).unwrap_or("");
    match name {
        "read_file" => match s("path") {
            "" => "Read file".to_string(),
            path => format!("Read {path}"),
        },
        "list_files" => match s("dir") {
            "" => "Listed the project root".to_string(),
            dir => format!("Listed {dir}"),
        },
        "search_files" => format!("Searched \u{201c}{}\u{201d}", s("query")),
        "find_files" => format!("Found files matching {}", s("pattern")),
        "run_command" => match s("description") {
            "" => match command_line(&parsed) {
                None => "Ran a command".to_string(),
                Some(line) => format!("Ran {line}"),
            },
            summary => summary.to_string(),
        },
        "edit_file" => match s("path") {
            "" => "Edited file".to_string(),
            path => format!("Edited {path}"),
        },
        "web/search" => "Sources".to_string(),
        "web_search" => match s("query") {
            "" => "Searched the web".to_string(),
            query => format!("Searched the web for \u{201c}{query}\u{201d}"),
        },
        "web_fetch" => match s("url") {
            "" => "Opened a web page".to_string(),
            url => format!("Opened {url}"),
        },
        "run_tests" => match s("filter") {
            "" => "Ran the test suite".to_string(),
            filter => format!("Ran tests matching {filter}"),
        },
        "command_job" => {
            let job = parsed["job"]
                .as_u64()
                .map_or_else(String::new, |id| format!(" {id}"));
            match s("action") {
                "output" => format!("Checked background command{job}"),
                "wait" => format!("Waited for background command{job}"),
                "stop" => format!("Stopped background command{job}"),
                _ => "Checked background commands".to_string(),
            }
        }
        "explore" => "Explored the repository".to_string(),
        "update_plan" => "Updated the plan".to_string(),
        "write_plan" => match parsed.get("content") {
            Some(_) => "Drafted the plan".to_string(),
            None => "Revised the plan".to_string(),
        },
        "exit_plan_mode" => "Asked to leave plan mode".to_string(),
        "ask_user" => match (s("header"), s("question")) {
            ("", "") => "Asked you a question".to_string(),
            ("", question) => format!("Asked \u{201c}{question}\u{201d}"),
            (header, _) => format!("Asked \u{201c}{header}\u{201d}"),
        },
        "ast_grep" => format!("Searched code for {}", s("pattern")),
        "ast_edit" => format!("Rewrote code matching {}", s("pattern")),
        "security_scan" => match s("path") {
            "" => "Scanned the project for security issues".to_string(),
            path => format!("Scanned {path} for security issues"),
        },
        "lsp_diagnostics" => format!("Checked {} for errors", s("path")),
        "lsp_references" => format!("Found uses of the symbol in {}", s("path")),
        "lsp_definitions" => format!("Found where the symbol in {} is defined", s("path")),
        "aster_mcp" => {
            let target = s("name");
            let (server, tool) = target.split_once('/').unwrap_or(("", target));
            let tool = tool.replace('_', " ");
            match (s("action"), server) {
                ("search", _) => format!(
                    "Searched connected tools for \u{201c}{}\u{201d}",
                    s("query")
                ),
                ("describe", _) => format!("Checked how {tool} works"),
                ("execute", "") if tool.is_empty() => "Used a connected tool".to_string(),
                ("execute", "") => format!("Used {tool}"),
                ("execute", server) => format!("Used {tool} from {server}"),
                _ => "Used a connected tool".to_string(),
            }
        }
        "open_preview" => match s("description") {
            "" => format!("Opened {}", s("target")),
            what => what.to_string(),
        },
        "remember" => "Saved to memory".to_string(),
        "recall" => match (s("name"), s("query")) {
            ("", "") => "Listed saved memories".to_string(),
            ("", query) => format!("Searched memory for \u{201c}{query}\u{201d}"),
            (name, _) => format!("Recalled {name}"),
        },
        "forget" => format!("Forgot {}", s("name")),
        "read_skill" => format!("Read skill {}", s("name")),
        "chat_history" => match parsed["id"].as_str() {
            Some(id) => format!("Read chat {id}"),
            None => "Listed saved chats".to_string(),
        },
        "agent" => {
            let names: Vec<&str> = parsed["tasks"]
                .as_array()
                .map(|a| a.iter().filter_map(|t| t["agent"].as_str()).collect())
                .unwrap_or_default();
            let total = names.len();
            let unique: Vec<&str> = {
                let mut seen = std::collections::BTreeSet::new();
                names.into_iter().filter(|n| seen.insert(*n)).collect()
            };
            if total == 0 {
                "agent".to_string()
            } else if total == 1 {
                format!("agent: {}", unique[0])
            } else if unique.len() == 1 {
                format!("agent ×{total}: {unique}", unique = unique[0])
            } else {
                format!("agent ×{total}: {}", unique.join(", "))
            }
        }
        other => other.replace('_', " "),
    }
}

fn command_line(args: &Value) -> Option<String> {
    let binary = args.get("command").and_then(Value::as_str)?;
    let rest = args
        .get("args")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    match rest.is_empty() {
        true => Some(binary.to_string()),
        false => Some(format!("{binary} {}", rest.join(" "))),
    }
}

#[cfg(test)]
#[path = "tests/step_label_test.rs"]
mod tests;
