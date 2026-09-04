//! Translate the agent loop's stream events into ACP session updates.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use agent_client_protocol::schema::v1::{
    ContentChunk, Diff, Plan, PlanEntry, PlanEntryPriority, PlanEntryStatus, SessionId,
    SessionInfoUpdate, SessionNotification, SessionUpdate, ToolCall, ToolCallLocation,
    ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use agent_client_protocol::{Client, ConnectionTo};
use serde_json::Value;

use crate::chat::ChatEventSink;

/// Tool output shown inline in the editor; the model still sees all of it.
const MAX_RESULT_CHARS: usize = 8_000;

pub(super) struct Sink {
    cx: ConnectionTo<Client>,
    session_id: SessionId,
    repo_root: PathBuf,
    /// Progress lines per `agent` tool call, so each update carries the whole
    /// picture rather than replacing the last line.
    agents: Mutex<HashMap<String, Vec<String>>>,
}

impl Sink {
    pub fn new(cx: ConnectionTo<Client>, session_id: SessionId, repo_root: PathBuf) -> Self {
        Self {
            cx,
            session_id,
            repo_root,
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

    fn translate(&self, event: &Value) -> Vec<SessionUpdate> {
        let text = |key: &str| event[key].as_str().unwrap_or("").to_string();
        match event["type"].as_str().unwrap_or("") {
            "token" | "text" => vec![message(text("content"))],
            "reasoning_delta" | "reasoning" => vec![thought(text("content"))],
            "injected" => vec![SessionUpdate::UserMessageChunk(ContentChunk::new(
                text("content").into(),
            ))],
            "citations" => citations(&event["sources"]).map_or_else(Vec::new, |s| vec![message(s)]),
            "tool_call" => self.tool_call(&text("id"), &text("name"), &text("arguments")),
            "tool_result" => vec![tool_result(
                &text("id"),
                &text("name"),
                &text("result"),
                event["error"].as_bool().unwrap_or(false),
            )],
            "agent_status" => self.agent_status(event),
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
        let title = crate::tui::step_label(name, arguments);
        let mut call = ToolCall::new(id.to_string(), title)
            .kind(kind(name))
            .status(ToolCallStatus::InProgress)
            .raw_input(args.clone());
        if let Some(path) = args["path"].as_str().filter(|p| !p.is_empty()) {
            let path = self.absolute(path);
            if name == "edit_file" {
                let replace = args["replace"].as_str().unwrap_or("");
                let search = args["search"].as_str().filter(|s| !s.is_empty());
                call = call.content(vec![
                    Diff::new(path.clone(), replace)
                        .old_text(search.map(str::to_string))
                        .into(),
                ]);
            }
            call = call.locations(vec![ToolCallLocation::new(path)]);
        }
        let mut updates = vec![SessionUpdate::ToolCall(call)];
        if name == "update_plan"
            && let Some(plan) = plan_from_args(&args)
        {
            updates.push(SessionUpdate::Plan(plan));
        }
        updates
    }

    fn agent_status(&self, event: &Value) -> Vec<SessionUpdate> {
        let call_id = event["call_id"].as_str().unwrap_or("").to_string();
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
        let lines = match self.agents.lock() {
            Ok(mut agents) => {
                let lines = agents.entry(call_id.clone()).or_default();
                lines.push(line);
                lines.join("\n")
            }
            Err(_) => line,
        };
        let fields = ToolCallUpdateFields::new()
            .title(format!("Agents {done}/{total}"))
            .content(vec![lines.into()]);
        vec![SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            call_id, fields,
        ))]
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

fn message(text: String) -> SessionUpdate {
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

fn tool_result(id: &str, name: &str, result: &str, error: bool) -> SessionUpdate {
    let mut fields = ToolCallUpdateFields::new()
        .status(if error {
            ToolCallStatus::Failed
        } else {
            ToolCallStatus::Completed
        })
        .raw_output(Value::String(result.to_string()));
    // A successful edit keeps its diff; every other result shows its text.
    if name != "edit_file" || error {
        fields = fields.content(vec![clipped(result).into()]);
    }
    SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(id.to_string(), fields))
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

fn kind(name: &str) -> ToolKind {
    match name {
        "read_file" | "list_files" | "find_files" | "recall" | "read_skill" => ToolKind::Read,
        "search_files" | "explore" | "ast_grep" => ToolKind::Search,
        "edit_file" | "ast_edit" | "remember" => ToolKind::Edit,
        "forget" => ToolKind::Delete,
        "run_command" | "run_tests" => ToolKind::Execute,
        "update_plan" | "ask_user" => ToolKind::Think,
        "exit_plan_mode" => ToolKind::SwitchMode,
        "open_preview" => ToolKind::Fetch,
        _ if name.starts_with("lsp_") => ToolKind::Read,
        _ => ToolKind::Other,
    }
}
