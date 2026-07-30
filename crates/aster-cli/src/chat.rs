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
use crate::util::usage_json;

/// Persistence handles threaded through a chat turn: the live append handle for
/// this session's transcript, and the store used to read and write memory.
#[derive(Default, Clone)]
pub(crate) struct SessionCtx {
    pub recorder: Option<Recorder>,
    pub store: Option<Store>,
    pub skills: Arc<aster_skills::SkillSet>,
    /// `AGENTS.md` and friends, read from the repo at session start.
    pub instructions: Arc<crate::instructions::Instructions>,
    pub probe: Arc<bash_tools::ToolProbe>,
    /// Plan state maintained by the `update_plan` tool, rendered as a progress
    /// strip in the TUI and the desktop UI.
    pub plan: std::sync::Arc<std::sync::Mutex<PlanState>>,
    /// Connected MCP servers, when any are configured and reachable.
    pub mcp: Option<crate::mcp::McpRuntime>,
    /// Per-turn caps, from aster.yaml `agent` and the environment.
    pub limits: Limits,
    /// YOLO mode: the session is running without sandbox restrictions.
    pub yolo: bool,
}

/// How long a turn may work before it has to answer, and how long one command
/// may run. Defaults suit real builds; `aster.yaml` and the env can lower them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    pub max_tool_rounds: usize,
    pub command_timeout_secs: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_tool_rounds: DEFAULT_MAX_TOOL_ROUNDS,
            command_timeout_secs: DEFAULT_COMMAND_TIMEOUT_SECS,
        }
    }
}

impl Limits {
    /// aster.yaml first, then the environment, which wins so one run can differ.
    pub(crate) fn resolve(agent: &crate::settings::Agent) -> Self {
        let env_usize = |key: &str| std::env::var(key).ok().and_then(|v| v.parse().ok());
        Self {
            max_tool_rounds: env_usize("ASTER_MAX_TOOL_ROUNDS")
                .or(agent.max_tool_rounds)
                .unwrap_or(DEFAULT_MAX_TOOL_ROUNDS)
                .max(1),
            command_timeout_secs: env_usize("ASTER_COMMAND_TIMEOUT")
                .or(agent.command_timeout_secs.map(|v| v as usize))
                .unwrap_or(DEFAULT_COMMAND_TIMEOUT_SECS)
                .max(1),
        }
    }
}

/// Phased plan the agent builds and tracks with `update_plan`. Read by the
/// `exit_plan_mode` tool and rendered by both front-ends.
#[derive(Debug, Default, Clone)]
pub(crate) struct PlanState {
    pub steps: Vec<PlanStep>,
}

/// One step in an agent's execution plan.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct PlanStep {
    pub label: String,
    #[serde(rename = "status")]
    pub status: PlanStepStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlanStepStatus {
    Pending,
    #[serde(rename = "in_progress")]
    InProgress,
    Done,
    Skipped,
    Blocked,
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

/// The agent persona, the repo's own instructions, and the memory block.
fn system_prompt(ctx: &SessionCtx, tools: bool) -> String {
    let mut prompt = String::from(AGENT_SYSTEM_PROMPT);
    // Ahead of tools and memory: these are the repo's standing rules, and they
    // shape how every other section gets used.
    if let Some(project) = ctx.instructions.render() {
        prompt.push_str("\n\n");
        prompt.push_str(&project);
    }
    if tools {
        prompt.push_str(TOOLS_PROMPT);
        if let Some(index) = ctx.skills.render_index() {
            prompt.push_str("\n\n");
            prompt.push_str(&index);
        }
    }
    if tools && let Some(injection) = ctx.mcp.as_ref().and_then(|m| m.injection()) {
        prompt.push_str("\n\n");
        prompt.push_str(&injection.prompt);
    }
    if tools && let Some(disabled) = ctx.mcp.as_ref().and_then(|m| m.disabled_servers_prompt()) {
        prompt.push_str("\n\n");
        prompt.push_str(&disabled);
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

/// A request the agent task sends to the UI loop: an edit needing approval, a
/// plan whose approval promotes the session to edit mode, or a question.
pub(crate) enum UiRequest {
    Approval(ApprovalRequest),
    /// Approving this outlives the turn, so the front-end must change its own
    /// mode rather than only unlocking the tool for the rest of this turn.
    PlanApproval(ApprovalRequest),
    Question(QuestionRequest),
}

/// A pending edit the agent wants to make; the UI renders a diff and asks the
/// user to confirm. `scope` is the directory an "always allow" answer covers;
/// `None` means the front-end offers only yes or no.
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

/// A structured question the agent asks the user, e.g. to disambiguate a plan.
pub(crate) struct QuestionRequest {
    pub header: String,
    pub question: String,
    /// 2-4 short options the user can pick from, plus an implicit "Other".
    pub options: Vec<String>,
    /// Resolves to the selected option text, or `None` when declined / headless.
    pub respond: oneshot::Sender<Option<String>>,
}

/// Channel for UI requests — approval prompts and agent questions. Headless
/// callers pass `None`, declining every prompt.
pub(crate) type UiSender = mpsc::Sender<UiRequest>;

const AGENT_SYSTEM_PROMPT: &str = include_str!("../prompts/aster-agent.md");
const CHAT_TEMPERATURE: f32 = 0.4;
/// Hard stop so a confused model cannot spin forever. High enough that real
/// multi-file work finishes inside it; `agent.max_tool_rounds` overrides.
const DEFAULT_MAX_TOOL_ROUNDS: usize = 60;
/// Caps tool output so one fat file cannot blow the context.
const MAX_TOOL_RESULT_CHARS: usize = 24_000;
/// Caps command output (stdout + stderr combined).
const MAX_SEARCH_HITS: usize = 80;
/// Lines shown either side of a hit, so a search usually answers on its own
/// instead of costing a follow-up `read_file`.
const SEARCH_CONTEXT_LINES: usize = 3;
const MAX_LIST_ENTRIES: usize = 200;
const MAX_FIND_HITS: usize = 100;
/// Nearby paths offered when a guessed path does not exist.
const MAX_PATH_SUGGESTIONS: usize = 8;
/// Maximum seconds a command may run before it is killed. Builds and test
/// suites live here, so it is minutes; `agent.command_timeout_secs` overrides.
const DEFAULT_COMMAND_TIMEOUT_SECS: usize = 300;
/// Total history size (chars) above which older turns are folded into a summary.
const COMPACT_BUDGET_CHARS: usize = 48_000;
/// Recent turns kept verbatim when compacting; everything older is summarized.
const COMPACT_KEEP_TAIL: usize = 6;

const TOOLS_PROMPT: &str = "\n\n## Tools\n\n\
You can inspect the repository with `read_file`, `list_files`, `find_files`, \
and `search_files`, and change it with `edit_file` when it is available. \
`search_files` searches file contents, supports regex syntax, and respects \
`.gitignore`. `find_files` locates files by name or glob; reach for it before \
guessing a path, and whenever a tool reports that a path does not exist. \
A path that does not exist is a wrong guess, not a failure: take the nearby \
paths the tool offers and try again. \
`edit_file` also creates files: omit `search` and pass the whole contents as \
`replace`. \
`run_command` runs a CLI tool or build command. Filesystem writes are \
restricted to the repo and temp directories, and secrets are dropped from \
the environment. Use it for builds, tests, and linters. Do not shell out \
to `rg`, `grep`, `find`, or `fd`: `search_files` and `find_files` already \
run them directly, without the overhead. \
Set `turbo: true` when the user asks to work offline or in turbo mode \
(blocks network access). Set `yolo: true` only when the user explicitly \
asks for yolo mode (no restrictions, requires extra confirmations). \
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

    /// Pick a session to resume from a list of this repo's saved sessions.
    /// Needs a terminal; with an ID it resumes that session directly.
    #[arg(long, value_name = "ID", num_args = 0..=1, conflicts_with_all = ["messages_json", "session"])]
    resume: Option<Option<String>>,

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

    /// Which session this run opens. `--resume <id>` and `--session <id>` name
    /// one outright; bare `--resume` defers the choice to the user.
    fn resume_mode(&self) -> Resume {
        match (&self.resume, &self.session) {
            (Some(Some(id)), _) | (_, Some(id)) => Resume::Id(id.clone()),
            (Some(None), _) => Resume::Pick,
            _ if self.continue_session => Resume::Latest,
            _ => Resume::New,
        }
    }
}

/// Which session a run opens with.
pub(crate) enum Resume {
    New,
    /// This repo's most recent session.
    Latest,
    Id(String),
    /// Chosen from a list once the UI is up.
    Pick,
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
    let client = crate::provider::resolve_client(&settings, args.model.as_deref())?;

    // The flag is the user asking for this run outright, so it replaces the
    // configured mode rather than only tightening it.
    let mut permissions = settings.permissions.clone();
    if let Some(mode) = args.permission_mode {
        permissions.mode = mode.into();
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
                "note: `manual` permissions confirm every edit and this run cannot ask, \
             so the agent is read-only. Pass --permission-mode edit (or auto), or run \
             with --stream so approvals have somewhere to go."
            );
            false
        } else {
            allow_edits
        };

    let policy = Arc::new(Policy::compile(&permissions)?);
    let grants = Arc::new(configured_grants(&permissions, &repo_root));

    let (mcp, mcp_problems) = crate::mcp::McpRuntime::connect(&settings.mcp).await;
    for problem in &mcp_problems {
        eprintln!("note: MCP server unavailable, {problem}");
    }

    let limits = Limits::resolve(&settings.agent);

    if args.is_interactive() {
        let seed = args.prompt.clone();
        return crate::tui::run_chat(
            client,
            repo_root,
            allow_edits,
            permissions,
            seed,
            args.resume_mode(),
            mcp,
            limits,
        )
        .await;
    }

    // A picker needs a terminal to draw in; naming the session is the way out.
    if matches!(args.resume_mode(), Resume::Pick) {
        anyhow::bail!(
            "--resume needs a terminal to show the session list. Pass an id instead: `aster sessions list`, then `aster chat --resume <ID>`"
        );
    }

    if args.stream {
        return run_stream(
            args,
            client,
            repo_root,
            policy,
            grants,
            allow_edits,
            mcp,
            limits,
        )
        .await;
    }

    let (ctx, history) = prepare_turn(&args, &repo_root, &client, mcp, limits)?;

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
            "usage": usage_json(&u),
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
    mcp: Option<crate::mcp::McpRuntime>,
    limits: Limits,
) -> Result<(SessionCtx, Vec<ChatMessage>)> {
    let new_turns = read_history(args)?;
    let store = crate::persist::store().ok();
    let (recorder, prior) =
        resolve_headless_session(store.as_ref(), repo_root, args, &client.model)?;
    let ctx = SessionCtx {
        recorder,
        store,
        skills: discover_skills(repo_root),
        instructions: Arc::new(crate::instructions::discover(repo_root)),
        probe: Arc::new(bash_tools::ToolProbe::detect()),
        plan: Default::default(),
        mcp,
        limits,
        yolo: false,
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

/// Bridge prompts to the caller: write an `approval_request` or `question`
/// line, then block on one reply line of JSON.
fn stdio_approver() -> UiSender {
    let (tx, mut rx) = mpsc::channel::<UiRequest>(1);
    tokio::spawn(async move {
        while let Some(req) = rx.recv().await {
            match req {
                UiRequest::Approval(a) | UiRequest::PlanApproval(a) => {
                    emit_line(&json!({
                        "type": "approval_request",
                        "preview": a.preview,
                        "scope": a.scope.as_ref().map(|p| p.display().to_string()),
                    }));
                    let answer = tokio::task::spawn_blocking(read_approval_reply)
                        .await
                        .unwrap_or(Answer::No);
                    let _ = a.respond.send(answer);
                }
                UiRequest::Question(q) => {
                    emit_line(&json!({
                        "type": "question",
                        "header": q.header,
                        "question": q.question,
                        "options": q.options,
                    }));
                    let answer = tokio::task::spawn_blocking(read_question_reply)
                        .await
                        .unwrap_or(None);
                    let _ = q.respond.send(answer);
                }
            }
        }
    });
    tx
}

/// One line of `{"choice": "string"}` on stdin, or `{"choice": null}`.
fn read_question_reply() -> Option<String> {
    let mut line = String::new();
    if io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
        return None;
    }
    let Ok(reply) = serde_json::from_str::<Value>(&line) else {
        return None;
    };
    reply
        .get("choice")
        .and_then(Value::as_str)
        .map(str::to_string)
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
#[allow(clippy::too_many_arguments)]
async fn run_stream(
    args: ChatArgs,
    client: AiClient,
    repo_root: PathBuf,
    policy: Arc<Policy>,
    grants: Arc<Grants>,
    allow_edits: bool,
    mcp: Option<crate::mcp::McpRuntime>,
    limits: Limits,
) -> Result<()> {
    let (ctx, history) = prepare_turn(&args, &repo_root, &client, mcp, limits)?;

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
            "usage": usage_json(&u),
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

    if let Some(prompt) = args
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        return Ok(vec![ChatMessage {
            role: "user".into(),
            content: prompt.into(),
        }]);
    }

    // Piped input is the prompt: `echo "why?" | aster` should just work.
    // Not on `--stream`, where stdin stays open for approval replies.
    if !args.stream && !io::stdin().is_terminal() {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .context("reading the prompt from stdin")?;
        if !buf.trim().is_empty() {
            return Ok(vec![ChatMessage {
                role: "user".into(),
                content: buf.trim().to_string(),
            }]);
        }
    }
    bail!("nothing to ask; pass a prompt (aster chat \"...\"), pipe one in, or use --messages-json")
}

/// Resolve the session a headless turn records into, and the prior history to
/// prepend. Recording is explicit: only `--session`/`--resume <id>` and
/// `--continue` persist anything; a bare prompt is ephemeral.
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

    let base = match args.resume_mode() {
        Resume::Id(id) => Some(
            store
                .resume(repo_root, &id)
                .with_context(|| format!("no session {id:?} for this repo"))?,
        ),
        Resume::Latest => store.latest(repo_root)?,
        Resume::New | Resume::Pick => None,
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
    approver: Option<UiSender>,
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
    mut allow_edits: bool,
    policy: &Policy,
    grants: &Grants,
    approver: Option<&UiSender>,
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

    for round in 0..ctx.limits.max_tool_rounds {
        let mut tools = tool_defs(allow_edits, approver.is_some());
        if let Some(injection) = ctx.mcp.as_ref().and_then(|m| m.injection()) {
            tools.push(injection.bridge_tool);
        }
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
                &mut allow_edits,
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

    // Round cap tripped: force a final plain answer out of what was gathered,
    // and say so, since a cut-off turn otherwise reads as the agent giving up.
    let rounds = ctx.limits.max_tool_rounds;
    emit(json!({
        "type": "notice",
        "message": format!(
            "stopped after {rounds} tool rounds; answering with what it has \
             (raise `agent.max_tool_rounds` in aster.yaml or ASTER_MAX_TOOL_ROUNDS)"
        ),
    }));
    wire.push(json!({
        "role": "user",
        "content": "Stop using tools and answer now with what you have. Say plainly what you did not get to.",
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

fn tool_defs(allow_edits: bool, has_approver: bool) -> Vec<Value> {
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
                "description": "Content search across repository files. The query is a regex, falling back to a literal match when it is not valid regex. Matching is smart-case: an all-lowercase query ignores case, a query with an uppercase letter is matched exactly. Results are grouped by file, with surrounding lines and a '>' on each matching line, so you usually do not need to read the file afterwards. At most 3 matches per file, so hits spread across the repository. A directory that does not exist is not an error: the whole repository is searched instead.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Text or regex to search for" },
                        "dir": { "type": "string", "description": "Repo-relative directory to search under (optional)" }
                    },
                    "required": ["query"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "find_files",
                "description": "Find files by name or glob, e.g. `chat.rs`, `*.rs`, `crates/*/src/**/*.rs`. A bare name matches at any depth. Use this before guessing a path, and whenever a read or list reports that a path does not exist.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string", "description": "File name or glob pattern" },
                        "dir": { "type": "string", "description": "Repo-relative directory to search under (optional)" }
                    },
                    "required": ["pattern"]
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
    if has_approver {
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "update_plan",
                "description": "Update the execution plan state. Accepts a list of step objects with `label` and `status` fields. Status must be one of: pending, in_progress, done, skipped, blocked. The plan is rendered as a progress strip in the UI.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "steps": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "label": { "type": "string", "description": "Short label for this step" },
                                    "status": { "type": "string", "description": "One of: pending, in_progress, done, skipped, blocked" }
                                },
                                "required": ["label", "status"]
                            },
                            "description": "All plan steps with their current statuses"
                        }
                    },
                    "required": ["steps"]
                }
            }
        }));
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "ask_user",
                "description": "Ask the user a structured question with a set of options. Use when you need a decision between alternatives, not for simple yes/no approval. The user can pick an option or write their own.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "header": { "type": "string", "description": "One-line title for the question (optional)" },
                        "question": { "type": "string", "description": "The question to ask" },
                        "options": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "2-4 short answer choices for the user (optional)"
                        }
                    },
                    "required": ["question"]
                }
            }
        }));
    }
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
    if has_approver && !allow_edits {
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "exit_plan_mode",
                "description": "Exit plan mode and switch to edit mode. Presents the current plan for user approval; if approved, edit_file and unapproved command execution become available.",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }));
    }
    tools.push(json!({
        "type": "function",
        "function": {
            "name": "run_command",
            "description": "Run a CLI command. Filesystem writes are restricted to the repository and temp directories, and secrets are dropped from the environment. Pass turbo:true for offline mode (no network). Pass yolo:true only when the user explicitly asks for unrestricted execution (requires extra confirmations). Returns stdout, stderr, and exit code.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The binary to run, e.g. `rg`, `cargo`, `npm`" },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Arguments to pass to the command"
                    },
                    "turbo": { "type": "boolean", "description": "Run without network access. Use when the user asks for turbo mode or wants to work offline." },
                    "yolo": { "type": "boolean", "description": "Run without any filesystem restrictions. Only use when the user explicitly asks for yolo mode; triggers extra confirmations." }
                },
                "required": ["command"]
            }
        }
    }));
    tools
}

/// Execute one tool call. Failures come back as plain text so the model can react.
#[allow(clippy::too_many_arguments)]
async fn exec_tool(
    repo_root: &Path,
    allow_edits: &mut bool,
    policy: &Policy,
    grants: &Grants,
    approver: Option<&UiSender>,
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
        "update_plan" => update_plan(
            ctx,
            args["steps"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|v| {
                            (
                                v["label"].as_str().unwrap_or("").to_string(),
                                v["status"].as_str(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
        ),
        "ask_user" => match str_arg("question").context("ask_user needs a `question`") {
            Ok(question) => {
                let options: Vec<String> = args["options"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                let header = str_arg("header").unwrap_or_default();
                ask_user(approver, &header, &question, &options).await
            }
            Err(e) => Err(e),
        },
        "exit_plan_mode" if !*allow_edits => exit_plan_mode(approver, ctx, allow_edits).await,
        "exit_plan_mode" => Err(anyhow::anyhow!(
            "already in edit mode; the plan has already been approved"
        )),
        "read_file" => match str_arg("path").context("read_file needs a `path`") {
            Ok(path) if !edits::exists_anywhere(repo_root, &path) => {
                return missing_path(repo_root, &path);
            }
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
        "list_files" => match missing_dir(repo_root, &str_arg("dir")) {
            Some(dir) => return missing_path(repo_root, &dir),
            None => {
                match resolve_dir(repo_root, policy, grants, approver, ctx, &str_arg("dir")).await {
                    Ok(base) => list_files(&ctx.probe, &base),
                    Err(e) => Err(e),
                }
            }
        },
        // A directory that does not exist widens the search rather than
        // failing it: the hits are usually what the model was after anyway.
        "search_files" => match str_arg("query").context("search_files needs a `query`") {
            Ok(query) => match missing_dir(repo_root, &str_arg("dir")) {
                Some(dir) => search_files(&ctx.probe, repo_root, policy, &query, repo_root)
                    .map(|hits| widened(&dir, hits)),
                None => {
                    match resolve_dir(repo_root, policy, grants, approver, ctx, &str_arg("dir"))
                        .await
                    {
                        Ok(base) => search_files(&ctx.probe, repo_root, policy, &query, &base),
                        Err(e) => Err(e),
                    }
                }
            },
            Err(e) => Err(e),
        },
        "find_files" => match str_arg("pattern").context("find_files needs a `pattern`") {
            Ok(pattern) => match missing_dir(repo_root, &str_arg("dir")) {
                Some(dir) => bash_tools::find(repo_root, repo_root, &pattern, MAX_FIND_HITS)
                    .map(|hits| widened(&dir, hits)),
                None => {
                    match resolve_dir(repo_root, policy, grants, approver, ctx, &str_arg("dir"))
                        .await
                    {
                        Ok(base) => bash_tools::find(repo_root, &base, &pattern, MAX_FIND_HITS),
                        Err(e) => Err(e),
                    }
                }
            },
            Err(e) => Err(e),
        },
        "edit_file" if !*allow_edits => Err(anyhow::anyhow!(
            "editing is disabled for this chat; tell the user to enable Allow edits"
        )),
        "edit_file" => edit_file(repo_root, policy, approver, &args, edited)
            .await
            .map(|done| match governing_instructions(ctx, &args) {
                // Nested instructions are advertised, not preloaded, so an edit
                // is the last point at which the rules for that directory can
                // still be raised.
                Some(path) => format!("{done}\n\n{path} sets the rules for this directory. Read it if you have not, and revisit this edit if it conflicts."),
                None => done,
            }),
        "run_command" => match str_arg("command").context("run_command needs a `command`") {
            Ok(cmd) => {
                let cmd_args: Vec<String> = args["args"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|v| v.as_str().unwrap_or("").to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                let turbo = args["turbo"].as_bool().unwrap_or(false);
                let yolo = args["yolo"].as_bool().unwrap_or(false) || ctx.yolo;
                run_command_tool(
                    repo_root,
                    policy,
                    approver,
                    &cmd,
                    &cmd_args,
                    turbo,
                    yolo,
                    ctx.limits.command_timeout_secs as u64,
                )
                .await
            }
            Err(e) => Err(e),
        },
        "aster_mcp" => mcp_bridge(policy, approver, ctx, arguments).await,
        other => Err(anyhow::anyhow!(
            "unknown tool: {other}. Available tools: {}",
            tool_names(*allow_edits, approver.is_some()).join(", ")
        )),
    };
    result.unwrap_or_else(|e| format!("error: {e:#}"))
}

fn tool_names(allow_edits: bool, has_approver: bool) -> Vec<String> {
    tool_defs(allow_edits, has_approver)
        .iter()
        .filter_map(|t| t["function"]["name"].as_str().map(str::to_string))
        .collect()
}

impl PlanStepStatus {
    fn parse(raw: Option<&str>) -> Result<Self> {
        match raw.unwrap_or("pending") {
            "pending" => Ok(Self::Pending),
            "in_progress" => Ok(Self::InProgress),
            "done" => Ok(Self::Done),
            "skipped" => Ok(Self::Skipped),
            "blocked" => Ok(Self::Blocked),
            other => Err(anyhow::anyhow!(
                "unknown step status `{other}`; use pending, in_progress, done, skipped, or blocked"
            )),
        }
    }

    /// Matches the TUI's glyphs so every surface shows the same plan.
    fn glyph(self) -> &'static str {
        match self {
            Self::Pending => "◻",
            Self::InProgress => "◼",
            Self::Done => "✔",
            Self::Skipped => "⊘",
            Self::Blocked => "✖",
        }
    }
}

impl PlanState {
    fn count(&self, want: PlanStepStatus) -> usize {
        self.steps.iter().filter(|s| s.status == want).count()
    }

    /// A count line over one row per step. Statuses nobody used stay off the
    /// summary, so the common case reads as "3 done, 1 open".
    fn render(&self) -> String {
        let mut parts = vec![format!("{} done", self.count(PlanStepStatus::Done))];
        if self.count(PlanStepStatus::InProgress) > 0 {
            parts.push(format!(
                "{} in progress",
                self.count(PlanStepStatus::InProgress)
            ));
        }
        parts.push(format!("{} open", self.count(PlanStepStatus::Pending)));
        for (status, label) in [
            (PlanStepStatus::Blocked, "blocked"),
            (PlanStepStatus::Skipped, "skipped"),
        ] {
            if self.count(status) > 0 {
                parts.push(format!("{} {label}", self.count(status)));
            }
        }

        let head = format!(
            "{} task{} ({})",
            self.steps.len(),
            if self.steps.len() == 1 { "" } else { "s" },
            parts.join(", ")
        );
        let rows = self
            .steps
            .iter()
            .map(|step| format!("  {} {}", step.status.glyph(), step.label))
            .collect::<Vec<_>>()
            .join("\n");
        format!("{head}\n{rows}")
    }
}

/// Replace the plan wholesale. The model resends every step each call, so the
/// tool is a set rather than a patch and the UI never shows a stale strip.
fn update_plan(ctx: &SessionCtx, steps: Vec<(String, Option<&str>)>) -> Result<String> {
    if steps.is_empty() {
        return Err(anyhow::anyhow!(
            "update_plan needs a non-empty `steps` list"
        ));
    }
    let parsed = steps
        .into_iter()
        .map(|(label, status)| {
            let label = label.trim();
            match label.is_empty() {
                true => Err(anyhow::anyhow!("every plan step needs a `label`")),
                false => Ok(PlanStep {
                    label: label.to_string(),
                    status: PlanStepStatus::parse(status)?,
                }),
            }
        })
        .collect::<Result<Vec<_>>>()?;

    let mut plan = ctx
        .plan
        .lock()
        .map_err(|_| anyhow::anyhow!("plan state lock poisoned"))?;
    plan.steps = parsed;
    Ok(format!("plan updated:\n{}", plan.render()))
}

/// Put a question to the user and wait for the answer. Headless callers have no
/// UI channel, so the agent is told to decide for itself rather than blocking.
async fn ask_user(
    approver: Option<&UiSender>,
    header: &str,
    question: &str,
    options: &[String],
) -> Result<String> {
    let Some(tx) = approver else {
        return Ok(
            "note: no interactive UI is attached, so the user cannot be asked. Pick the most reasonable option and say which you chose."
                .to_string(),
        );
    };
    // With zero or one choices there is nothing for the user to pick. Rather
    // than bouncing the decision back to the model (which tends to retry the
    // tool in a loop), commit to the single option or decline outright.
    match options.len() {
        0 => {
            return Ok(
                "the user declined to answer; proceed with your best judgement"
                    .to_string(),
            )
        }
        1 => return Ok(format!("the user chose: {}", options[0])),
        _ => {} // fall through to the interactive path
    }

    let (respond, rx) = oneshot::channel();
    let request = UiRequest::Question(QuestionRequest {
        header: match header.trim().is_empty() {
            true => "Question".to_string(),
            false => header.trim().to_string(),
        },
        question: question.to_string(),
        options: options.to_vec(),
        respond,
    });
    if tx.send(request).await.is_err() {
        return Err(anyhow::anyhow!("the UI closed before the question was put"));
    }
    match rx.await.unwrap_or(None) {
        Some(answer) => Ok(format!("the user chose: {answer}")),
        None => Ok("the user declined to answer; proceed with your best judgement".to_string()),
    }
}

/// Present the plan for approval. Approval promotes the rest of the turn to edit
/// mode, which is what unlocks `edit_file` and unapproved commands.
async fn exit_plan_mode(
    approver: Option<&UiSender>,
    ctx: &SessionCtx,
    allow_edits: &mut bool,
) -> Result<String> {
    let plan = ctx
        .plan
        .lock()
        .map_err(|_| anyhow::anyhow!("plan state lock poisoned"))?
        .clone();
    if plan.steps.is_empty() {
        return Err(anyhow::anyhow!(
            "build a plan with update_plan before leaving plan mode"
        ));
    }

    let preview = format!("Approve this plan and start editing?\n\n{}", plan.render());
    if !request_plan_approval(approver, preview).await.allowed() {
        return Ok(
            "the user did not approve the plan; stay in plan mode and revise it".to_string(),
        );
    }
    *allow_edits = true;
    Ok("plan approved; edit mode is now active".to_string())
}

/// The requested directory when it does not exist, so a caller can widen or
/// hint instead of failing.
fn missing_dir(repo_root: &Path, dir: &Option<String>) -> Option<String> {
    let dir = dir.as_deref().map(str::trim).filter(|d| !d.is_empty())?;
    (!edits::exists_anywhere(repo_root, dir)).then(|| dir.to_string())
}

fn widened(dir: &str, hits: String) -> String {
    format!("note: {dir} does not exist, so the whole repository was searched instead.\n\n{hits}")
}

/// A wrong path is a bad guess, not a failure. Name the nearest real paths so
/// the model corrects itself instead of guessing again.
fn missing_path(repo_root: &Path, path: &str) -> String {
    let nearby = bash_tools::suggest(repo_root, path, MAX_PATH_SUGGESTIONS);
    if nearby.is_empty() {
        return format!(
            "note: {path} does not exist. Call find_files with a name or glob to locate it."
        );
    }
    format!(
        "note: {path} does not exist. Nearest paths in the repository:\n{}\n\nCall find_files if none of these are the one.",
        nearby
            .iter()
            .map(|p| format!("  {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

/// Run a command, subject to policy and approval.
/// The nested instruction file governing the path an edit just touched.
fn governing_instructions(ctx: &SessionCtx, args: &Value) -> Option<String> {
    let path = args["path"].as_str()?;
    ctx.instructions
        .nearest(Path::new(path))
        .map(|p| p.display().to_string())
}

/// Route one `aster_mcp` call. Discovery answers from the local catalog;
/// execution is authorized against the resolved `server/tool` id, never
/// against the bridge, so allowing the bridge never allows every tool behind it.
async fn mcp_bridge(
    policy: &Policy,
    approver: Option<&UiSender>,
    ctx: &SessionCtx,
    arguments: &str,
) -> Result<String> {
    let runtime = ctx
        .mcp
        .as_ref()
        .context("no MCP servers are connected in this session")?;
    let action = runtime.injector().route(arguments)?;
    let (tool, call_args) = match action {
        aster_mcp::BridgeAction::Search(matches) => {
            return Ok(serde_json::to_string_pretty(
                &json!({ "matches": matches }),
            )?);
        }
        aster_mcp::BridgeAction::Describe(tool) => {
            return Ok(serde_json::to_string_pretty(&json!({
                "id": tool.id(),
                "description": tool.description,
                "input_schema": tool.input_schema,
            }))?);
        }
        aster_mcp::BridgeAction::Execute { tool, arguments } => (tool, arguments),
    };

    let id = tool.id();
    match policy.evaluate(&Action::Exec {
        binary: "mcp",
        args: &[&id],
    }) {
        Decision::Allow => {}
        Decision::Deny { reason } => bail!("{reason}"),
        Decision::Prompt { .. } => {
            let preview = format!("call MCP tool {id}:\n{call_args:#}");
            if !request_approval(approver, preview, None).await.allowed() {
                bail!(
                    "MCP tool `{id}` needs user approval; it was rejected or this run cannot ask"
                );
            }
        }
    }

    let result = runtime.call(&tool, &call_args).await?;
    Ok(crate::mcp::render_result(&result))
}

#[allow(clippy::too_many_arguments)]
async fn run_command_tool(
    repo_root: &Path,
    policy: &Policy,
    approver: Option<&UiSender>,
    binary: &str,
    args: &[String],
    turbo: bool,
    yolo: bool,
    timeout_secs: u64,
) -> Result<String> {
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    match policy.evaluate(&Action::Exec {
        binary,
        args: &arg_refs,
    }) {
        Decision::Allow => {}
        Decision::Deny { reason } => bail!("{reason}"),
        Decision::Prompt { preview } => {
            if !request_approval(approver, preview, None).await.allowed() {
                bail!(
                    "command `{binary}` needs user approval; it was rejected or this run cannot ask"
                );
            }
        }
    }

    if yolo {
        for i in 1..=3 {
            let preview =
                format!("YOLO mode — run `{binary}` with no sandbox?\n\nConfirmation {i}/3");
            if !request_approval(approver, preview, None).await.allowed() {
                bail!("yolo mode rejected at confirmation {i}/3");
            }
        }
        return run_direct(repo_root, binary, args).await;
    }

    let profile = aster_sandbox::SandboxProfile::new(repo_root)
        .timeout(timeout_secs)
        .network(!turbo);
    let config = aster_sandbox::SandboxConfig::new(profile);
    let output = aster_sandbox::run_command(&config, binary, args).await?;
    let mut result = String::new();
    if !output.stdout.is_empty() {
        result.push_str("stdout:\n");
        result.push_str(&output.stdout);
    }
    if !output.stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str("stderr:\n");
        result.push_str(&output.stderr);
    }
    result.push_str(&format!("\nexit code: {}", output.exit_code.unwrap_or(-1)));
    if result.is_empty() {
        return Ok("(no output)".into());
    }
    Ok(result)
}

async fn run_direct(repo_root: &Path, binary: &str, args: &[String]) -> Result<String> {
    let output = tokio::process::Command::new(binary)
        .args(args)
        .current_dir(repo_root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .context("running command")?;
    let mut result = String::new();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stdout.is_empty() {
        result.push_str("stdout:\n");
        result.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str("stderr:\n");
        result.push_str(&stderr);
    }
    result.push_str(&format!(
        "\nexit code: {}",
        output.status.code().unwrap_or(-1)
    ));
    if result.is_empty() {
        return Ok("(no output)".into());
    }
    Ok(result)
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
    approver: Option<&UiSender>,
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
    approver: Option<&UiSender>,
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
    let hits = bash_tools::search(probe, repo_root, base, query, MAX_SEARCH_HITS)?;
    let filtered: Vec<bash_tools::Hit> = hits
        .into_iter()
        .filter(|hit| {
            !matches!(
                policy.evaluate(&Action::Read { path: &hit.path }),
                Decision::Deny { .. }
            )
        })
        .collect();
    Ok(bash_tools::render(
        repo_root,
        &filtered,
        SEARCH_CONTEXT_LINES,
    ))
}

async fn edit_file(
    repo_root: &Path,
    policy: &Policy,
    approver: Option<&UiSender>,
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
    approver: Option<&UiSender>,
    preview: String,
    scope: Option<PathBuf>,
) -> Answer {
    ask_approval(approver, preview, scope, UiRequest::Approval).await
}

/// As [`request_approval`], but tagged so the front-end promotes its own mode.
async fn request_plan_approval(approver: Option<&UiSender>, preview: String) -> Answer {
    ask_approval(approver, preview, None, UiRequest::PlanApproval).await
}

async fn ask_approval(
    approver: Option<&UiSender>,
    preview: String,
    scope: Option<PathBuf>,
    wrap: fn(ApprovalRequest) -> UiRequest,
) -> Answer {
    let Some(tx) = approver else {
        return Answer::No;
    };
    let (respond, rx) = oneshot::channel();
    let request = wrap(ApprovalRequest {
        preview,
        scope,
        respond,
    });
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

    #[test]
    fn limits_come_from_the_agent_block() {
        let agent = crate::settings::Agent {
            max_tool_rounds: Some(9),
            command_timeout_secs: Some(11),
        };
        let limits = Limits::resolve(&agent);
        assert_eq!(limits.max_tool_rounds, 9);
        assert_eq!(limits.command_timeout_secs, 11);
    }

    #[test]
    fn limits_default_to_room_for_real_work() {
        let limits = Limits::default();
        assert!(limits.max_tool_rounds >= 60);
        assert!(limits.command_timeout_secs >= 120);
    }

    /// A question with nothing to pick used to be shown and silently declined.
    #[tokio::test]
    async fn a_question_without_options_asks_the_model_for_some() {
        let (tx, _rx) = mpsc::channel::<UiRequest>(1);
        let result = ask_user(Some(&tx), "", "which one?", &[]).await.unwrap();
        assert!(result.contains("2-4"), "{result}");
    }

    fn args(path: &str, search: Option<&str>, replace: &str) -> Value {
        match search {
            Some(s) => json!({ "path": path, "search": s, "replace": replace }),
            None => json!({ "path": path, "replace": replace }),
        }
    }

    /// Unwraps the approval these tests expect; a question here is a bug.
    fn approval(req: UiRequest) -> ApprovalRequest {
        match req {
            UiRequest::Approval(req) | UiRequest::PlanApproval(req) => req,
            UiRequest::Question(_) => panic!("expected an approval, got a question"),
        }
    }

    async fn run_tool(repo: &Path, name: &str, arguments: Value) -> String {
        exec_tool(
            repo,
            &mut false,
            &Policy::permissive(),
            &Grants::default(),
            None,
            name,
            &arguments.to_string(),
            &mut Vec::new(),
            &SessionCtx::default(),
        )
        .await
    }

    /// Runs a tool against a shared ctx and mutable edit gate, for the plan
    /// tools whose whole point is the state they leave behind.
    async fn run_tool_with(
        repo: &Path,
        allow_edits: &mut bool,
        approver: Option<&UiSender>,
        ctx: &SessionCtx,
        name: &str,
        arguments: Value,
    ) -> String {
        exec_tool(
            repo,
            allow_edits,
            &Policy::permissive(),
            &Grants::default(),
            approver,
            name,
            &arguments.to_string(),
            &mut Vec::new(),
            ctx,
        )
        .await
    }

    fn steps(pairs: &[(&str, &str)]) -> Value {
        json!({
            "steps": pairs
                .iter()
                .map(|(label, status)| json!({ "label": label, "status": status }))
                .collect::<Vec<_>>()
        })
    }

    #[tokio::test]
    async fn update_plan_stores_every_step_with_its_status() {
        let repo = tempfile::tempdir().unwrap();
        let ctx = SessionCtx::default();
        let out = run_tool_with(
            repo.path(),
            &mut false,
            None,
            &ctx,
            "update_plan",
            steps(&[("read the code", "done"), ("write the fix", "in_progress")]),
        )
        .await;

        assert!(out.contains("✔ read the code"), "{out}");
        assert!(out.contains("◼ write the fix"), "{out}");
        assert!(
            out.contains("2 tasks (1 done, 1 in progress, 0 open)"),
            "{out}"
        );
        assert_eq!(ctx.plan.lock().unwrap().steps.len(), 2);
    }

    #[tokio::test]
    async fn update_plan_replaces_rather_than_appends() {
        let repo = tempfile::tempdir().unwrap();
        let ctx = SessionCtx::default();
        for pairs in [
            &[("first", "pending")][..],
            &[("second", "pending"), ("third", "pending")][..],
        ] {
            run_tool_with(
                repo.path(),
                &mut false,
                None,
                &ctx,
                "update_plan",
                steps(pairs),
            )
            .await;
        }

        let plan = ctx.plan.lock().unwrap();
        assert_eq!(plan.steps.len(), 2, "the second call replaced the first");
        assert_eq!(plan.steps[0].label, "second");
    }

    #[tokio::test]
    async fn update_plan_rejects_an_unknown_status() {
        let repo = tempfile::tempdir().unwrap();
        let ctx = SessionCtx::default();
        let out = run_tool_with(
            repo.path(),
            &mut false,
            None,
            &ctx,
            "update_plan",
            steps(&[("do it", "almost")]),
        )
        .await;

        assert!(out.starts_with("error:"), "{out}");
        assert!(
            out.contains("in_progress"),
            "the error lists valid ones: {out}"
        );
        assert!(
            ctx.plan.lock().unwrap().steps.is_empty(),
            "nothing was stored"
        );
    }

    #[tokio::test]
    async fn update_plan_needs_at_least_one_step() {
        let repo = tempfile::tempdir().unwrap();
        let out = run_tool(repo.path(), "update_plan", json!({ "steps": [] })).await;
        assert!(out.starts_with("error:"), "{out}");
    }

    #[tokio::test]
    async fn exit_plan_mode_needs_a_plan_first() {
        let repo = tempfile::tempdir().unwrap();
        let out = run_tool(repo.path(), "exit_plan_mode", json!({})).await;
        assert!(out.contains("update_plan"), "{out}");
    }

    #[tokio::test]
    async fn approving_the_plan_unlocks_editing() {
        let repo = tempfile::tempdir().unwrap();
        let ctx = SessionCtx::default();
        let mut allow_edits = false;

        let (tx, mut rx) = mpsc::channel::<UiRequest>(1);
        let prompt = tokio::spawn(async move {
            let req = approval(rx.recv().await.unwrap());
            assert!(req.preview.contains("◻ ship it"), "{}", req.preview);
            let _ = req.respond.send(Answer::Yes);
        });

        run_tool_with(
            repo.path(),
            &mut allow_edits,
            None,
            &ctx,
            "update_plan",
            steps(&[("ship it", "pending")]),
        )
        .await;
        let out = run_tool_with(
            repo.path(),
            &mut allow_edits,
            Some(&tx),
            &ctx,
            "exit_plan_mode",
            json!({}),
        )
        .await;

        prompt.await.unwrap();
        assert!(out.contains("edit mode is now active"), "{out}");
        assert!(allow_edits, "approval promotes the turn to edit mode");
    }

    #[tokio::test]
    async fn rejecting_the_plan_leaves_editing_locked() {
        let repo = tempfile::tempdir().unwrap();
        let ctx = SessionCtx::default();
        let mut allow_edits = false;

        let (tx, mut rx) = mpsc::channel::<UiRequest>(1);
        let prompt = tokio::spawn(async move {
            let _ = approval(rx.recv().await.unwrap()).respond.send(Answer::No);
        });

        run_tool_with(
            repo.path(),
            &mut allow_edits,
            None,
            &ctx,
            "update_plan",
            steps(&[("ship it", "pending")]),
        )
        .await;
        let out = run_tool_with(
            repo.path(),
            &mut allow_edits,
            Some(&tx),
            &ctx,
            "exit_plan_mode",
            json!({}),
        )
        .await;

        prompt.await.unwrap();
        assert!(out.contains("stay in plan mode"), "{out}");
        assert!(!allow_edits);
    }

    #[tokio::test]
    async fn exit_plan_mode_is_refused_once_already_editing() {
        let repo = tempfile::tempdir().unwrap();
        let out = run_tool_with(
            repo.path(),
            &mut true,
            None,
            &SessionCtx::default(),
            "exit_plan_mode",
            json!({}),
        )
        .await;
        assert!(out.contains("already in edit mode"), "{out}");
    }

    #[tokio::test]
    async fn ask_user_relays_the_chosen_option() {
        let repo = tempfile::tempdir().unwrap();
        let (tx, mut rx) = mpsc::channel::<UiRequest>(1);
        let prompt = tokio::spawn(async move {
            let UiRequest::Question(req) = rx.recv().await.unwrap() else {
                panic!("expected a question");
            };
            assert_eq!(req.header, "Storage");
            assert_eq!(req.options, ["sqlite", "postgres"]);
            let _ = req.respond.send(Some("postgres".to_string()));
        });

        let out = run_tool_with(
            repo.path(),
            &mut false,
            Some(&tx),
            &SessionCtx::default(),
            "ask_user",
            json!({
                "header": "Storage",
                "question": "Which database?",
                "options": ["sqlite", "postgres"]
            }),
        )
        .await;

        prompt.await.unwrap();
        assert!(out.contains("postgres"), "{out}");
    }

    #[tokio::test]
    async fn ask_user_tells_the_agent_to_decide_when_headless() {
        let repo = tempfile::tempdir().unwrap();
        let out = run_tool(
            repo.path(),
            "ask_user",
            json!({ "question": "Which database?", "options": ["sqlite"] }),
        )
        .await;

        assert!(
            !out.starts_with("error:"),
            "a missing UI is not an error: {out}"
        );
        assert!(out.contains("no interactive UI"), "{out}");
    }

    #[tokio::test]
    async fn a_declined_question_does_not_stall_the_turn() {
        let repo = tempfile::tempdir().unwrap();
        let (tx, mut rx) = mpsc::channel::<UiRequest>(1);
        // Dropping the responder is how a dismissed picker answers.
        let prompt = tokio::spawn(async move { drop(rx.recv().await.unwrap()) });

        let out = run_tool_with(
            repo.path(),
            &mut false,
            Some(&tx),
            &SessionCtx::default(),
            "ask_user",
            json!({ "question": "Which database?", "options": ["sqlite"] }),
        )
        .await;

        prompt.await.unwrap();
        assert!(out.contains("declined"), "{out}");
    }

    fn sample_repo() -> tempfile::TempDir {
        let repo = tempfile::tempdir().unwrap();
        fs::create_dir_all(repo.path().join("crates/aster-cli/src/tui")).unwrap();
        fs::write(
            repo.path().join("crates/aster-cli/src/tui/composer.rs"),
            "fn compose() {}\n",
        )
        .unwrap();
        repo
    }

    #[tokio::test]
    async fn a_missing_read_path_suggests_real_ones_instead_of_failing() {
        let repo = sample_repo();

        let out = run_tool(
            repo.path(),
            "read_file",
            json!({ "path": "crates/ui/src/composer.rs" }),
        )
        .await;

        assert!(!out.starts_with("error: "), "{out}");
        assert!(
            out.contains("crates/aster-cli/src/tui/composer.rs"),
            "{out}"
        );
    }

    #[tokio::test]
    async fn a_missing_search_dir_widens_to_the_whole_repo() {
        let repo = sample_repo();

        let out = run_tool(
            repo.path(),
            "search_files",
            json!({ "query": "compose", "dir": "crates/aster-tui" }),
        )
        .await;

        assert!(
            out.starts_with("note: crates/aster-tui does not exist"),
            "{out}"
        );
        assert!(out.contains("composer.rs"), "{out}");
    }

    #[tokio::test]
    async fn find_files_locates_a_file_by_name() {
        let repo = sample_repo();

        let out = run_tool(
            repo.path(),
            "find_files",
            json!({ "pattern": "composer.rs" }),
        )
        .await;

        assert_eq!(out, "crates/aster-cli/src/tui/composer.rs");
    }

    #[tokio::test]
    async fn an_unknown_tool_names_the_real_ones() {
        let repo = tempfile::tempdir().unwrap();

        let out = run_tool(repo.path(), "search_file", json!({ "query": "x" })).await;

        assert!(out.starts_with("error: unknown tool: search_file"), "{out}");
        assert!(out.contains("search_files"), "{out}");
        assert!(out.contains("find_files"), "{out}");
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

        let (tx, mut rx) = mpsc::channel::<UiRequest>(1);
        let answer = tokio::spawn(async move {
            let req = approval(rx.recv().await.unwrap());
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

        let (tx, mut rx) = mpsc::channel::<UiRequest>(1);
        let prompts = tokio::spawn(async move {
            let mut seen = 0;
            while let Some(req) = rx.recv().await {
                seen += 1;
                let _ = approval(req).respond.send(Answer::Yes);
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
