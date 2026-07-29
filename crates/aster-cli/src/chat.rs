//! `aster chat`: a conversational turn with an agentic read/list/search/edit tool loop.

use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{env, fs, io};

use anyhow::{Context, Result, bail};
use aster_ai::{AiClient, ChatMessage};
use aster_persist::{MessageEvent, Store, SummaryEvent, TranscriptEvent};
use aster_policy::{Action, Decision, Grants, Policy};
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
    pub probe: Arc<bash_tools::ToolProbe>,
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

/// CLI spelling of [`aster_policy::Mode`]. `ask` and `deny` stay as hidden
/// aliases so older scripts and aster.yaml files keep working.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum PermissionModeArg {
    /// Explore the code and present a plan before editing.
    Plan,
    /// Ask for approval before each edit.
    Manual,
    /// Apply what passes the safety check, pause for anything risky.
    Auto,
    /// Edit files without asking.
    Edit,
    #[value(hide = true)]
    Ask,
    #[value(hide = true)]
    Deny,
}

impl From<PermissionModeArg> for aster_policy::Mode {
    fn from(arg: PermissionModeArg) -> Self {
        match arg {
            PermissionModeArg::Plan | PermissionModeArg::Deny => Self::Plan,
            PermissionModeArg::Manual | PermissionModeArg::Ask => Self::Manual,
            PermissionModeArg::Auto => Self::Auto,
            PermissionModeArg::Edit => Self::Edit,
        }
    }
}

/// Emits one NDJSON event per line on the `--stream` path.
pub(crate) type ChatEventSink = Box<dyn Fn(Value) + Send + Sync>;

/// A pending action the agent task sends to the UI loop, which replies through
/// `respond`. `scope` is the directory an "always allow" answer would cover;
/// `None` means the request has no path to remember, so the front-end offers
/// only yes or no.
pub(crate) struct ApprovalRequest {
    pub preview: String,
    pub scope: Option<PathBuf>,
    pub respond: oneshot::Sender<Answer>,
}

/// How the user answered an [`ApprovalRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Answer {
    Yes,
    No,
    /// Yes, and remember it: persist `scope` so the question stops recurring.
    Always,
}

impl Answer {
    pub(crate) fn allowed(self) -> bool {
        !matches!(self, Answer::No)
    }
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
`search_files`, and change it with `edit_file` when it is available. \
`search_files` supports regex syntax and respects `.gitignore`. \
`edit_file` also creates files: omit `search` and pass the whole contents as \
`replace`. \
Ground every claim about the code in what you actually read. Only edit files when the \
user asked for a change; keep edits minimal and in the file's existing style. \
After editing, state plainly which files you changed and what the change does. \
If `edit_file` is unavailable, say so and describe the change instead.";

#[derive(Args)]
pub struct ChatArgs {
    /// One-shot question, e.g. `aster chat "why is finding 2 critical?"`.
    #[arg(value_name = "PROMPT", conflicts_with = "messages_json")]
    prompt: Option<String>,

    /// Continue this repo's most recent session, seeding its prior history.
    /// Without it every session starts clean, in the TUI too.
    #[arg(long = "continue", conflicts_with = "messages_json")]
    continue_session: bool,

    /// Persist this turn into a session by id, resuming it if it exists and
    /// creating it if not. Alone it also seeds the session's prior history;
    /// with --messages-json (the caller owns history) it only records.
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

    /// How edits are gated, overriding aster.yaml `permissions.mode`: plan,
    /// manual, auto, or edit. `manual` needs a front-end that answers prompts:
    /// the TUI, or `--stream`. Anything but `plan` also enables the edit tool.
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
}

impl ChatArgs {
    /// True when both ends are a real terminal and no flag forced one-shot output.
    pub fn is_interactive(&self) -> bool {
        let one_shot = self.print
            || crate::json_mode()
            || self.no_tools
            || self.stream
            || self.messages_json.is_some();
        !one_shot && io::stdout().is_terminal() && io::stdin().is_terminal()
    }
}

#[derive(Deserialize)]
struct WireMessage {
    role: String,
    content: String,
}

/// `manual` needs the TUI or --stream to confirm edits; otherwise the agent is
/// read-only. `auto` still edits headlessly: only its risky paths would prompt.
fn ask_needs_front_end(mode: aster_policy::Mode, allow_edits: bool, can_prompt: bool) -> bool {
    allow_edits && mode == aster_policy::Mode::Manual && !can_prompt
}

pub async fn run(args: ChatArgs) -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let settings = crate::settings::Settings::load(Some(&repo_root))?;

    let llm = crate::provider::resolve(&settings.review, args.model.as_deref())?;
    let client = AiClient::new(llm.base_url, llm.api_key, llm.model).with_effort(llm.effort);

    let mut permissions = settings.permissions.clone();
    if let Some(mode) = args.permission_mode {
        permissions.mode = permissions.mode.stricter(mode.into());
    }

    // The TUI answers its own prompts, so it is editable unless the config or
    // --permission-mode says otherwise; --allow-edits only gates headless runs.
    let interactive = args.is_interactive();
    let allow_edits = match args.permission_mode {
        Some(_) => true,
        None => interactive || args.allow_edits,
    } && permissions.mode.can_edit();

    let can_prompt = args.is_interactive() || args.stream;
    let allow_edits =
        if !args.no_tools && ask_needs_front_end(permissions.mode, allow_edits, can_prompt) {
            eprintln!(
                "note: `ask` permissions confirm every edit and this run cannot ask, \
             so the agent is read-only. Use --permission-mode edit to let it edit."
            );
            false
        } else {
            allow_edits
        };

    let policy = Arc::new(Policy::compile(&permissions)?);
    let grants = Arc::new(configured_grants(&permissions, &repo_root));

    if args.is_interactive() {
        let seed = args.prompt.clone();
        return crate::tui::run_chat(
            client,
            repo_root,
            allow_edits,
            permissions,
            seed,
            args.continue_session,
        )
        .await;
    }

    if args.stream {
        return run_stream(args, client, repo_root, policy, grants, allow_edits).await;
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
            &grants,
            None,
            &mut edited,
            &ctx,
            None,
        )
        .await?
        .0
    };

    if crate::json_mode() {
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
        probe: Arc::new(bash_tools::ToolProbe::detect()),
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
            emit_line(&json!({
                "type": "approval_request",
                "preview": req.preview,
                // Present only when "always" is on the table, so a front-end
                // can label the option with what it would remember.
                "scope": req.scope.as_ref().map(|p| p.display().to_string()),
            }));
            let answer = tokio::task::spawn_blocking(read_approval_reply)
                .await
                .unwrap_or(Answer::No);
            let _ = req.respond.send(answer);
        }
    });
    tx
}

/// One line of `{"allow": bool}` on stdin, optionally with `"always": true` to
/// persist the request's scope. A closed pipe or junk denies.
fn read_approval_reply() -> Answer {
    let mut line = String::new();
    if io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
        return Answer::No;
    }
    let Ok(reply) = serde_json::from_str::<Value>(&line) else {
        return Answer::No;
    };
    if !reply.get("allow").and_then(Value::as_bool).unwrap_or(false) {
        return Answer::No;
    }
    match reply.get("always").and_then(Value::as_bool) {
        Some(true) => Answer::Always,
        _ => Answer::Yes,
    }
}

/// Run a turn as NDJSON events on stdout, reading approval replies from stdin.
async fn run_stream(
    args: ChatArgs,
    client: AiClient,
    repo_root: PathBuf,
    policy: Arc<Policy>,
    grants: Arc<Grants>,
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
        &grants,
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
/// prepend. Recording is explicit: only `--session` (resume-or-create by id)
/// and `--continue` (resume the repo's latest) persist anything — a bare
/// prompt or a `--messages-json` replay without `--session` is ephemeral and
/// never creates or reopens a transcript on its own.
fn resolve_headless_session(
    store: Option<&Store>,
    repo_root: &Path,
    args: &ChatArgs,
    model: &str,
) -> Result<(Option<Recorder>, Vec<ChatMessage>)> {
    let Some(store) = store else {
        return Ok((None, Vec::new()));
    };

    // The wire path (a UI replays full history) owns its history; with
    // `--session` it also records into that session, resuming it if the id is
    // already on disk — an explicit ask, since the id then keeps appending.
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

    let Some(transcript) = base else {
        // No explicit session: the turn is ephemeral, nothing is recorded.
        return Ok((None, Vec::new()));
    };
    let prior = transcript.to_chat_messages();
    let writer = store.resume_writer(repo_root, &transcript.meta.id)?;
    Ok((Some(recorder(writer)), prior))
}

fn recorder(writer: aster_persist::SessionWriter) -> Recorder {
    std::sync::Arc::new(std::sync::Mutex::new(writer))
}

/// One full agentic turn that also reports progress as it goes: streamed
/// tokens, tool call steps, and edit notifications arrive on `events` so a
/// front-end can render the turn live instead of waiting for the reply.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn agent_turn_streaming(
    client: AiClient,
    repo_root: PathBuf,
    history: Vec<ChatMessage>,
    allow_edits: bool,
    policy: Arc<Policy>,
    grants: Arc<Grants>,
    approver: Option<ApprovalSender>,
    ctx: SessionCtx,
    events: ChatEventSink,
) -> Result<(String, Vec<String>, Option<Vec<ChatMessage>>)> {
    let mut edited = Vec::new();
    let (reply, compacted) = agent_loop(
        &client,
        &repo_root,
        &history,
        allow_edits,
        &policy,
        &grants,
        approver.as_ref(),
        &mut edited,
        &ctx,
        Some(&events),
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
    grants: &Grants,
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
        // False when the endpoint ignored `stream` and the client fell back to a
        // whole response, which the commentary emit below has to make up for.
        let mut streamed = false;
        let msg = match client
            .complete_tools_stream_with(
                &client.model,
                wire.clone(),
                tools.clone(),
                CHAT_TEMPERATURE,
                |delta| {
                    streamed = true;
                    emit(json!({ "type": "token", "content": delta }));
                },
            )
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
        // Streaming already delivered it, so that path only needs the separator.
        if let Some(text) = msg.content.as_deref().filter(|c| !c.trim().is_empty()) {
            if streamed {
                emit(json!({ "type": "token", "content": "\n\n" }));
            } else {
                emit(json!({ "type": "text", "content": text }));
            }
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
                grants,
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
        .complete_tools_stream_with(&client.model, wire, Vec::new(), CHAT_TEMPERATURE, |delta| {
            emit(json!({ "type": "token", "content": delta }));
        })
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
                "description": "Read a file, with line numbers. Optionally a line range. Paths outside the repository (absolute, or starting with ~) are allowed but the user is asked to approve each one, so prefer repo-relative paths.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Repo-relative path, or an absolute/~ path outside the repo (needs approval)" },
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
                "description": "List the entries of a directory. Directories end with '/'. Paths outside the repository are allowed but the user is asked to approve each one.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "dir": { "type": "string", "description": "Repo-relative directory, or an absolute/~ path outside the repo (needs approval); omit for the root" }
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
                "description": "Replace text in a repository file, or create a new one. `search` must be copied verbatim from the file and match exactly once; include surrounding lines to disambiguate. Omit `search` to create a new file at `path` with `replace` as its whole contents; missing parent directories are created.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Repo-relative file path" },
                        "search": { "type": "string", "description": "Exact existing text to replace; omit or leave empty to create a new file" },
                        "replace": { "type": "string", "description": "Replacement text, or the new file's contents" }
                    },
                    "required": ["path", "replace"]
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
    grants: &Grants,
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
        "read_file" => match str_arg("path").context("read_file needs a `path`") {
            Ok(path) => {
                match resolve_for_read(repo_root, policy, grants, approver, ctx, &path).await {
                    Ok(target) => read_numbered(
                        &target,
                        args["start_line"].as_u64().map(|n| n as usize),
                        args["end_line"].as_u64().map(|n| n as usize),
                    ),
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        },
        "list_files" => {
            match resolve_dir(repo_root, policy, grants, approver, ctx, &str_arg("dir")).await {
                Ok(base) => list_files(&ctx.probe, &base),
                Err(e) => Err(e),
            }
        }
        "search_files" => match str_arg("query").context("search_files needs a `query`") {
            Ok(query) => {
                match resolve_dir(repo_root, policy, grants, approver, ctx, &str_arg("dir")).await {
                    Ok(base) => search_files(&ctx.probe, repo_root, policy, &query, &base),
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        },
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

/// Seed the session's grants from `permissions.additional_directories`.
/// Unreadable entries are dropped rather than failing the run: a stale entry in
/// aster.yaml should not stop the agent from starting.
pub(crate) fn configured_grants(
    permissions: &aster_policy::PermissionsConfig,
    repo_root: &Path,
) -> Grants {
    let configured = permissions
        .additional_directories
        .iter()
        .filter_map(|dir| edits::expand_home(dir).canonicalize().ok());
    let persisted = crate::persist::store()
        .map(|store| store.grants(repo_root).load())
        .unwrap_or_default();
    Grants::new(configured.chain(persisted))
}

/// The directory an approval covers: the file's parent, or the directory itself.
fn grant_root(resolved: &Path) -> PathBuf {
    if resolved.is_dir() {
        return resolved.to_path_buf();
    }
    resolved.parent().unwrap_or(resolved).to_path_buf()
}

/// Resolve a path the agent wants to read. In-repo paths go through the
/// policy's secret-read rules; anything outside the repo is gated on the
/// user's approval, which a headless run cannot give.
async fn resolve_for_read(
    repo_root: &Path,
    policy: &Policy,
    grants: &Grants,
    approver: Option<&ApprovalSender>,
    ctx: &SessionCtx,
    path: &str,
) -> Result<PathBuf> {
    let (resolved, scope) = edits::resolve_anywhere(repo_root, path)?;
    match scope {
        edits::Scope::InRepo => {
            let root = repo_root.canonicalize().unwrap_or_default();
            let relative = resolved.strip_prefix(&root).unwrap_or(&resolved);
            if let Decision::Deny { reason } = policy.evaluate(&Action::Read {
                path: &relative.to_string_lossy(),
            }) {
                bail!("{reason}");
            }
        }
        edits::Scope::Outside if !grants.allows(&resolved) => {
            // Grant the directory, not the file, so the rest of the session can
            // read its siblings without another prompt.
            let root = grant_root(&resolved);
            let preview = format!("read outside the repository:\n  {}", resolved.display());
            match request_approval(approver, preview, Some(root.clone())).await {
                Answer::No => bail!(
                    "{} is outside the repository and needs the user's approval; \
                     it was rejected or this run has no way to ask",
                    resolved.display()
                ),
                Answer::Yes => grants.grant(root),
                Answer::Always => {
                    grants.grant(root.clone());
                    if let Some(store) = &ctx.store
                        && let Err(e) = store.grants(repo_root).add(&root)
                    {
                        tracing::warn!("could not persist the grant for {}: {e:#}", root.display());
                    }
                }
            }
        }
        edits::Scope::Outside => {}
    }
    Ok(resolved)
}

/// An omitted or empty `dir` means the repo root, which never needs approval.
async fn resolve_dir(
    repo_root: &Path,
    policy: &Policy,
    grants: &Grants,
    approver: Option<&ApprovalSender>,
    ctx: &SessionCtx,
    dir: &Option<String>,
) -> Result<PathBuf> {
    match dir.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        Some(dir) => resolve_for_read(repo_root, policy, grants, approver, ctx, dir).await,
        None => Ok(repo_root.to_path_buf()),
    }
}

fn read_numbered(target: &Path, start: Option<usize>, end: Option<usize>) -> Result<String> {
    let content =
        fs::read_to_string(target).with_context(|| format!("reading {}", target.display()))?;
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

fn list_files(probe: &bash_tools::ToolProbe, base: &Path) -> Result<String> {
    bash_tools::list(probe, base, MAX_LIST_ENTRIES)
}

/// Search via the best available tool, then strip any paths the policy
/// blocks (secret files) so they never reach the model.
fn search_files(
    probe: &bash_tools::ToolProbe,
    repo_root: &Path,
    policy: &Policy,
    query: &str,
    base: &Path,
) -> Result<String> {
    let raw = bash_tools::search(probe, repo_root, base, query, MAX_SEARCH_HITS)?;
    let filtered: Vec<&str> = raw
        .lines()
        .filter(|line| {
            let path = line.split(':').next().unwrap_or("");
            !matches!(
                policy.evaluate(&Action::Read { path }),
                Decision::Deny { .. }
            )
        })
        .collect();
    if filtered.is_empty() {
        return Ok("no matches".into());
    }
    Ok(filtered.join("\n"))
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
        search: args["search"].as_str().unwrap_or_default().to_string(),
        replace: args["replace"]
            .as_str()
            .context("edit_file needs `replace`")?
            .to_string(),
    };
    // An empty `search` has nothing to match, so it means "create this file".
    let creating = block.search.is_empty();
    let (resolved, updated) = if creating {
        let resolved = edits::resolve_new_in_repo(repo_root, path)?;
        if resolved.exists() {
            bail!("{path} already exists; put the text to replace in `search`");
        }
        (resolved, block.replace.clone())
    } else {
        let (resolved, content) = edits::read_repo_file(repo_root, path)?;
        let updated = edits::apply_block(&content, &block)?;
        (resolved, updated)
    };
    let verb = if creating { "create" } else { "edit" };

    match policy.evaluate(&Action::Edit { path }) {
        Decision::Allow => {}
        Decision::Deny { reason } => bail!("edit blocked by policy: {reason}"),
        Decision::Prompt { .. } => {
            let preview = format!("{verb} {path}:\n{}", edits::preview(&block));
            if !request_approval(approver, preview, None).await.allowed() {
                bail!(
                    "edit needs user approval (permissions mode is `ask`); \
                     it was rejected or no interactive approver is available"
                );
            }
        }
    }

    if creating && let Some(parent) = resolved.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(&resolved, &updated).with_context(|| format!("writing {}", resolved.display()))?;
    if !edited.iter().any(|p| p == path) {
        edited.push(path.to_string());
    }
    let done = if creating { "created" } else { "edited" };
    Ok(format!("{done} {path}:\n{}", edits::preview(&block)))
}

/// Ask the front-end to approve a pending action. Headless callers have no
/// approver, so every request is a `No`.
async fn request_approval(
    approver: Option<&ApprovalSender>,
    preview: String,
    scope: Option<PathBuf>,
) -> Answer {
    let Some(tx) = approver else {
        return Answer::No;
    };
    let (respond, rx) = oneshot::channel();
    let request = ApprovalRequest {
        preview,
        scope,
        respond,
    };
    if tx.send(request).await.is_err() {
        return Answer::No;
    }
    rx.await.unwrap_or(Answer::No)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(path: &str, search: Option<&str>, replace: &str) -> Value {
        match search {
            Some(s) => json!({ "path": path, "search": s, "replace": replace }),
            None => json!({ "path": path, "replace": replace }),
        }
    }

    #[tokio::test]
    async fn edit_file_creates_a_missing_file_without_search() {
        let repo = tempfile::tempdir().unwrap();
        let policy = Policy::permissive();
        let mut edited = Vec::new();

        let out = edit_file(
            repo.path(),
            &policy,
            None,
            &args("docs/notes/test.md", None, "# Test\n"),
            &mut edited,
        )
        .await
        .unwrap();

        assert!(out.starts_with("created docs/notes/test.md"), "{out}");
        assert_eq!(
            fs::read_to_string(repo.path().join("docs/notes/test.md")).unwrap(),
            "# Test\n"
        );
        assert_eq!(edited, ["docs/notes/test.md"]);
    }

    #[tokio::test]
    async fn outside_reads_are_approved_by_the_front_end() {
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("notes.txt");
        fs::write(&target, "hello").unwrap();
        let policy = Policy::permissive();

        let (tx, mut rx) = mpsc::channel::<ApprovalRequest>(1);
        let answer = tokio::spawn(async move {
            let req = rx.recv().await.unwrap();
            assert!(
                req.preview.contains("outside the repository"),
                "{}",
                req.preview
            );
            let _ = req.respond.send(Answer::Yes);
        });

        let resolved = resolve_for_read(
            repo.path(),
            &policy,
            &Grants::default(),
            Some(&tx),
            &SessionCtx::default(),
            &target.to_string_lossy(),
        )
        .await
        .unwrap();

        answer.await.unwrap();
        assert_eq!(resolved, target.canonicalize().unwrap());
    }

    #[tokio::test]
    async fn a_grant_covers_the_rest_of_the_directory() {
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("a.txt"), "a").unwrap();
        fs::write(outside.path().join("b.txt"), "b").unwrap();
        let policy = Policy::permissive();
        let grants = Grants::default();

        let (tx, mut rx) = mpsc::channel::<ApprovalRequest>(1);
        let prompts = tokio::spawn(async move {
            let mut seen = 0;
            while let Some(req) = rx.recv().await {
                seen += 1;
                let _ = req.respond.send(Answer::Yes);
            }
            seen
        });

        for name in ["a.txt", "b.txt"] {
            let path = outside.path().join(name);
            resolve_for_read(
                repo.path(),
                &policy,
                &grants,
                Some(&tx),
                &SessionCtx::default(),
                &path.to_string_lossy(),
            )
            .await
            .unwrap();
        }
        drop(tx);

        assert_eq!(
            prompts.await.unwrap(),
            1,
            "the second read should be covered"
        );
        assert_eq!(grants.granted(), [outside.path().canonicalize().unwrap()]);
    }

    #[tokio::test]
    async fn configured_directories_never_prompt() {
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("a.txt"), "a").unwrap();

        let permissions = aster_policy::PermissionsConfig {
            additional_directories: vec![outside.path().to_string_lossy().into_owned()],
            ..Default::default()
        };

        let resolved = resolve_for_read(
            repo.path(),
            &Policy::permissive(),
            &configured_grants(&permissions, repo.path()),
            None,
            &SessionCtx::default(),
            &outside.path().join("a.txt").to_string_lossy(),
        )
        .await
        .unwrap();

        assert_eq!(
            resolved,
            outside.path().join("a.txt").canonicalize().unwrap()
        );
    }

    #[tokio::test]
    async fn outside_reads_are_denied_without_an_approver() {
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("notes.txt");
        fs::write(&target, "hello").unwrap();

        let err = resolve_for_read(
            repo.path(),
            &Policy::permissive(),
            &Grants::default(),
            None,
            &SessionCtx::default(),
            &target.to_string_lossy(),
        )
        .await
        .unwrap_err()
        .to_string();

        assert!(err.contains("needs the user's approval"), "{err}");
    }

    #[tokio::test]
    async fn edit_file_refuses_to_clobber_an_existing_file() {
        let repo = tempfile::tempdir().unwrap();
        fs::write(repo.path().join("test.md"), "keep me").unwrap();
        let policy = Policy::permissive();

        let err = edit_file(
            repo.path(),
            &policy,
            None,
            &args("test.md", None, "gone"),
            &mut Vec::new(),
        )
        .await
        .unwrap_err()
        .to_string();

        assert!(err.contains("already exists"), "{err}");
        assert_eq!(
            fs::read_to_string(repo.path().join("test.md")).unwrap(),
            "keep me"
        );
    }
}
