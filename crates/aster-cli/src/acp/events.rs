//! Translate the agent loop's stream events into ACP session updates.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, Diff, Meta, Plan, PlanEntry, PlanEntryPriority, PlanEntryStatus,
    ResourceLink, SessionId, SessionInfoUpdate, SessionNotification, SessionUpdate, Terminal,
    ToolCall, ToolCallContent, ToolCallLocation, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind,
};
use agent_client_protocol::{Client, ConnectionTo};
use aster_persist::MessageEvent;
use serde_json::{Value, json};

use crate::chat::ChatEventSink;

const MAX_RESULT_CHARS: usize = 8_000;
const MAX_TITLE_STEPS: usize = 2;

pub(super) use aster_acp::Calls;

pub(super) struct Sink {
    cx: ConnectionTo<Client>,
    session_id: SessionId,
    repo_root: PathBuf,
    terminal_cards: bool,
    calls: Arc<Calls>,
    agents: Mutex<HashMap<String, Vec<String>>>,
}

impl Sink {
    pub fn new(
        cx: ConnectionTo<Client>,
        session_id: SessionId,
        repo_root: PathBuf,
        terminal_cards: bool,
        calls: Arc<Calls>,
    ) -> Self {
        Self {
            cx,
            session_id,
            repo_root,
            terminal_cards,
            calls,
            agents: Mutex::new(HashMap::new()),
        }
    }

    pub fn into_chat_sink(self) -> ChatEventSink {
        Box::new(move |event| {
            for update in self.translate(&event) {
                self.send(update);
            }
        })
    }

    pub fn send(&self, update: SessionUpdate) {
        let notification = SessionNotification::new(self.session_id.clone(), update);
        if let Err(err) = self.cx.send_notification(notification) {
            tracing::warn!("acp: dropping session update: {err}");
        }
    }

    /// Rebuild one recorded message as the updates a live turn would have
    /// sent, so a resumed thread shows its thoughts and tool rows again.
    pub fn replay(&self, message: &MessageEvent) -> Vec<SessionUpdate> {
        let content = message.content.as_deref().unwrap_or("").trim();
        match message.role.as_str() {
            "user" => (!content.is_empty())
                .then(|| SessionUpdate::UserMessageChunk(ContentChunk::new(content.into())))
                .into_iter()
                .collect(),
            "assistant" => {
                let mut updates = Vec::new();
                if let Some(reasoning) = &message.reasoning
                    && !reasoning.text.trim().is_empty()
                {
                    updates.push(thought(reasoning.text.clone()));
                }
                if !content.is_empty() {
                    updates.push(message_chunk(content.to_string()));
                }
                for call in &message.tool_calls {
                    updates.extend(self.tool_call(
                        &call.id,
                        &call.function.name,
                        &call.function.arguments,
                    ));
                }
                updates
            }
            "tool" => {
                let Some(id) = message.tool_call_id.as_deref() else {
                    return Vec::new();
                };
                let name = self.calls.name_of(id).unwrap_or_default();
                self.tool_result(id, &name, content, content.starts_with("error: "))
            }
            _ => Vec::new(),
        }
    }

    fn translate(&self, event: &Value) -> Vec<SessionUpdate> {
        let text = |key: &str| event[key].as_str().unwrap_or("").to_string();
        match event["type"].as_str().unwrap_or("") {
            "token" | "text" => vec![message_chunk(text("content"))],
            "reasoning_delta" | "reasoning" => vec![thought(text("content"))],
            "injected" => vec![SessionUpdate::UserMessageChunk(ContentChunk::new(
                text("content").into(),
            ))],
            "citations" => {
                citations(&event["sources"]).map_or_else(Vec::new, |s| vec![message_chunk(s)])
            }
            "tool_call" => self.tool_call(&text("id"), &text("name"), &text("arguments")),
            "tool_result" => self.tool_result(
                &text("id"),
                &text("name"),
                &text("result"),
                event["error"].as_bool().unwrap_or(false),
            ),
            "agent_status" => self.agent_status(event),
            "agent_activity" => self.agent_activity(event),
            "goal_set" => vec![thought(format!(
                "Goal: {} (up to {} turns)\n",
                text("condition"),
                event["max_turns"].as_u64().unwrap_or(0)
            ))],
            "goal_verdict" => vec![thought(format!(
                "Goal check {}: {} ({})\n",
                event["turn"].as_u64().unwrap_or(0),
                text("verdict"),
                text("reason")
            ))],
            "title" => vec![SessionUpdate::SessionInfoUpdate(
                SessionInfoUpdate::new().title(text("title")),
            )],
            _ => Vec::new(),
        }
    }

    fn tool_call(&self, id: &str, name: &str, arguments: &str) -> Vec<SessionUpdate> {
        let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
        // The plan panel is the whole story; a tool row beside it is noise.
        if name == "update_plan" {
            return plan_from_args(&args)
                .map(SessionUpdate::Plan)
                .into_iter()
                .collect();
        }
        self.calls.push(aster_acp::Call {
            id: id.to_string(),
            name: name.to_string(),
        });
        let terminal = name == "run_command" && self.terminal_cards;
        let title = if terminal {
            command_line(&args).unwrap_or_else(|| "Ran a command".to_string())
        } else {
            title(name, arguments, &args)
        };
        let mut call = ToolCall::new(id.to_string(), title)
            .kind(kind(name, &args))
            .status(ToolCallStatus::InProgress)
            .raw_input(args.clone());
        let path = args["path"].as_str().filter(|p| !p.is_empty());
        match (name, path) {
            ("edit_file", Some(path)) => {
                let path = self.absolute(path);
                let replace = args["replace"].as_str().unwrap_or("");
                let search = args["search"].as_str().filter(|s| !s.is_empty());
                call = call
                    .content(vec![
                        Diff::new(path.clone(), replace)
                            .old_text(search.map(str::to_string))
                            .into(),
                    ])
                    .locations(vec![ToolCallLocation::new(path)]);
            }
            ("read_file", Some(path)) => {
                call = call
                    .content(vec![
                        ContentBlock::ResourceLink(self.file_link(path)).into(),
                    ])
                    .locations(vec![ToolCallLocation::new(self.absolute(path))]);
            }
            ("explore", _) => {
                let locations: Vec<ToolCallLocation> = steps(&args)
                    .filter(|(tool, _)| tool == "read_file")
                    .filter_map(|(_, step)| step["path"].as_str().map(|p| self.absolute(p)))
                    .map(ToolCallLocation::new)
                    .collect();
                if !locations.is_empty() {
                    call = call.locations(locations);
                }
            }
            ("run_command", _) if terminal => {
                let meta = Meta::from_iter([(
                    "terminal_info".to_string(),
                    json!({ "terminal_id": id, "cwd": self.repo_root }),
                )]);
                call = call
                    .content(vec![ToolCallContent::Terminal(Terminal::new(
                        id.to_string(),
                    ))])
                    .meta(Some(meta));
            }
            (_, Some(path)) => {
                call = call.locations(vec![ToolCallLocation::new(self.absolute(path))]);
            }
            _ => {}
        }
        vec![SessionUpdate::ToolCall(call)]
    }

    fn tool_result(&self, id: &str, name: &str, result: &str, error: bool) -> Vec<SessionUpdate> {
        self.calls.finish(id);
        if name == "update_plan" {
            return Vec::new();
        }
        let mut fields = ToolCallUpdateFields::new()
            .status(if error {
                ToolCallStatus::Failed
            } else {
                ToolCallStatus::Completed
            })
            .raw_output(Value::String(result.to_string()));
        let mut meta = None;
        match name {
            // A successful edit keeps its diff and a read keeps its link.
            "edit_file" | "read_file" if !error => {}
            "run_command" if self.terminal_cards => {
                meta = Some(Meta::from_iter([
                    (
                        "terminal_output".to_string(),
                        json!({ "terminal_id": id, "data": crlf(result) }),
                    ),
                    (
                        "terminal_exit".to_string(),
                        json!({ "terminal_id": id, "exit_code": exit_code(result, error) }),
                    ),
                ]));
            }
            _ => fields = fields.content(vec![clipped(result).into()]),
        }
        let update = ToolCallUpdate::new(id.to_string(), fields).meta(meta);
        vec![SessionUpdate::ToolCallUpdate(update)]
    }

    fn agent_status(&self, event: &Value) -> Vec<SessionUpdate> {
        let agent = event["agent"].as_str().unwrap_or("agent");
        let line = match event["status"].as_str().unwrap_or("") {
            "done" => format!(
                "{agent}: {}",
                event["report"].as_str().unwrap_or("done").trim()
            ),
            "error" => format!(
                "{agent} failed: {}",
                event["error"].as_str().unwrap_or("unknown error")
            ),
            _ => format!(
                "{agent}: running {}",
                event["task"].as_str().unwrap_or("").trim()
            ),
        };
        let done = event["done"].as_u64().unwrap_or(0);
        let total = event["total"].as_u64().unwrap_or(0);
        self.agent_update(event, line, Some(format!("Agents {done}/{total}")))
    }

    fn agent_activity(&self, event: &Value) -> Vec<SessionUpdate> {
        let agent = event["agent"].as_str().unwrap_or("agent");
        let line = event["line"].as_str().unwrap_or("").trim();
        if line.is_empty() {
            return Vec::new();
        }
        self.agent_update(event, format!("{agent}: {line}"), None)
    }

    fn agent_update(
        &self,
        event: &Value,
        line: String,
        title: Option<String>,
    ) -> Vec<SessionUpdate> {
        let call_id = event["call_id"].as_str().unwrap_or("").to_string();
        let lines = match self.agents.lock() {
            Ok(mut agents) => {
                let lines = agents.entry(call_id.clone()).or_default();
                lines.push(line);
                lines.join("\n")
            }
            Err(_) => line,
        };
        let fields = ToolCallUpdateFields::new()
            .title(title)
            .content(vec![lines.into()]);
        vec![SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            call_id, fields,
        ))]
    }

    fn file_link(&self, path: &str) -> ResourceLink {
        let absolute = self.absolute(path);
        ResourceLink::new(path, format!("file://{}", absolute.display()))
    }

    fn absolute(&self, path: &str) -> PathBuf {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            path
        } else {
            self.repo_root.join(path)
        }
    }
}

fn message_chunk(text: String) -> SessionUpdate {
    SessionUpdate::AgentMessageChunk(ContentChunk::new(text.into()))
}

fn thought(text: String) -> SessionUpdate {
    SessionUpdate::AgentThoughtChunk(ContentChunk::new(text.into()))
}

fn citations(sources: &Value) -> Option<String> {
    let sources = sources.as_array()?;
    if sources.is_empty() {
        return None;
    }
    let mut out = String::from("\n\nSources:\n");
    for source in sources {
        let url = source["url"].as_str().unwrap_or("");
        let title = source["title"].as_str().unwrap_or(url);
        out.push_str(&format!("- [{title}]({url})\n"));
    }
    Some(out)
}

fn title(name: &str, arguments: &str, args: &Value) -> String {
    if name != "explore" {
        return crate::tui::step_label(name, arguments);
    }
    let labels: Vec<String> = steps(args)
        .map(|(tool, step)| crate::tui::step_label(&tool, &step.to_string()))
        .collect();
    match labels.len() {
        0 => "Explored the repository".to_string(),
        n if n <= MAX_TITLE_STEPS => labels.join(", "),
        n => format!(
            "{}, +{} more",
            labels[..MAX_TITLE_STEPS].join(", "),
            n - MAX_TITLE_STEPS
        ),
    }
}

/// The same lenient read of `steps` the loop itself does: the array may
/// arrive as a JSON string, and each step names its tool and args loosely.
fn steps(args: &Value) -> impl Iterator<Item = (String, Value)> + '_ {
    let steps = match &args["steps"] {
        Value::Array(steps) => steps.clone(),
        Value::String(raw) => serde_json::from_str(raw).unwrap_or_default(),
        _ => Vec::new(),
    };
    steps.into_iter().map(|step| {
        let tool = ["tool", "name"]
            .iter()
            .find_map(|key| step[*key].as_str())
            .unwrap_or("")
            .to_string();
        let args = ["args", "arguments", "input", "parameters"]
            .iter()
            .find_map(|key| match &step[*key] {
                Value::Object(map) => Some(Value::Object(map.clone())),
                Value::String(raw) => serde_json::from_str(raw).ok(),
                _ => None,
            })
            .unwrap_or_else(|| json!({}));
        (tool, args)
    })
}

fn command_line(args: &Value) -> Option<String> {
    let binary = args["command"].as_str().filter(|c| !c.is_empty())?;
    let rest: Vec<&str> = args["args"]
        .as_array()
        .map(|list| list.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    Some(if rest.is_empty() {
        binary.to_string()
    } else {
        format!("{binary} {}", rest.join(" "))
    })
}

/// The editor's terminal needs a carriage return per line; the loop captured
/// pipes, not a pty.
fn crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

fn exit_code(result: &str, error: bool) -> u64 {
    let reported = result
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("exit code: "))
        .and_then(|code| code.trim().parse::<i64>().ok());
    match reported {
        Some(code) if code >= 0 => code as u64,
        _ => u64::from(error),
    }
}

fn clipped(text: &str) -> String {
    if text.chars().count() <= MAX_RESULT_CHARS {
        return text.to_string();
    }
    let head: String = text.chars().take(MAX_RESULT_CHARS).collect();
    format!("{head}\n… (truncated)")
}

fn plan_from_args(args: &Value) -> Option<Plan> {
    let steps = args["steps"].as_array()?;
    let entries = steps
        .iter()
        .filter_map(|step| {
            let label = step["label"].as_str()?;
            let status = match step["status"].as_str().unwrap_or("pending") {
                "in_progress" => PlanEntryStatus::InProgress,
                "done" | "skipped" => PlanEntryStatus::Completed,
                _ => PlanEntryStatus::Pending,
            };
            Some(PlanEntry::new(label, PlanEntryPriority::Medium, status))
        })
        .collect();
    Some(Plan::new(entries))
}

fn kind(name: &str, args: &Value) -> ToolKind {
    match name {
        "read_file" | "list_files" | "find_files" | "recall" | "read_skill" => ToolKind::Read,
        "search_files" | "ast_grep" => ToolKind::Search,
        "explore" => {
            let searches =
                steps(args).any(|(tool, _)| matches!(tool.as_str(), "search_files" | "find_files"));
            if searches {
                ToolKind::Search
            } else {
                ToolKind::Read
            }
        }
        "edit_file" | "ast_edit" | "remember" => ToolKind::Edit,
        "forget" => ToolKind::Delete,
        "run_command" | "run_tests" => ToolKind::Execute,
        "ask_user" => ToolKind::Think,
        "exit_plan_mode" => ToolKind::SwitchMode,
        "open_preview" => ToolKind::Fetch,
        _ if name.starts_with("lsp_") => ToolKind::Read,
        _ => ToolKind::Other,
    }
}

#[cfg(test)]
#[path = "tests/events_test.rs"]
mod tests;
