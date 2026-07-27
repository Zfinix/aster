//! `aster chat`: a conversational turn with an agentic read/list/search/edit tool loop.

use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{env, fs, io};

use anyhow::{Context, Result, bail};
use aster_ai::{AiClient, ChatMessage};
use aster_persist::{MessageEvent, Store, SummaryEvent, TranscriptEvent};
use aster_policy::{Action, Decision, Policy};
use clap::Args;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use crate::edits::{self, EditBlock};
use crate::persist::Recorder;

/// Persistence handles threaded through a chat turn: the live append handle for
/// this session's transcript, and the store used to read and write memory.
#[derive(Default, Clone)]
pub(crate) struct SessionCtx {
    pub recorder: Option<Recorder>,
    pub store: Option<Store>,
    pub skills: Arc<aster_skills::SkillSet>,
}

impl SessionCtx {
    fn record(&self, event: MessageEvent) {
        let Some(recorder) = &self.recorder else {
            return;
        };
        match recorder.lock() {
            Ok(mut writer) => {
                if let Err(e) = writer.append_message(event) {
                    tracing::warn!("failed to record transcript event: {e:#}");
                }
            }
            Err(e) => tracing::warn!("transcript writer lock poisoned: {e}"),
        }
    }

    fn record_summary(&self, content: &str, replaces_through: usize) {
        let Some(recorder) = &self.recorder else {
            return;
        };
        if let Ok(mut writer) = recorder.lock()
            && let Err(e) = writer.append(&TranscriptEvent::Summary(SummaryEvent::new(
                content,
                replaces_through,
            )))
        {
            tracing::warn!("failed to record summary event: {e:#}");
        }
    }

    fn memory_context(&self) -> Option<String> {
        let store = self.store.as_ref()?;
        match store.memory().load_context() {
            Ok(ctx) if !ctx.trim().is_empty() => Some(ctx),
            Ok(_) => None,
            Err(e) => {
                tracing::warn!("failed to load memory context: {e:#}");
                None
            }
        }
    }
}

/// Discover skills from the project (`.aster/skills`) and user-global
/// (`<config>/aster/skills`) roots, project taking precedence on name collision.
pub(crate) fn discover_skills(repo_root: &Path) -> Arc<aster_skills::SkillSet> {
    let mut roots = vec![repo_root.join(".aster").join("skills")];
    match crate::persist::home() {
        Ok(home) => roots.push(home.join("skills")),
        Err(e) => tracing::debug!("no global skills root: {e:#}"),
    }
    Arc::new(aster_skills::SkillSet::discover(&roots))
}

/// The agent persona plus the memory block, when any facts are stored.
fn system_prompt(ctx: &SessionCtx, tools: bool) -> String {
    let mut prompt = String::from(AGENT_SYSTEM_PROMPT);
    if tools {
        prompt.push_str(TOOLS_PROMPT);
        if let Some(index) = ctx.skills.render_index() {
            prompt.push_str("\n\n");
            prompt.push_str(&index);
        }
    }
    if let Some(memory) = ctx.memory_context() {
        prompt.push_str("\n\n");
        prompt.push_str(&memory);
    }
    prompt
}

/// CLI spelling of [`aster_policy::Mode`].
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum PermissionModeArg {
    /// Apply edits without confirmation.
    Auto,
    /// Confirm every edit before it lands.
    Ask,
    /// Refuse all edits.
    Deny,
}

impl From<PermissionModeArg> for aster_policy::Mode {
    fn from(arg: PermissionModeArg) -> Self {
        match arg {
            PermissionModeArg::Auto => Self::Auto,
            PermissionModeArg::Ask => Self::Ask,
            PermissionModeArg::Deny => Self::Deny,
        }
    }
}

/// Emits one NDJSON event per line on the `--stream` path.
pub(crate) type ChatEventSink = Box<dyn Fn(Value) + Send + Sync>;

/// A pending edit the agent task sends to the UI loop, which replies through `respond`.
pub(crate) struct ApprovalRequest {
    pub preview: String,
    pub respond: oneshot::Sender<bool>,
}

/// Channel for `ask` mode prompts; headless callers pass `None`, denying every prompt.
pub(crate) type ApprovalSender = mpsc::Sender<ApprovalRequest>;

const AGENT_SYSTEM_PROMPT: &str = include_str!("../prompts/aster-agent.md");
const CHAT_TEMPERATURE: f32 = 0.4;
/// Hard stop so a confused model cannot spin forever.
const MAX_TOOL_ROUNDS: usize = 12;
/// Caps tool output so one fat file cannot blow the context.
const MAX_TOOL_RESULT_CHARS: usize = 24_000;
const MAX_SEARCH_HITS: usize = 80;
const MAX_LIST_ENTRIES: usize = 200;
/// Total history size (chars) above which older turns are folded into a summary.
const COMPACT_BUDGET_CHARS: usize = 48_000;
/// Recent turns kept verbatim when compacting; everything older is summarized.
const COMPACT_KEEP_TAIL: usize = 6;

const TOOLS_PROMPT: &str = "\n\n## Tools\n\n\
You can inspect the repository with `read_file`, `list_files`, and \
`search_files`, and change it with `edit_file` when it is available. Ground \
every claim about the code in what you actually read. Only edit files when the \
user asked for a change; keep edits minimal and in the file's existing style. \
After editing, state plainly which files you changed and what the change does. \
If `edit_file` is unavailable, say so and describe the change instead.";

#[derive(Args)]
pub struct ChatArgs {
    /// One-shot question, e.g. `aster chat "why is finding 2 critical?"`.
    #[arg(value_name = "PROMPT", conflicts_with = "messages_json")]
    prompt: Option<String>,

    /// Continue this repo's most recent session, seeding its prior history.
    #[arg(long = "continue", conflicts_with = "messages_json")]
    continue_session: bool,

    /// Persist this turn into a session by id. Alone it also seeds the session's
    /// prior history; with --messages-json (the caller owns history) it only records.
    #[arg(long, value_name = "ID")]
    session: Option<String>,

    /// Read a JSON array of {"role","content"} messages from PATH, or `-` for
    /// stdin. Roles: "user" | "assistant" | "system". For editors and UIs.
    /// With --stream this must be a single line, since stdin stays open for
    /// approval replies.
    #[arg(long, value_name = "PATH")]
    messages_json: Option<String>,

    /// Model override (else ASTER_MODEL, aster.yaml, default).
    #[arg(long, value_name = "MODEL")]
    model: Option<String>,

    /// Let the agent edit repo files via its edit_file tool.
    #[arg(long)]
    allow_edits: bool,

    /// How edits are gated, overriding aster.yaml `permissions.mode`. `ask`
    /// needs a front-end that answers prompts: the TUI, or `--stream`.
    /// Anything but `deny` also enables the edit tool.
    #[arg(long, value_name = "MODE", value_enum)]
    permission_mode: Option<PermissionModeArg>,

    /// Stream the turn as NDJSON on stdout, one event per line, and read
    /// approval replies from stdin. For editors and UIs.
    #[arg(long, conflicts_with_all = ["tui", "json", "print"])]
    stream: bool,

    /// Plain single-shot chat: no read/search/edit tools.
    #[arg(long)]
    no_tools: bool,

    /// Open the interactive chat TUI (default in a terminal). Optional PROMPT seeds the first question.
    #[arg(long, conflicts_with_all = ["messages_json", "json", "no_tools", "print"])]
    tui: bool,

    /// Answer once and print plain text instead of opening the TUI (default when piped).
    #[arg(long, short = 'p', conflicts_with_all = ["messages_json", "json"])]
    print: bool,

    /// Emit {"reply", "edits", "usage"} as one JSON object on stdout.
    #[arg(long)]
    json: bool,
}

impl ChatArgs {
    /// True when both ends are a real terminal and no flag forced one-shot output.
    pub fn is_interactive(&self) -> bool {
        let one_shot =
            self.print || self.json || self.no_tools || self.stream || self.messages_json.is_some();
        !one_shot && io::stdout().is_terminal() && io::stdin().is_terminal()
    }
}

#[derive(Deserialize)]
struct WireMessage {
    role: String,
    content: String,
}

pub async fn run(args: ChatArgs) -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let settings = crate::settings::Settings::load(Some(&repo_root))?;

    let llm = crate::provider::resolve(&settings.review, args.model.as_deref())?;
    let client = AiClient::new(llm.base_url, llm.api_key, llm.model);

    let mut permissions = settings.permissions.clone();
    if let Some(mode) = args.permission_mode {
        permissions.mode = mode.into();
    }
    // `deny` means no edits at all, so the tool is withheld entirely; the other
    // two modes imply the caller wants editing available.
    let allow_edits = match args.permission_mode {
        Some(PermissionModeArg::Deny) => false,
        Some(_) => true,
        None => args.allow_edits,
    };
    let policy = Arc::new(Policy::compile(&permissions)?);

    if args.is_interactive() {
        let seed = args.prompt.clone();
        return crate::tui::run_chat(client, repo_root, allow_edits, permissions, seed).await;
    }

    if args.stream {
        return run_stream(args, client, repo_root, policy, allow_edits).await;
    }

    let (ctx, history) = prepare_turn(&args, &repo_root, &client)?;

    let mut edited: Vec<String> = Vec::new();
    let reply = if args.no_tools {
        let mut messages = vec![ChatMessage {
            role: "system".into(),
            content: system_prompt(&ctx, false),
        }];
        messages.extend(history);
        let reply = client
            .complete_messages(&messages, CHAT_TEMPERATURE)
            .await?;
        ctx.record(MessageEvent::assistant(Some(reply.clone()), Vec::new()));
        reply
    } else {
        agent_loop(
            &client,
            &repo_root,
            &history,
            allow_edits,
            &policy,
            None,
            &mut edited,
            &ctx,
            None,
        )
        .await?
        .0
    };

    if args.json {
        let u = client.usage_snapshot();
        let out = json!({
            "reply": reply,
            "edits": edited,
            "usage": {
                "prompt_tokens": u.prompt_tokens,
                "completion_tokens": u.completion_tokens,
                "total_tokens": u.total_tokens,
                "requests": u.requests,
                "estimated_cost_usd": u.estimated_cost_usd,
                "estimated": u.estimated,
            },
        });
        println!("{out}");
    } else {
        println!("{reply}");
        for path in &edited {
            eprintln!("  ✎ edited {path}");
        }
        crate::review::print_usage(client.usage_snapshot());
    }
    Ok(())
}

/// Resolve the session and assemble the history for one headless turn.
fn prepare_turn(
    args: &ChatArgs,
    repo_root: &Path,
    client: &AiClient,
) -> Result<(SessionCtx, Vec<ChatMessage>)> {
    let new_turns = read_history(args)?;
    let store = crate::persist::store().ok();
    let (recorder, prior) =
        resolve_headless_session(store.as_ref(), repo_root, args, &client.model)?;
    let ctx = SessionCtx {
        recorder,
        store,
        skills: discover_skills(repo_root),
    };
    // Record only the new user turn. On the wire path `new_turns` is the full
    // replayed history, whose earlier turns were recorded on previous calls.
    if let Some(last) = new_turns.last().filter(|m| m.role == "user") {
        ctx.record(MessageEvent::user(last.content.clone()));
    }
    let mut history = prior;
    history.extend(new_turns);
    Ok((ctx, history))
}

/// Serialize one NDJSON event to stdout. Every write flushes so the reader sees
/// events as they happen rather than when the pipe buffer fills.
fn emit_line(value: &Value) {
    let mut out = io::stdout();
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

/// Bridge `ask` prompts to the caller: write an `approval_request` line, then
/// block on one reply line of `{"allow": bool}`.
fn stdio_approver() -> ApprovalSender {
    let (tx, mut rx) = mpsc::channel::<ApprovalRequest>(1);
    tokio::spawn(async move {
        while let Some(req) = rx.recv().await {
            emit_line(&json!({ "type": "approval_request", "preview": req.preview }));
            let allow = tokio::task::spawn_blocking(read_approval_reply)
                .await
                .unwrap_or(false);
            let _ = req.respond.send(allow);
        }
    });
    tx
}

/// One line of `{"allow": bool}` on stdin. A closed pipe or junk denies.
fn read_approval_reply() -> bool {
    let mut line = String::new();
    if io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
        return false;
    }
    serde_json::from_str::<Value>(&line)
        .ok()
        .and_then(|v| v.get("allow").and_then(Value::as_bool))
        .unwrap_or(false)
}

/// Run a turn as NDJSON events on stdout, reading approval replies from stdin.
async fn run_stream(
    args: ChatArgs,
    client: AiClient,
    repo_root: PathBuf,
    policy: Arc<Policy>,
    allow_edits: bool,
) -> Result<()> {
    let (ctx, history) = prepare_turn(&args, &repo_root, &client)?;

    let sink: ChatEventSink = Box::new(|event| emit_line(&event));
    let mut edited: Vec<String> = Vec::new();
    let result = agent_loop(
        &client,
        &repo_root,
        &history,
        allow_edits,
        &policy,
        Some(&stdio_approver()),
        &mut edited,
        &ctx,
        Some(&sink),
    )
    .await;

    let u = client.usage_snapshot();
    match result {
        Ok((reply, _)) => emit_line(&json!({
            "type": "done",
            "reply": reply,
            "edits": edited,
            "usage": {
                "prompt_tokens": u.prompt_tokens,
                "completion_tokens": u.completion_tokens,
                "total_tokens": u.total_tokens,
                "requests": u.requests,
                "estimated_cost_usd": u.estimated_cost_usd,
                "estimated": u.estimated,
            },
        })),
        Err(e) => emit_line(&json!({ "type": "error", "message": format!("{e:#}") })),
    }
    Ok(())
}

/// The conversation to send, minus the system prompt.
fn read_history(args: &ChatArgs) -> Result<Vec<ChatMessage>> {
    if let Some(path) = args.messages_json.as_deref() {
        let raw = if path == "-" && args.stream {
            // Streaming keeps stdin open for approval replies, so the messages
            // are one line rather than everything up to EOF.
            let mut buf = String::new();
            io::stdin()
                .read_line(&mut buf)
                .context("reading the messages line from stdin")?;
            buf
        } else if path == "-" {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .context("reading messages from stdin")?;
            buf
        } else {
            fs::read_to_string(path).with_context(|| format!("reading {path}"))?
        };
        let wire: Vec<WireMessage> = serde_json::from_str(&raw)
            .context("parsing --messages-json: expected a JSON array of {role, content}")?;
        if wire.is_empty() {
            bail!("nothing to ask; --messages-json was an empty array");
        }
        return wire
            .into_iter()
            .map(|m| match m.role.as_str() {
                "user" | "assistant" | "system" => Ok(ChatMessage {
                    role: m.role,
                    content: m.content,
                }),
                other => bail!("unsupported role {other:?} in --messages-json"),
            })
            .collect();
    }

    match args.prompt.as_deref().map(str::trim) {
        Some(prompt) if !prompt.is_empty() => Ok(vec![ChatMessage {
            role: "user".into(),
            content: prompt.into(),
        }]),
        _ => bail!("nothing to ask; pass a prompt (aster chat \"...\") or --messages-json"),
    }
}

/// Resolve the session a headless turn records into, and the prior history to
/// prepend. The `--messages-json` wire path (desktop replays it) records nothing.
/// Otherwise a session is opened: `--session`/`--continue` resume an existing one,
/// a bare prompt starts a fresh one so it is resumable later.
fn resolve_headless_session(
    store: Option<&Store>,
    repo_root: &Path,
    args: &ChatArgs,
    model: &str,
) -> Result<(Option<Recorder>, Vec<ChatMessage>)> {
    let Some(store) = store else {
        return Ok((None, Vec::new()));
    };

    // The wire path (desktop replays full history) owns its history, so it only
    // records into a named session and never seeds prior turns.
    if args.messages_json.is_some() {
        let Some(id) = &args.session else {
            return Ok((None, Vec::new()));
        };
        let writer = store.session_writer_for(repo_root, id, repo_root, Some(model.to_string()))?;
        return Ok((Some(recorder(writer)), Vec::new()));
    }

    let base = if let Some(id) = &args.session {
        Some(
            store
                .resume(repo_root, id)
                .with_context(|| format!("no session {id:?} for this repo"))?,
        )
    } else if args.continue_session {
        store.latest(repo_root)?
    } else {
        None
    };

    let (prior, writer) = match base {
        Some(transcript) => {
            let prior = transcript.to_chat_messages();
            let writer = store.resume_writer(repo_root, &transcript.meta.id)?;
            (prior, writer)
        }
        None => (
            Vec::new(),
            store.new_session(repo_root, repo_root, Some(model.to_string()))?,
        ),
    };
    Ok((Some(recorder(writer)), prior))
}

fn recorder(writer: aster_persist::SessionWriter) -> Recorder {
    std::sync::Arc::new(std::sync::Mutex::new(writer))
}

/// One full agentic turn owning its inputs so it can be spawned as a task.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn agent_turn(
    client: AiClient,
    repo_root: PathBuf,
    history: Vec<ChatMessage>,
    allow_edits: bool,
    policy: Arc<Policy>,
    approver: Option<ApprovalSender>,
    ctx: SessionCtx,
) -> Result<(String, Vec<String>, Option<Vec<ChatMessage>>)> {
    let mut edited = Vec::new();
    let (reply, compacted) = agent_loop(
        &client,
        &repo_root,
        &history,
        allow_edits,
        &policy,
        approver.as_ref(),
        &mut edited,
        &ctx,
        None,
    )
    .await?;
    Ok((reply, edited, compacted))
}

/// Drive the model's tool calls until it answers in plain text or the round cap trips.
/// Tool failures return to the model as tool results so it can retry instead of dying.
#[allow(clippy::too_many_arguments)]
async fn agent_loop(
    client: &AiClient,
    repo_root: &Path,
    history: &[ChatMessage],
    allow_edits: bool,
    policy: &Policy,
    approver: Option<&ApprovalSender>,
    edited: &mut Vec<String>,
    ctx: &SessionCtx,
    events: Option<&ChatEventSink>,
) -> Result<(String, Option<Vec<ChatMessage>>)> {
    let emit = |event: Value| {
        if let Some(sink) = events {
            sink(event);
        }
    };
    let (history, compacted) = compact_if_needed(client, history, ctx).await?;
    let mut wire: Vec<Value> = vec![json!({
        "role": "system",
        "content": system_prompt(ctx, true),
    })];
    for m in &history {
        wire.push(serde_json::to_value(m)?);
    }
    let tools = tool_defs(allow_edits);

    for round in 0..MAX_TOOL_ROUNDS {
        let msg = match client
            .complete_tools(wire.clone(), tools.clone(), CHAT_TEMPERATURE)
            .await
        {
            Ok(msg) => msg,
            // Some models reject tool definitions; degrade to plain chat. Only
            // safe on round 0, before any tool turns entered the history.
            Err(e) if round == 0 && is_tool_unsupported(&e) => {
                tracing::debug!("model rejected tools; falling back to plain chat: {e:#}");
                let mut messages = vec![ChatMessage {
                    role: "system".into(),
                    content: system_prompt(ctx, false),
                }];
                messages.extend(history.iter().cloned());
                let reply = client
                    .complete_messages(&messages, CHAT_TEMPERATURE)
                    .await?;
                ctx.record(MessageEvent::assistant(Some(reply.clone()), Vec::new()));
                return Ok((reply, compacted));
            }
            Err(e) => return Err(e),
        };

        if msg.tool_calls.is_empty() {
            let reply = msg
                .content
                .filter(|c| !c.trim().is_empty())
                .context("model returned an empty reply")?;
            ctx.record(MessageEvent::assistant(Some(reply.clone()), Vec::new()));
            return Ok((reply, compacted));
        }

        ctx.record(MessageEvent::assistant(
            msg.content.clone(),
            msg.tool_calls.clone(),
        ));
        // Text the model emitted alongside its tool calls: its running commentary.
        if let Some(text) = msg.content.as_deref().filter(|c| !c.trim().is_empty()) {
            emit(json!({ "type": "text", "content": text }));
        }
        wire.push(json!({
            "role": "assistant",
            "content": msg.content,
            "tool_calls": msg.tool_calls,
        }));
        for call in &msg.tool_calls {
            emit(json!({
                "type": "tool_call",
                "id": call.id,
                "name": call.function.name,
                "arguments": call.function.arguments,
            }));
            let result = exec_tool(
                repo_root,
                allow_edits,
                policy,
                approver,
                &call.function.name,
                &call.function.arguments,
                edited,
                ctx,
            )
            .await;
            tracing::debug!(tool = %call.function.name, "tool call executed");
            let result = truncate(&result, MAX_TOOL_RESULT_CHARS);
            emit(json!({
                "type": "tool_result",
                "id": call.id,
                "name": call.function.name,
                "result": result,
                "error": result.starts_with("error: "),
            }));
            ctx.record(MessageEvent::tool(&call.id, &result));
            wire.push(json!({
                "role": "tool",
                "tool_call_id": call.id,
                "content": result,
            }));
        }
    }

    // Round cap tripped: force a final plain answer out of what was gathered.
    wire.push(json!({
        "role": "user",
        "content": "Stop using tools and answer now with what you have.",
    }));
    let msg = client
        .complete_tools(wire, Vec::new(), CHAT_TEMPERATURE)
        .await?;
    let reply = msg
        .content
        .filter(|c| !c.trim().is_empty())
        .context("model returned an empty reply after the tool-round limit")?;
    ctx.record(MessageEvent::assistant(Some(reply.clone()), Vec::new()));
    Ok((reply, compacted))
}

const COMPACT_PROMPT: &str = "You are compacting a conversation to fit a context \
window. Summarize the exchange below into a compact brief the assistant can \
continue from: the user's goals, the key decisions and facts established, the \
files and code touched, and any open threads or next steps. Be specific and \
terse. Do not add commentary or a preamble.";

async fn compact_if_needed(
    client: &AiClient,
    history: &[ChatMessage],
    ctx: &SessionCtx,
) -> Result<(Vec<ChatMessage>, Option<Vec<ChatMessage>>)> {
    let total: usize = history.iter().map(|m| m.content.len()).sum();
    if total <= COMPACT_BUDGET_CHARS || history.len() <= COMPACT_KEEP_TAIL + 2 {
        return Ok((history.to_vec(), None));
    }

    let split = history.len().saturating_sub(COMPACT_KEEP_TAIL);
    let summary = summarize(client, &history[..split]).await?;
    ctx.record_summary(&summary, split);

    let mut compacted = Vec::with_capacity(COMPACT_KEEP_TAIL + 1);
    compacted.push(ChatMessage {
        role: "assistant".into(),
        content: format!("Summary of earlier conversation:\n{summary}"),
    });
    compacted.extend(history[split..].iter().cloned());
    Ok((compacted.clone(), Some(compacted)))
}

async fn summarize(client: &AiClient, head: &[ChatMessage]) -> Result<String> {
    let mut transcript = String::new();
    for m in head {
        transcript.push_str(&m.role);
        transcript.push_str(": ");
        transcript.push_str(&m.content);
        transcript.push_str("\n\n");
    }
    let messages = vec![
        ChatMessage {
            role: "system".into(),
            content: COMPACT_PROMPT.into(),
        },
        ChatMessage {
            role: "user".into(),
            content: transcript,
        },
    ];
    client.complete_messages(&messages, 0.2).await
}

fn is_tool_unsupported(e: &anyhow::Error) -> bool {
    let text = format!("{e:#}").to_lowercase();
    text.contains("tool") || text.contains("function")
}

fn tool_defs(allow_edits: bool) -> Vec<Value> {
    let mut tools = vec![
        json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a file from the repository, with line numbers. Optionally a line range.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Repo-relative file path" },
                        "start_line": { "type": "integer", "description": "First line, 1-based (optional)" },
                        "end_line": { "type": "integer", "description": "Last line, inclusive (optional)" }
                    },
                    "required": ["path"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "list_files",
                "description": "List the entries of a repository directory. Directories end with '/'.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "dir": { "type": "string", "description": "Repo-relative directory; omit for the root" }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "search_files",
                "description": "Case-insensitive substring search across repository files. Returns path:line: text.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Text to search for" },
                        "dir": { "type": "string", "description": "Repo-relative directory to search under (optional)" }
                    },
                    "required": ["query"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "remember",
                "description": "Save a durable fact to memory so it survives across sessions. Use for lasting project facts, conventions, or user preferences the model should recall later. `title` creates a named memory block; without it the note is appended to project memory (ASTER.md).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "note": { "type": "string", "description": "The fact to remember, stated plainly" },
                        "title": { "type": "string", "description": "Optional short name for a dedicated memory block" }
                    },
                    "required": ["note"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "recall",
                "description": "Read a memory block's full contents by name. The system prompt lists recallable memory as name and description only; call this to load the full body of a block before relying on it.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "The memory block name, as listed under Recallable memory" }
                    },
                    "required": ["name"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "read_skill",
                "description": "Load a skill's full instructions by name. The system prompt lists skills as name and description only; call this to read a skill's body before following it, once a request matches its description.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "The skill name, as listed under Skills" }
                    },
                    "required": ["name"]
                }
            }
        }),
    ];
    if allow_edits {
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "edit_file",
                "description": "Replace text in a repository file. `search` must be copied verbatim from the file and match exactly once; include surrounding lines to disambiguate.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Repo-relative file path" },
                        "search": { "type": "string", "description": "Exact existing text to replace" },
                        "replace": { "type": "string", "description": "Replacement text" }
                    },
                    "required": ["path", "search", "replace"]
                }
            }
        }));
    }
    tools
}

/// Execute one tool call. Failures come back as plain text so the model can react.
#[allow(clippy::too_many_arguments)]
async fn exec_tool(
    repo_root: &Path,
    allow_edits: bool,
    policy: &Policy,
    approver: Option<&ApprovalSender>,
    name: &str,
    arguments: &str,
    edited: &mut Vec<String>,
    ctx: &SessionCtx,
) -> String {
    let args: Value = match serde_json::from_str(arguments) {
        Ok(v) => v,
        Err(e) => return format!("error: tool arguments were not valid JSON: {e}"),
    };
    let str_arg = |key: &str| args[key].as_str().map(str::to_string);

    let result = match name {
        "remember" => str_arg("note")
            .context("remember needs a `note`")
            .and_then(|note| remember(ctx, str_arg("title").as_deref(), &note)),
        "recall" => str_arg("name")
            .context("recall needs a `name`")
            .and_then(|name| recall(ctx, &name)),
        "read_skill" => str_arg("name")
            .context("read_skill needs a `name`")
            .and_then(|name| read_skill(ctx, &name)),
        "read_file" => str_arg("path")
            .context("read_file needs a `path`")
            .and_then(|path| {
                deny_secret_read(policy, &path)?;
                read_numbered(
                    repo_root,
                    &path,
                    args["start_line"].as_u64().map(|n| n as usize),
                    args["end_line"].as_u64().map(|n| n as usize),
                )
            }),
        "list_files" => list_files(repo_root, str_arg("dir").as_deref().unwrap_or("")),
        "search_files" => str_arg("query")
            .context("search_files needs a `query`")
            .and_then(|q| {
                search_files(
                    repo_root,
                    policy,
                    &q,
                    str_arg("dir").as_deref().unwrap_or(""),
                )
            }),
        "edit_file" if !allow_edits => Err(anyhow::anyhow!(
            "editing is disabled for this chat; tell the user to enable Allow edits"
        )),
        "edit_file" => edit_file(repo_root, policy, approver, &args, edited).await,
        other => Err(anyhow::anyhow!("unknown tool: {other}")),
    };
    result.unwrap_or_else(|e| format!("error: {e:#}"))
}

fn remember(ctx: &SessionCtx, title: Option<&str>, note: &str) -> Result<String> {
    let store = ctx
        .store
        .as_ref()
        .context("memory is unavailable; no store is open")?;
    let memory = store.memory();
    match title.map(str::trim).filter(|t| !t.is_empty()) {
        Some(title) => {
            memory.remember(title, note, note)?;
            Ok(format!("remembered under \"{title}\""))
        }
        None => {
            memory.append_project(note)?;
            Ok("remembered in project memory".to_string())
        }
    }
}

fn recall(ctx: &SessionCtx, name: &str) -> Result<String> {
    let store = ctx
        .store
        .as_ref()
        .context("memory is unavailable; no store is open")?;
    store.memory().read_block(name)
}

fn read_skill(ctx: &SessionCtx, name: &str) -> Result<String> {
    let skill = ctx
        .skills
        .get(name)
        .with_context(|| format!("no skill named {name:?}; check the Skills list"))?;
    skill.load_body()
}

fn deny_secret_read(policy: &Policy, path: &str) -> Result<()> {
    if let Decision::Deny { reason } = policy.evaluate(&Action::Read { path }) {
        bail!("{reason}");
    }
    Ok(())
}

fn read_numbered(
    repo_root: &Path,
    path: &str,
    start: Option<usize>,
    end: Option<usize>,
) -> Result<String> {
    let (_, content) = edits::read_repo_file(repo_root, path)?;
    let lines: Vec<&str> = content.lines().collect();
    let from = start.unwrap_or(1).max(1) - 1;
    let to = end.unwrap_or(lines.len()).min(lines.len());
    if from >= to {
        bail!("empty range: the file has {} lines", lines.len());
    }
    let body = lines[from..to]
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{:>5} | {l}", from + i + 1))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(body)
}

/// Directories never worth walking; mirrors the review path filter's defaults.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "dist",
    "build",
    "out",
    "vendor",
    ".hg",
    ".svn",
];

fn list_files(repo_root: &Path, dir: &str) -> Result<String> {
    let base = if dir.is_empty() {
        repo_root.to_path_buf()
    } else {
        edits::resolve_in_repo(repo_root, dir)?
    };
    let mut entries: Vec<String> = fs::read_dir(&base)
        .with_context(|| format!("listing {}", base.display()))?
        .filter_map(|e| e.ok())
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() {
                format!("{name}/")
            } else {
                name
            }
        })
        .collect();
    entries.sort();
    entries.truncate(MAX_LIST_ENTRIES);
    Ok(entries.join("\n"))
}

fn search_files(repo_root: &Path, policy: &Policy, query: &str, dir: &str) -> Result<String> {
    if query.trim().is_empty() {
        bail!("empty search query");
    }
    let base = if dir.is_empty() {
        repo_root.to_path_buf()
    } else {
        edits::resolve_in_repo(repo_root, dir)?
    };
    let needle = query.to_lowercase();
    let mut hits = Vec::new();
    let mut stack = vec![base];
    while let Some(current) = stack.pop() {
        if hits.len() >= MAX_SEARCH_HITS {
            break;
        }
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_str()) {
                    stack.push(path);
                }
                continue;
            }
            let rel = path
                .strip_prefix(repo_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            // Never surface secret-file contents in search results.
            if matches!(
                policy.evaluate(&Action::Read { path: &rel }),
                Decision::Deny { .. }
            ) {
                continue;
            }
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            for (no, line) in content.lines().enumerate() {
                if line.to_lowercase().contains(&needle) {
                    hits.push(format!("{rel}:{}: {}", no + 1, truncate(line.trim(), 240)));
                    if hits.len() >= MAX_SEARCH_HITS {
                        break;
                    }
                }
            }
            if hits.len() >= MAX_SEARCH_HITS {
                break;
            }
        }
    }
    if hits.is_empty() {
        return Ok("no matches".into());
    }
    Ok(hits.join("\n"))
}

async fn edit_file(
    repo_root: &Path,
    policy: &Policy,
    approver: Option<&ApprovalSender>,
    args: &Value,
    edited: &mut Vec<String>,
) -> Result<String> {
    let path = args["path"].as_str().context("edit_file needs a `path`")?;
    let block = EditBlock {
        search: args["search"]
            .as_str()
            .context("edit_file needs `search`")?
            .to_string(),
        replace: args["replace"]
            .as_str()
            .context("edit_file needs `replace`")?
            .to_string(),
    };
    let (resolved, content) = edits::read_repo_file(repo_root, path)?;
    let updated = edits::apply_block(&content, &block)?;

    match policy.evaluate(&Action::Edit { path }) {
        Decision::Allow => {}
        Decision::Deny { reason } => bail!("edit blocked by policy: {reason}"),
        Decision::Prompt { .. } => {
            let preview = format!("edit {path}:\n{}", edits::preview(&block));
            if !request_approval(approver, preview).await {
                bail!(
                    "edit needs user approval (permissions mode is `ask`); \
                     it was rejected or no interactive approver is available"
                );
            }
        }
    }

    fs::write(&resolved, &updated).with_context(|| format!("writing {}", resolved.display()))?;
    if !edited.iter().any(|p| p == path) {
        edited.push(path.to_string());
    }
    Ok(format!("edited {path}:\n{}", edits::preview(&block)))
}

/// Ask the front-end to approve a pending edit; false when headless or rejected.
async fn request_approval(approver: Option<&ApprovalSender>, preview: String) -> bool {
    let Some(tx) = approver else {
        return false;
    };
    let (respond, rx) = oneshot::channel();
    if tx.send(ApprovalRequest { preview, respond }).await.is_err() {
        return false;
    }
    rx.await.unwrap_or(false)
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut cut = max;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n... [truncated]", &text[..cut])
}
