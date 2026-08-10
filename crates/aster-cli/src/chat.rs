//! Bare `aster`: a conversational turn with an agentic read/list/search/edit tool loop.

use std::collections::{HashMap, HashSet};
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::{env, fs, io};

use anyhow::{Context, Result, bail};
use aster_ai::{AiClient, Annotation, ChatMessage, DegenerateOutput, UsageSnapshot};
use aster_persist::{
    EventUsage, EvictionEvent, MessageEvent, Store, SummaryEvent, TranscriptEvent,
};
use aster_policy::{Action, Decision, Grants, Policy};
use clap::Args;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tracing::Instrument;

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
    /// Lockfile-derived repo facts, rendered into the system prompt.
    pub environment: Option<String>,
    /// YOLO mode: the session is running without sandbox restrictions.
    pub yolo: bool,
    /// Credential directories approved per command this session. Kept apart
    /// from the file-read grants: approving `gh` must not widen `read_file`.
    pub credentials: Arc<aster_policy::CommandGrants>,
    /// Ranges already read this turn, keyed by path and range, with the file's
    /// modification time. A repeat read of an unchanged range is answered with
    /// a pointer instead of a second full copy in the history.
    pub reads: Arc<Mutex<HashMap<String, Option<std::time::SystemTime>>>>,
    /// Lookups already answered this turn, keyed by tool and arguments. A
    /// repeat is answered with a pointer instead of a second copy in the
    /// history. Anything that can change the tree clears it.
    pub lookups: Arc<Mutex<HashSet<String>>>,
    /// User messages sent while the turn runs, absorbed at the next round
    /// boundary instead of waiting out the whole turn.
    pub injected: Arc<std::sync::Mutex<Vec<String>>>,
    /// Agents discovered at startup, rendered into the system prompt.
    pub agents: Arc<aster_agents::AgentRegistry>,
    /// Non-None when this is a sub-agent session.  Shapes the system prompt,
    /// tool schema, and persistence.
    pub sub_agent: Option<Arc<SubAgentOverrides>>,
    /// Fan-out caps read from aster.yaml + env.
    pub swarm: SwarmLimits,
}

/// How long a turn may work before it has to answer, and how long one command
/// may run. Defaults suit real builds; `aster.yaml` and the env can lower them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    pub max_tool_rounds: usize,
    pub command_timeout_secs: usize,
    /// History size (chars) above which older turns are compacted. Lower it
    /// for small-context models.
    pub compact_budget_chars: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_tool_rounds: DEFAULT_MAX_TOOL_ROUNDS,
            command_timeout_secs: DEFAULT_COMMAND_TIMEOUT_SECS,
            compact_budget_chars: COMPACT_BUDGET_CHARS,
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
            compact_budget_chars: env_usize("ASTER_COMPACT_BUDGET")
                .or(agent.compact_budget_chars)
                .unwrap_or(COMPACT_BUDGET_CHARS)
                .max(COMPACT_KEEP_TAIL * 1_000),
        }
    }
}

/// Caps on the sub-agent fan-out.  aster.yaml first, then the environment.
#[derive(Debug, Clone)]
pub(crate) struct SwarmLimits {
    pub max_concurrent: usize,
    pub max_per_turn: usize,
    pub agent_timeout_secs: u64,
    pub collector_model: Option<String>,
}

impl SwarmLimits {
    pub(crate) fn resolve(agents: &crate::settings::Agents) -> Self {
        let env_usize = |key: &str| std::env::var(key).ok().and_then(|v| v.parse().ok());
        let env_u64 = |key: &str| std::env::var(key).ok().and_then(|v| v.parse().ok());
        Self {
            max_concurrent: env_usize("ASTER_AGENT_MAX_CONCURRENT")
                .or(agents.max_concurrent)
                .unwrap_or(8)
                .max(1),
            max_per_turn: env_usize("ASTER_AGENT_MAX_PER_TURN")
                .or(agents.max_per_turn)
                .unwrap_or(24)
                .max(1),
            agent_timeout_secs: env_u64("ASTER_AGENT_TIMEOUT")
                .or(agents.agent_timeout_secs)
                .unwrap_or(300)
                .max(1),
            collector_model: std::env::var("ASTER_COLLECTOR_MODEL")
                .ok()
                .or_else(|| agents.collector_model.clone()),
        }
    }
}

impl Default for SwarmLimits {
    fn default() -> Self {
        Self {
            max_concurrent: 8,
            max_per_turn: 24,
            agent_timeout_secs: 300,
            collector_model: None,
        }
    }
}

/// Overrides applied to a sub-agent's session so it runs as a child.
#[derive(Debug, Clone)]
pub(crate) struct SubAgentOverrides {
    pub prompt_body: String,
    pub tool_allowlist: std::collections::HashSet<String>,
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

/// The plan as comparable data; `None` when empty or the lock is poisoned.
fn plan_snapshot(ctx: &SessionCtx) -> Option<Vec<(String, PlanStepStatus)>> {
    let plan = ctx.plan.lock().ok()?;
    (!plan.steps.is_empty()).then(|| {
        plan.steps
            .iter()
            .map(|s| (s.label.clone(), s.status))
            .collect()
    })
}

fn plan_unfinished(snapshot: &Option<Vec<(String, PlanStepStatus)>>) -> bool {
    snapshot.as_ref().is_some_and(|steps| {
        steps.iter().any(|(_, status)| {
            matches!(status, PlanStepStatus::Pending | PlanStepStatus::InProgress)
        })
    })
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

    pub(crate) fn record_summary(&self, content: &str, replaces_through: usize) {
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

    fn record_eviction(&self, eviction: &crate::budget::Eviction) {
        let Some(recorder) = &self.recorder else {
            return;
        };
        if let Ok(mut writer) = recorder.lock()
            && let Err(e) = writer.append(&TranscriptEvent::Eviction(EvictionEvent::new(
                eviction.reason,
                eviction.role,
                eviction.index,
                eviction.chars,
            )))
        {
            tracing::warn!("failed to record eviction event: {e:#}");
        }
    }

    /// True once this session carries a generated name, so a resumed session
    /// keeps the one it already has.
    fn is_titled(&self) -> bool {
        self.recorder
            .as_ref()
            .and_then(|r| r.lock().ok())
            .is_some_and(|w| w.title().is_some())
    }

    fn record_title(&self, title: &str) {
        let Some(recorder) = &self.recorder else {
            return;
        };
        if let Ok(mut writer) = recorder.lock()
            && let Err(e) = writer.set_title(title)
        {
            tracing::warn!("failed to record title event: {e:#}");
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
/// Installed plugins contribute theirs next, and built-ins last, so a skills
/// root shadows a plugin and a plugin shadows a built-in.
pub(crate) fn discover_skills(repo_root: &Path) -> Arc<aster_skills::SkillSet> {
    let mut roots = vec![repo_root.join(".aster").join("skills")];
    match crate::persist::home() {
        Ok(home) => roots.push(home.join("skills")),
        Err(e) => tracing::debug!("no global skills root: {e:#}"),
    }
    let (plugins, problems) = crate::plugins::installed(Some(repo_root));
    crate::plugins::report(&plugins, &problems);
    Arc::new(
        aster_skills::SkillSet::discover(&roots)
            .extend_dirs(&crate::plugins::skill_dirs(&plugins))
            .with_builtins(),
    )
}

/// Session-start snapshot: platform, date, git state, and which package
/// manager each lockfile pins. Taken once, so the model starts a turn knowing
/// what a round of discovery commands would have told it.
pub(crate) fn environment_note(repo_root: &Path) -> Option<String> {
    let mut note = format!(
        "## Environment\n- Platform: {} ({})\n- Today's date: {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
        chrono::Local::now().format("%Y-%m-%d")
    );
    if let Some(git) = git_snapshot(repo_root) {
        note.push_str(&git);
    }
    if let Some(pm) = package_manager_note(repo_root) {
        note.push_str(&pm);
    }
    if let Some(runners) = task_runner_note(repo_root) {
        note.push_str(&runners);
    }
    Some(note)
}

/// How many package.json script names the snapshot lists.
const MAX_SCRIPT_NAMES: usize = 12;

/// The project's own verbs: task-runner files and script names, so the model
/// reaches for `just build` or `bun run check` instead of hand-rolling the
/// pipeline those already encode.
fn task_runner_note(repo_root: &Path) -> Option<String> {
    let mut note = String::new();
    // One candidate list per runner: a case-insensitive filesystem would
    // otherwise report Justfile and justfile as two files.
    let runners: [(&[&str], &str); 3] = [
        (
            &["Justfile", "justfile"],
            "run recipes with `just <name>`; `just --list` shows them",
        ),
        (&["Makefile"], "run targets with `make <name>`"),
        (
            &["Taskfile.yml"],
            "run tasks with `task <name>`; `task --list` shows them",
        ),
    ];
    for (candidates, hint) in runners {
        if let Some(file) = candidates.iter().find(|f| repo_root.join(f).is_file()) {
            note.push_str(&format!("- {file} present: {hint}.\n"));
        }
    }
    if let Some(scripts) = package_scripts(&repo_root.join("package.json")) {
        note.push_str(&format!(
            "- package.json scripts: {}. Prefer these over hand-rolled equivalents.\n",
            scripts.join(", ")
        ));
    }
    (!note.is_empty()).then_some(note)
}

/// Script names from a `package.json`, bounded, alphabetical.
fn package_scripts(manifest: &Path) -> Option<Vec<String>> {
    let raw = fs::read_to_string(manifest).ok()?;
    let json: Value = serde_json::from_str(&raw).ok()?;
    let scripts = json.get("scripts")?.as_object()?;
    if scripts.is_empty() {
        return None;
    }
    let mut names: Vec<String> = scripts.keys().take(MAX_SCRIPT_NAMES).cloned().collect();
    if scripts.len() > MAX_SCRIPT_NAMES {
        names.push(format!("... {} more", scripts.len() - MAX_SCRIPT_NAMES));
    }
    Some(names)
}

/// How many changed files the git snapshot lists before summarizing the rest.
const GIT_STATUS_LINES: usize = 15;

/// Branch, working-tree status, and recent commits, labelled as a snapshot so
/// a later turn does not treat it as live. `None` outside a git repository.
fn git_snapshot(repo_root: &Path) -> Option<String> {
    let git = |args: &[&str]| -> Option<String> {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(args)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"])?;
    let mut note = format!("- Git branch: {branch}");
    if let Some(default) = git(&["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        .as_deref()
        .and_then(|head| head.rsplit('/').next())
    {
        note.push_str(&format!(" (default branch: {default})"));
    }
    note.push('\n');
    match git(&["status", "--porcelain"]).as_deref() {
        Some("") => note.push_str("- Working tree clean at session start\n"),
        Some(status) => {
            let lines: Vec<&str> = status.lines().collect();
            note.push_str(&format!(
                "- Changed files at session start ({}):\n",
                lines.len()
            ));
            for line in lines.iter().take(GIT_STATUS_LINES) {
                note.push_str(&format!("  {line}\n"));
            }
            if lines.len() > GIT_STATUS_LINES {
                note.push_str(&format!(
                    "  ... and {} more\n",
                    lines.len() - GIT_STATUS_LINES
                ));
            }
        }
        None => {}
    }
    if let Some(log) = git(&["log", "--oneline", "-5"]).filter(|log| !log.is_empty()) {
        note.push_str("- Recent commits:\n");
        for line in log.lines() {
            note.push_str(&format!("  {line}\n"));
        }
    }
    Some(note)
}

/// Which JavaScript package manager each lockfile pins, so the model runs
/// `bun`/`pnpm`/`yarn` where the repo does instead of defaulting to npm.
fn package_manager_note(repo_root: &Path) -> Option<String> {
    const LOCKS: &[(&str, &str)] = &[
        ("bun.lock", "bun"),
        ("bun.lockb", "bun"),
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("package-lock.json", "npm"),
    ];
    let mut found: std::collections::BTreeMap<&str, Vec<String>> = Default::default();
    let walk = ignore::WalkBuilder::new(repo_root)
        .max_depth(Some(3))
        .build();
    for entry in walk.flatten() {
        let Some(name) = entry.file_name().to_str() else {
            continue;
        };
        let Some((_, pm)) = LOCKS.iter().find(|(lock, _)| *lock == name) else {
            continue;
        };
        let dir = entry
            .path()
            .parent()
            .and_then(|p| p.strip_prefix(repo_root).ok())
            .map(|p| p.display().to_string())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| ".".to_string());
        let dirs = found.entry(pm).or_default();
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    if found.is_empty() {
        return None;
    }
    let mut note = String::new();
    for (pm, dirs) in &found {
        note.push_str(&format!(
            "- JavaScript packages in {} use `{pm}`; run scripts and one-off tools with it, not npm/npx.\n",
            dirs.join(", ")
        ));
    }
    note.push_str("- Run package commands from the directory that owns the lockfile.");
    Some(note)
}

/// The agent persona, the repo's own instructions, and the memory block.
fn system_prompt(ctx: &SessionCtx, tools: bool) -> String {
    // Sub-agents get only their prompt body and an environment note; the
    // persona, instructions, memory, skills, and agent index are skipped.
    if let Some(sub) = &ctx.sub_agent {
        let mut prompt = sub.prompt_body.clone();
        if let Some(environment) = &ctx.environment {
            prompt.push_str("\n\n");
            prompt.push_str(environment);
        }
        return prompt;
    }
    let mut prompt = String::from(AGENT_SYSTEM_PROMPT);
    // Ahead of tools and memory: these are the repo's standing rules, and they
    // shape how every other section gets used.
    if let Some(project) = ctx.instructions.render() {
        prompt.push_str("\n\n");
        prompt.push_str(&project);
    }
    if let Some(environment) = &ctx.environment {
        prompt.push_str("\n\n");
        prompt.push_str(environment);
    }
    if tools {
        prompt.push_str(TOOLS_PROMPT);
        if let Some(index) = ctx.skills.render_index() {
            prompt.push_str("\n\n");
            prompt.push_str(&index);
        }
        if let Some(index) = ctx.agents.render_index() {
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
    /// Skip policy checks and isolation entirely. Use with extreme caution.
    Yolo,
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
            PermissionModeArg::Yolo => Self::Yolo,
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
/// Lines one open-ended `read_file` returns. A window with a resume hint beats
/// a whole file cut off mid-line, which costs a blind re-read.
const READ_WINDOW_LINES: usize = 600;
/// Caps each of a command's streams, so one noisy build cannot spend the whole
/// tool-result budget before the combined cap even applies.
const MAX_STREAM_CHARS: usize = 10_000;
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
/// Total history size (chars) above which older turns are folded into a
/// summary. Roughly 48k tokens: roomy for 128k-context models, since every
/// compaction costs a summarize round-trip and loses detail the agent re-reads.
const COMPACT_BUDGET_CHARS: usize = 192_000;
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
the environment. Use it for builds, tests, and linters. It can also reach \
the network: prefer `curl` (or a similar CLI) for fetching URLs and calling \
APIs before suggesting a browser-based tool. Do not shell out \
to `rg`, `grep`, `find`, or `fd`: `search_files` and `find_files` already \
run them directly, without the overhead. \
Tool rounds are the slow part of a turn: each one costs a full model \
round-trip, while the tools themselves are nearly instant. Work in as few \
rounds as the task allows:\n\
- Look things up with `explore`, not one call at a time. If you are about to \
send a single `read_file`, `search_files`, `find_files`, or `list_files`, \
first ask what else you will want once you see it, and send them together as \
`explore` steps. Two lookups in one `explore` are twice as fast as two \
rounds; ten are ten times.\n\
- Batch independent calls into one response: several reads, or a search and a \
find together, instead of one call per response.\n\
- Search before you read. `search_files` returns the matching lines with \
context, which usually answers the question without reading the file at all.\n\
- Never re-read what is already in this conversation. A file you read earlier \
is still above you; scroll back instead of calling the tool again.\n\
- When you do need more of a file, ask for the specific range you are missing \
rather than the whole file again.\n\
- Get everything one command can give you in a single call. `run_command` \
runs one binary directly, with no shell, so chain with \
`bash -lc \"git status --short; git log --oneline -5; git diff --stat\"` \
rather than spending a round on each. Bound noisy output with flags like \
`--stat`, `-n 20`, or a `| head` inside that `bash -lc` string.\n\
- Stop gathering as soon as you can act or answer: do not re-verify what you \
already read, and do not explore beyond what the task needs.\n\
When a user message contains `[@name]` tokens, each token's full path is \
listed beneath the message as `[@name]: /full/path`. Resolve the token from \
that list rather than guessing a path. \
Set `turbo: true` when the user asks to work offline or in turbo mode \
(blocks network access). Set `yolo: true` only when the user explicitly \
asks for yolo mode (no restrictions). \
Ground every claim about the code in what you actually read. Only edit files when the \
user asked for a change; keep edits minimal and in the file's existing style. \
After editing, state plainly which files you changed and what the change does. \
If `edit_file` is unavailable, say so and describe the change instead.";

#[derive(Args)]
pub struct ChatArgs {
    /// One-shot question, e.g. `aster "why is finding 2 critical?"`.
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

    #[command(flatten)]
    pub effort: crate::EffortArgs,
}

/// Args equivalent to `aster --resume <id>`, for commands that hand off into
/// a resumed chat.
pub fn resume_args(id: &str) -> ChatArgs {
    #[derive(clap::Parser)]
    struct Wrap {
        #[command(flatten)]
        chat: ChatArgs,
    }
    <Wrap as clap::Parser>::parse_from(["aster", "--resume", id]).chat
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

    // `yolo` means no policy *and* no sandbox; the TUI already treats it that
    // way, and a headless run must match or commands fail on writes.
    let yolo = permissions.mode == aster_policy::Mode::Yolo;
    let policy = Arc::new(Policy::compile(&permissions)?);
    let grants = Arc::new(configured_grants(&permissions, &repo_root));
    let credentials = Arc::new(configured_credentials(&permissions, &repo_root));

    let limits = Limits::resolve(&settings.agent);
    let swarm = SwarmLimits::resolve(&settings.agents);
    let agents = crate::agents::discover_agents(&repo_root);

    if args.is_interactive() {
        // Every server costs a process spawn, and `npx` ones a registry round
        // trip, so waiting here would leave the terminal blank for seconds.
        // The TUI draws first and adopts the tools when the connect lands.
        let mcp_settings = settings.mcp.clone();
        let mcp = tokio::spawn(async move { crate::mcp::McpRuntime::connect(&mcp_settings).await });
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
            swarm,
            agents,
        )
        .await;
    }

    let (mcp, mcp_problems) = crate::mcp::McpRuntime::connect(&settings.mcp).await;
    for problem in &mcp_problems {
        eprintln!("{}", console::style(format!("✗ {problem}")).red());
    }

    // A picker needs a terminal to draw in; naming the session is the way out.
    if matches!(args.resume_mode(), Resume::Pick) {
        anyhow::bail!(
            "--resume needs a terminal to show the session list. Pass an id instead: `aster sessions list`, then `aster --resume <ID>`"
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
            yolo,
            credentials,
        )
        .await;
    }

    let (ctx, history) = prepare_turn(&args, &repo_root, &client, mcp, limits, yolo, credentials)?;
    let titling = history.clone();

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
    name_session(&client, &ctx, &titling).await;

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
        if let Some(recorder) = &ctx.recorder
            && let Ok(writer) = recorder.lock()
        {
            eprintln!("Resume this session with: aster --resume {}", writer.id());
        }
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
    yolo: bool,
    credentials: Arc<aster_policy::CommandGrants>,
) -> Result<(SessionCtx, Vec<ChatMessage>)> {
    let new_turns = read_history(args)?;
    let store = crate::persist::store().ok();
    let (recorder, prior) =
        resolve_headless_session(store.as_ref(), repo_root, args, &client.model)?;
    let agents = crate::agents::discover_agents(repo_root);
    let swarm = SwarmLimits::default();
    let ctx = SessionCtx {
        recorder,
        store,
        credentials,
        skills: discover_skills(repo_root),
        instructions: Arc::new(crate::instructions::discover(repo_root)),
        probe: Arc::new(bash_tools::ToolProbe::detect()),
        plan: Default::default(),
        mcp,
        limits,
        environment: environment_note(repo_root),
        // Yolo is a mode, not just a per-call flag: asking for it once must
        // drop the sandbox too, otherwise commands still fail on writes.
        yolo,
        reads: Default::default(),
        lookups: Default::default(),
        injected: Default::default(),
        agents,
        sub_agent: None,
        swarm,
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

/// Emit a `citations` event carrying the web-search source URLs attached to
/// the assistant message. Consumed by the TUI and the `--stream` front-ends.
fn emit_citations(annotations: &[Annotation], emit: &impl Fn(Value)) {
    let sources: Vec<Value> = annotations
        .iter()
        .map(|a| {
            json!({
                "url": a.url_citation.url,
                "title": a.url_citation.title,
            })
        })
        .collect();
    emit(json!({ "type": "citations", "sources": sources }));
}

/// Read stdin forever, splitting lines by kind: `{"message"}` injections go
/// into the running turn's queue, everything else is a prompt reply.
fn spawn_stdin_router(injected: Arc<std::sync::Mutex<Vec<String>>>) -> mpsc::Receiver<Value> {
    let (tx, rx) = mpsc::channel::<Value>(4);
    std::thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            if io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if let Some(message) = value.get("message").and_then(Value::as_str) {
                if let Ok(mut queue) = injected.lock() {
                    queue.push(message.to_string());
                }
                continue;
            }
            if tx.blocking_send(value).is_err() {
                break;
            }
        }
    });
    rx
}

/// Bridge prompts to the caller: write an `approval_request` or `question`
/// line, then block on the next reply from the stdin router.
fn stdio_approver(mut replies: mpsc::Receiver<Value>) -> UiSender {
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
                    let answer = replies.recv().await.map_or(Answer::No, parse_approval);
                    let _ = a.respond.send(answer);
                }
                UiRequest::Question(q) => {
                    emit_line(&json!({
                        "type": "question",
                        "header": q.header,
                        "question": q.question,
                        "options": q.options,
                    }));
                    let answer = replies.recv().await.and_then(parse_question);
                    let _ = q.respond.send(answer);
                }
            }
        }
    });
    tx
}

/// A `{"choice": "string"}` reply, or `{"choice": null}` to skip.
fn parse_question(reply: Value) -> Option<String> {
    reply
        .get("choice")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// A `{"allow": bool}` reply, optionally with `"always": true` to persist the
/// request's scope. A closed pipe or junk denies.
fn parse_approval(reply: Value) -> Answer {
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
    yolo: bool,
    credentials: Arc<aster_policy::CommandGrants>,
) -> Result<()> {
    let (ctx, history) = prepare_turn(&args, &repo_root, &client, mcp, limits, yolo, credentials)?;
    // The router owns stdin from here: replies feed the approver, and typed-in
    // `{"message"}` lines join the turn at the next round boundary.
    let replies = spawn_stdin_router(ctx.injected.clone());

    let sink: ChatEventSink = Box::new(|event| emit_line(&event));
    let mut edited: Vec<String> = Vec::new();
    let result = agent_loop(
        &client,
        &repo_root,
        &history,
        allow_edits,
        &policy,
        &grants,
        Some(&stdio_approver(replies)),
        &mut edited,
        &ctx,
        Some(&sink),
    )
    .await;

    if result.is_ok()
        && let Some(title) = name_session(&client, &ctx, &history).await
    {
        emit_line(&json!({ "type": "title", "title": title }));
    }

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
    bail!("nothing to ask; pass a prompt (aster \"...\"), pipe one in, or use --messages-json")
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

    // `--session` names a session to append to and creates it when it is not
    // there yet. Only `--resume` insists the session already exists.
    if let Some(id) = &args.session {
        let prior = store
            .resume(repo_root, id)
            .map(|t| t.to_chat_messages())
            .unwrap_or_default();
        let writer = store.session_writer_for(repo_root, id, repo_root, Some(model.to_string()))?;
        return Ok((Some(recorder(writer)), prior));
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
    if let Some(title) = name_session(&client, &ctx, &history).await {
        events(json!({ "type": "title", "title": title }));
    }
    Ok((reply, edited, compacted))
}

/// Verdict for one tool round fed to [`NoProgress`].
enum RoundVerdict {
    /// Keep looping.
    Continue,
    /// The round repeated itself; inject the correction and keep going once.
    Correct,
    /// Already corrected and it looped again; abort the turn.
    Abort,
}

/// Message injected once when a tool loop is detected. It changes the prompt,
/// so a fixed-seed retry is not identical to the degenerate rounds.
const LOOP_CORRECTION: &str = "You repeated the same tool calls with the same \
    results three times in a row. Stop repeating. Re-read the results above and \
    do something different, or give your final answer.";

/// How many extra round allotments the plan-progress extension may grant. The
/// cap is a backstop, so it cannot stretch indefinitely.
const MAX_ROUND_EXTENSIONS: usize = 2;

/// Consecutive identical tool rounds, and consecutive all-error rounds, mean the
/// model is spinning: correct once, then abort instead of burning the round cap.
#[derive(Default)]
struct NoProgress {
    last_round: Option<u64>,
    identical_rounds: usize,
    error_rounds: usize,
    corrected: bool,
}

impl NoProgress {
    /// Feed one round's signature (hashed name/args/result) and whether every
    /// result was an error. Returns what the loop should do next.
    fn feed(&mut self, sig: u64, all_errors: bool) -> RoundVerdict {
        if self.last_round == Some(sig) {
            self.identical_rounds += 1;
        } else {
            self.last_round = Some(sig);
            self.identical_rounds = 1;
        }
        self.error_rounds = if all_errors { self.error_rounds + 1 } else { 0 };
        let looping = self.identical_rounds >= 3 || self.error_rounds >= 3;
        if !looping {
            return RoundVerdict::Continue;
        }
        if !self.corrected {
            self.corrected = true;
            self.identical_rounds = 0;
            self.error_rounds = 0;
            return RoundVerdict::Correct;
        }
        RoundVerdict::Abort
    }
}

/// Hash a round's (tool name, arguments, result) triples so identical rounds
/// compare equal. The call id is deliberately excluded: a model re-issues a
/// fresh id each round, so including it would hide a repeated round.
fn round_signature(round: &[(String, String, String)]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    for (name, args, result) in round {
        name.hash(&mut hasher);
        args.hash(&mut hasher);
        result.hash(&mut hasher);
    }
    hasher.finish()
}

/// Drive the model's tool calls until it answers in plain text or the round cap trips.
/// Tool failures return to the model as tool results so it can retry instead of dying.
#[allow(clippy::too_many_arguments)]
/// `rounds` and `calls` are the two numbers a slow turn is almost always made
/// of, so they are recorded on the span rather than left to be counted later.
#[tracing::instrument(
    name = "turn",
    skip_all,
    fields(rounds = tracing::field::Empty, calls = tracing::field::Empty)
)]
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
    let mut calls = 0usize;
    let turn_span = tracing::Span::current();
    let emit = |event: Value| {
        if let Some(sink) = events {
            sink(event);
        }
    };
    // Measured first so its reservation (persona, instructions, memory,
    // skills) comes off the top of the budget the history may spend.
    let system = system_prompt(ctx, true);
    let (history, compacted) = compact_if_needed(client, history, ctx, system.len()).await?;
    let mut wire: Vec<Value> = vec![json!({
        "role": "system",
        "content": system,
    })];
    let system_chars = wire[0]["content"].as_str().map_or(0, str::len);
    for m in &history {
        wire.push(serde_json::to_value(m)?);
    }

    // The round cap is a runaway backstop, not a work limit: while the plan
    // keeps moving, hitting it grants another allotment. A full allotment
    // with no plan progress is the runaway case, and the loop ends.
    let mut round_cap = ctx.limits.max_tool_rounds;
    let mut plan_at_extension = plan_snapshot(ctx);
    let mut extensions = 0usize;
    let mut no_progress = NoProgress::default();
    for round in 0.. {
        if round >= round_cap {
            let now = plan_snapshot(ctx);
            if extensions < MAX_ROUND_EXTENSIONS
                && plan_unfinished(&now)
                && now != plan_at_extension
            {
                tracing::debug!(
                    round,
                    "plan still in motion; extending the tool-round budget"
                );
                plan_at_extension = now;
                extensions += 1;
                round_cap += ctx.limits.max_tool_rounds;
            } else {
                break;
            }
        }
        turn_span.record("rounds", round + 1);
        // Messages the user sent mid-turn join here, before the next request.
        let pending: Vec<String> = match ctx.injected.lock() {
            Ok(mut queue) => queue.drain(..).collect(),
            Err(_) => Vec::new(),
        };
        for content in pending {
            emit(json!({ "type": "injected", "content": content }));
            ctx.record(MessageEvent::user(content.clone()));
            wire.push(json!({ "role": "user", "content": content }));
        }
        // Tool results accumulate inside a turn too; evict by policy before
        // each request and leave a trace of everything the model lost.
        let budget = crate::budget::history_budget(ctx.limits.compact_budget_chars, system_chars)
            + system_chars;
        for eviction in crate::budget::evict_tool_results(&mut wire, budget) {
            tracing::debug!(
                reason = eviction.reason,
                index = eviction.index,
                chars = eviction.chars,
                "evicted message to fit the context budget"
            );
            ctx.record_eviction(&eviction);
        }
        let mut tools = tool_defs(allow_edits, approver.is_some());
        // Sub-agents only see their allowlisted tools.
        if let Some(sub) = &ctx.sub_agent {
            tools.retain(|t| {
                t["function"]["name"]
                    .as_str()
                    .map(|n| sub.tool_allowlist.contains(n))
                    .unwrap_or(false)
            });
        }
        // Main session: push the agent tool when the registry is non-empty.
        if ctx.sub_agent.is_none() && !ctx.agents.is_empty() {
            tools.push(agent_tool_schema());
        }
        if let Some(injection) = ctx.mcp.as_ref().and_then(|m| m.injection()) {
            tools.push(injection.bridge_tool);
        }
        if client.web_search() {
            tools.push(json!({"type": "openrouter:web_search"}));
        }
        // False when the endpoint ignored `stream` and the client fell back to a
        // whole response, which the commentary emit below has to make up for.
        let mut streamed = false;
        // The client only exposes a cumulative counter, so this round's spend is
        // the delta across the call.
        let before = client.usage_snapshot();
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
                ctx.record(
                    MessageEvent::assistant(Some(reply.clone()), Vec::new())
                        .with_usage(round_usage(before, client.usage_snapshot())),
                );
                return Ok((reply, compacted));
            }
            Err(e) => return Err(e),
        };
        let usage = round_usage(before, client.usage_snapshot());

        if msg.tool_calls.is_empty() {
            let reply = msg
                .content
                .filter(|c| !c.trim().is_empty())
                .context("model returned an empty reply")?;
            if !msg.annotations.is_empty() {
                emit_citations(&msg.annotations, &emit);
                ctx.record(
                    MessageEvent::assistant(Some(reply.clone()), Vec::new())
                        .with_annotations(msg.annotations.clone())
                        .with_usage(usage),
                );
            } else {
                ctx.record(
                    MessageEvent::assistant(Some(reply.clone()), Vec::new()).with_usage(usage),
                );
            }
            return Ok((reply, compacted));
        }

        ctx.record(
            MessageEvent::assistant(msg.content.clone(), msg.tool_calls.clone())
                .with_annotations(msg.annotations.clone())
                .with_usage(usage),
        );
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
        }
        // A batch of pure reads runs on parallel threads. Any stateful call
        // keeps the whole round sequential so this-then-that ordering holds.
        let mut prefetched: Vec<Option<String>> = vec![None; msg.tool_calls.len()];
        // The reads in a batch fan out even when a command or edit sits beside
        // them; those still run in order afterwards.
        let reads: Vec<usize> = msg
            .tool_calls
            .iter()
            .enumerate()
            .filter(|(_, c)| PARALLEL_READ_TOOLS.contains(&c.function.name.as_str()))
            .map(|(i, _)| i)
            .collect();
        if reads.len() > 1 {
            // spawn_blocking fans the synchronous reads out on the blocking
            // pool, so this worker stays free to run other tasks meanwhile.
            let handles: Vec<_> = reads
                .iter()
                .map(|&i| {
                    let call = &msg.tool_calls[i];
                    let repo_root = repo_root.to_path_buf();
                    let policy = policy.clone();
                    let ctx = ctx.clone();
                    let name = call.function.name.clone();
                    let arguments = call.function.arguments.clone();
                    tokio::task::spawn_blocking(move || {
                        read_only_call(&repo_root, &policy, &ctx, &name, &arguments)
                    })
                })
                .collect();
            for (&i, handle) in reads.iter().zip(handles) {
                prefetched[i] = handle.await.unwrap_or_default();
            }
        }
        let mut round_sig = Vec::with_capacity(msg.tool_calls.len());
        let mut round_all_errors = true;
        for (call, prefetched) in msg.tool_calls.iter().zip(prefetched) {
            calls += 1;
            turn_span.record("calls", calls);
            let span = tracing::info_span!(
                "tool_call",
                tool = %call.function.name,
                round,
                cached = prefetched.is_some(),
                result_chars = tracing::field::Empty,
                barren = tracing::field::Empty,
                error = tracing::field::Empty,
            );
            let result = match prefetched {
                Some(result) => result,
                None => {
                    if call.function.name == "agent" {
                        dispatch_agent_tool(
                            repo_root,
                            client,
                            &call.function.arguments,
                            policy,
                            grants,
                            ctx,
                            &call.id,
                            events,
                        )
                        .instrument(span.clone())
                        .await
                    } else {
                        exec_tool(
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
                        .instrument(span.clone())
                        .await
                    }
                }
            };
            // Re-asking costs a round either way, but the answer is already
            // above; a pointer keeps the history from carrying it twice.
            let result = if is_repeat_lookup(ctx, &call.function.name, &call.function.arguments) {
                format!(
                    "[identical {} call earlier in this turn — scroll up for the result]",
                    call.function.name
                )
            } else {
                result
            };
            // The same rule aster-eval applies offline, so a live dashboard and
            // a session report never disagree about what counted as barren.
            span.record("result_chars", result.len());
            span.record("barren", aster_eval::barren(&result));
            span.record("error", result.starts_with("error: "));
            tracing::debug!(tool = %call.function.name, "tool call executed");
            let result = truncate(&result, MAX_TOOL_RESULT_CHARS);
            round_sig.push((
                call.function.name.clone(),
                call.function.arguments.clone(),
                result.clone(),
            ));
            if !result.starts_with("error: ") {
                round_all_errors = false;
            }
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
        match no_progress.feed(round_signature(&round_sig), round_all_errors) {
            RoundVerdict::Continue => {}
            RoundVerdict::Correct => {
                tracing::warn!("model looped on tool calls; injecting one correction");
                emit(json!({ "type": "injected", "content": LOOP_CORRECTION }));
                ctx.record(MessageEvent::user(LOOP_CORRECTION.to_string()));
                wire.push(json!({ "role": "user", "content": LOOP_CORRECTION }));
            }
            RoundVerdict::Abort => {
                bail!(
                    "the model kept repeating the same tool calls after being told to stop; ending the turn"
                );
            }
        }
    }

    // Round cap tripped with no plan progress: force a final plain answer out
    // of what was gathered. Logged rather than shown; the answer itself says
    // what was not finished.
    tracing::warn!(
        round_cap,
        "stopped after the tool-round cap with no plan progress; forcing a final answer"
    );
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

const TITLE_PROMPT: &str = "Name the conversation below. Reply with the name \
and nothing else: 3 to 6 words, sentence case, no quotes, no trailing period. \
Start with a plain verb and drop articles. Name what the user is trying to do, \
not what the assistant did, and be concrete about the subject (\"Fix sandbox \
seccomp filter\", not \"Debugging a bug\"). Keep the user's own nouns for \
files, tools, and features.";

/// User turns a session needs before it earns a name. Two is enough for the
/// topic to be clear while the session is still worth finding later.
const TITLE_AFTER_TURNS: usize = 2;

/// Longest title kept; anything past this is the model ignoring the prompt.
const TITLE_MAX_CHARS: usize = 60;

/// Name the session once it has enough shape to be worth naming. Best-effort:
/// a failure here must never cost the user their turn, so errors are logged and
/// the session keeps its opening message as its name.
async fn name_session(
    client: &AiClient,
    ctx: &SessionCtx,
    history: &[ChatMessage],
) -> Option<String> {
    if ctx.sub_agent.is_some() {
        return None;
    }
    let turns = history.iter().filter(|m| m.role == "user").count();
    if turns < TITLE_AFTER_TURNS {
        return None;
    }
    // A recorded session names itself once and the transcript remembers it. An
    // unrecorded one (a desktop thread nobody saved) has nowhere to remember,
    // so it names itself exactly on the threshold turn and not again.
    if ctx.recorder.is_some() {
        if ctx.is_titled() {
            return None;
        }
    } else if turns != TITLE_AFTER_TURNS {
        return None;
    }

    let messages = vec![
        ChatMessage {
            role: "system".into(),
            content: TITLE_PROMPT.into(),
        },
        ChatMessage {
            role: "user".into(),
            content: title_context(history),
        },
    ];
    let reply = match client.complete_messages(&messages, 0.2).await {
        Ok(reply) => reply,
        Err(e) => {
            tracing::debug!("could not name the session: {e:#}");
            return None;
        }
    };
    let title = clean_title(&reply)?;
    ctx.record_title(&title);
    Some(title)
}

/// The exchange the titler sees: user turns in full, assistant turns clipped,
/// since the opening lines carry the topic and the rest is tool narration.
fn title_context(history: &[ChatMessage]) -> String {
    let mut out = String::new();
    for m in history
        .iter()
        .filter(|m| m.role == "user" || m.role == "assistant")
    {
        let body = truncate(m.content.trim(), if m.role == "user" { 2000 } else { 500 });
        out.push_str(&m.role);
        out.push_str(": ");
        out.push_str(&body);
        out.push_str("\n\n");
    }
    out
}

/// Strip the wrappers a model reaches for when asked for a bare line: quotes,
/// a trailing period, a markdown heading, extra lines.
fn clean_title(reply: &str) -> Option<String> {
    let line = reply.trim().lines().next()?.trim();
    let line = line.trim_start_matches('#').trim();
    let line = line.trim_matches(|c| c == '"' || c == '\'' || c == '`');
    let line = line.trim_end_matches('.').trim();
    if line.is_empty() || line.chars().count() > TITLE_MAX_CHARS {
        return None;
    }
    Some(line.to_string())
}

async fn compact_if_needed(
    client: &AiClient,
    history: &[ChatMessage],
    ctx: &SessionCtx,
    system_chars: usize,
) -> Result<(Vec<ChatMessage>, Option<Vec<ChatMessage>>)> {
    let total: usize = history.iter().map(|m| m.content.len()).sum();
    let budget = crate::budget::history_budget(ctx.limits.compact_budget_chars, system_chars);
    if total <= budget || !can_compact(history) {
        return Ok((history.to_vec(), None));
    }
    let (compacted, summary, split) = compact_now(client, history).await?;
    ctx.record_summary(&summary, split);
    Ok((compacted.clone(), Some(compacted)))
}

/// True once history has grown past the tail that compaction keeps.
pub(crate) fn can_compact(history: &[ChatMessage]) -> bool {
    history.len() > COMPACT_KEEP_TAIL + 2
}

/// Fold everything but the last few turns into a summary, unconditionally.
/// Returns the folded history plus the summary and split for the transcript.
pub(crate) async fn compact_now(
    client: &AiClient,
    history: &[ChatMessage],
) -> Result<(Vec<ChatMessage>, String, usize)> {
    if !can_compact(history) {
        bail!("nothing to compact yet");
    }
    let split = history.len().saturating_sub(COMPACT_KEEP_TAIL);
    let summary = summarize(client, &history[..split]).await?;
    let mut compacted = Vec::with_capacity(COMPACT_KEEP_TAIL + 1);
    compacted.push(ChatMessage {
        role: "assistant".into(),
        content: format!("Summary of earlier conversation:\n{summary}"),
    });
    compacted.extend(history[split..].iter().cloned());
    Ok((compacted, summary, split))
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

/// Tokens spent between two cumulative snapshots. `None` when the counter did
/// not move, so a provider that reports no usage records nothing rather than a
/// misleading zero.
fn round_usage(before: UsageSnapshot, after: UsageSnapshot) -> Option<EventUsage> {
    let prompt_tokens = after.prompt_tokens.saturating_sub(before.prompt_tokens);
    let completion_tokens = after
        .completion_tokens
        .saturating_sub(before.completion_tokens);
    (prompt_tokens > 0 || completion_tokens > 0).then_some(EventUsage {
        prompt_tokens,
        completion_tokens,
    })
}

fn is_tool_unsupported(e: &anyhow::Error) -> bool {
    // A degenerate reply is never "the model rejected tools", even if the
    // message text happens to mention a tool or function.
    if e.downcast_ref::<DegenerateOutput>().is_some() {
        return false;
    }
    let text = format!("{e:#}").to_lowercase();
    text.contains("tool") || text.contains("function")
}

fn tool_defs(allow_edits: bool, has_approver: bool) -> Vec<Value> {
    let mut tools = vec![
        json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a file, with line numbers. Optionally a line range. Document formats (PDF, Word, PowerPoint, Excel, OpenDocument, EPUB, RTF) are converted to Markdown, so their line numbers do not map to bytes on disk. Paths outside the repository (absolute, or starting with ~) are allowed but the user is asked to approve each one, so prefer repo-relative paths.",
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
                "name": "explore",
                "description": "Run several lookups in ONE round instead of one per round. Every step runs in parallel and all results come back together, labelled. Each model round-trip costs seconds while these lookups take microseconds, so reaching for this instead of a lone read or search is the single biggest thing you can do to answer faster. Use it whenever you need more than one thing before you can act: the file plus the two it references, a search plus the file it will point at, several searches for the same concept. Steps may mix tools freely. Only lookups inside the repository run here; anything else comes back marked, and you call that tool on its own.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "steps": {
                            "type": "array",
                            "description": "Lookups to run together, in the order you want them reported",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "tool": {
                                        "type": "string",
                                        "enum": ["read_file", "search_files", "find_files", "list_files", "recall", "read_skill"],
                                        "description": "Which lookup to run"
                                    },
                                    "args": {
                                        "type": "object",
                                        "description": "That tool's own arguments, exactly as you would send them on their own"
                                    }
                                },
                                "required": ["tool", "args"]
                            }
                        }
                    },
                    "required": ["steps"]
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
                        "dir": { "type": "string", "description": "Repo-relative directory to search under, or a single file to search within (optional)" }
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
                "description": "Ask the user a structured question with a set of options. Only for decisions that are genuinely the user's to make and that you cannot resolve from the request, the code, or sensible defaults. If the user's message already implies the answer, act on it; never ask how to do the thing they just asked for. Not for yes/no approval. The user can pick an option or write their own.",
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
            "description": "Run a CLI command. There is no shell: `&&`, `|`, `>`, `*`, and `cd` are not interpreted, so to chain or pipe pass command:`bash` with args `[\"-lc\", \"one && two | head\"]`. Filesystem writes are restricted to the repository and temp directories (`.git` and CI workflow files are not writable), and secrets are dropped from the environment. Pass turbo:true for offline mode (no network). Pass yolo:true only when the user explicitly asks for unrestricted execution. Returns stdout, stderr, and exit code.",
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
                    "yolo": { "type": "boolean", "description": "Run without any filesystem restrictions. Only use when the user explicitly asks for yolo mode." }
                },
                "required": ["command"]
            }
        }
    }));
    tools.push(json!({
        "type": "function",
        "function": {
            "name": "run_tests",
            "description": "Run the repository's test suite and get structured results: pass/fail counts, failing test names, and the output tail. Detects cargo, npm/bun/pnpm/yarn, pytest, or go from the repo's manifests. Prefer this over run_command for running tests.",
            "parameters": {
                "type": "object",
                "properties": {
                    "runner": { "type": "string", "enum": ["cargo", "npm", "bun", "pnpm", "yarn", "pytest", "go"], "description": "Force a specific runner instead of detecting one" },
                    "filter": { "type": "string", "description": "Only run tests matching this name or pattern" },
                    "turbo": { "type": "boolean", "description": "Run without network access." },
                    "yolo": { "type": "boolean", "description": "Run without any filesystem restrictions. Only use when the user explicitly asks for yolo mode." }
                }
            }
        }
    }));
    tools
}

/// Execute one tool call. Failures come back as plain text so the model can react.
#[allow(clippy::too_many_arguments)]
/// The escapes JSON actually allows.
const JSON_ESCAPES: &str = "\"\\/bfnrtu";

/// Tool arguments, repaired if the model produced an escape JSON does not
/// allow. Bouncing those back costs a whole round, and the intent is never in
/// doubt, so they are fixed here instead.
fn parse_arguments(raw: &str) -> Result<Value> {
    match serde_json::from_str(raw) {
        Ok(value) => Ok(value),
        Err(original) => match serde_json::from_str(&repair_escapes(raw)) {
            Ok(value) => {
                tracing::debug!("repaired an invalid escape in tool arguments");
                Ok(value)
            }
            // Report what the model actually sent, not the repair attempt.
            Err(_) => Err(anyhow::anyhow!("{original}")),
        },
    }
}

/// `\s` and `\.` come from regexes, where the backslash was meant literally,
/// so it is doubled. `\'` comes from shell quoting, where it was not meant at
/// all, so it is dropped.
fn repair_escapes(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 8);
    let mut chars = raw.chars();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if c == '"' {
            in_string = !in_string;
            out.push(c);
            continue;
        }
        if c != '\\' || !in_string {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(next) if JSON_ESCAPES.contains(next) => {
                out.push('\\');
                out.push(next);
            }
            Some('\'') => out.push('\''),
            Some(next) => {
                out.push_str("\\\\");
                out.push(next);
            }
            None => out.push('\\'),
        }
    }
    out
}

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
    let args: Value = match parse_arguments(arguments) {
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
                    Ok(target) => cached_read(
                        ctx,
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
        "explore" => explore(repo_root, policy, ctx, &args).await,
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
                let env = ExecEnv {
                    repo_root,
                    policy,
                    approver,
                    credentials: &ctx.credentials,
                    store: ctx.store.as_ref(),
                    yolo: ctx.yolo,
                };
                run_command_tool(&env, &cmd, &cmd_args, run_opts(&args, ctx)).await
            }
            Err(e) => Err(e),
        },
        "run_tests" => {
            let env = ExecEnv {
                repo_root,
                policy,
                approver,
                credentials: &ctx.credentials,
                store: ctx.store.as_ref(),
                yolo: ctx.yolo,
            };
            run_tests_tool(
                &env,
                str_arg("runner").as_deref(),
                str_arg("filter").as_deref(),
                run_opts(&args, ctx),
            )
            .await
        }
        "aster_mcp" => mcp_bridge(policy, approver, ctx, arguments).await,
        other => Err(anyhow::anyhow!(
            "unknown tool: {other}. Available tools: {}",
            tool_names(*allow_edits, approver.is_some()).join(", ")
        )),
    };
    result.unwrap_or_else(|e| format!("error: {e:#}"))
}

/// Tools a round may run on parallel threads: read-only, no approval prompts,
/// no ordering against edits or commands.
const PARALLEL_READ_TOOLS: [&str; 6] = [
    "read_file",
    "list_files",
    "search_files",
    "find_files",
    "recall",
    "read_skill",
];

/// Lookups worth deduplicating within a turn: pure, cheap, and answered
/// entirely from the tree. `read_file` is absent because [`cached_read`]
/// already dedupes it per range, against the file's mtime.
const DEDUPED_LOOKUPS: [&str; 4] = ["list_files", "search_files", "find_files", "explore"];

/// True once this exact lookup has been answered in this turn. Recording and
/// testing are one step so two identical calls in the same batch cannot both
/// miss. Anything outside [`DEDUPED_LOOKUPS`] is never a repeat and clears the
/// cache instead: a command or an edit can change what a lookup would return.
fn is_repeat_lookup(ctx: &SessionCtx, name: &str, arguments: &str) -> bool {
    let Ok(mut lookups) = ctx.lookups.lock() else {
        return false;
    };
    if !DEDUPED_LOOKUPS.contains(&name) {
        lookups.clear();
        return false;
    }
    !lookups.insert(format!("{name}:{arguments}"))
}

/// Fan a batch of lookups out in one round. Every step goes through
/// [`read_only_call`], so nothing here escapes the policy or runs anything
/// stateful; a step it declines is reported rather than run another way.
async fn explore(
    repo_root: &Path,
    policy: &Policy,
    ctx: &SessionCtx,
    args: &Value,
) -> Result<String> {
    let steps = args["steps"]
        .as_array()
        .context("explore needs a `steps` array")?;
    if steps.is_empty() {
        bail!("explore needs at least one step");
    }
    let handles: Vec<_> = steps
        .iter()
        .map(|step| {
            let name = step["tool"].as_str().unwrap_or_default().to_string();
            let arguments = step
                .get("args")
                .cloned()
                .unwrap_or_else(|| json!({}))
                .to_string();
            let label = step_label(&name, &step["args"]);
            let repo_root = repo_root.to_path_buf();
            let policy = policy.clone();
            let ctx = ctx.clone();
            tokio::task::spawn_blocking(move || {
                (
                    label,
                    read_only_call(&repo_root, &policy, &ctx, &name, &arguments),
                )
            })
        })
        .collect();

    let mut out = String::new();
    for (i, handle) in handles.into_iter().enumerate() {
        let (label, result) = match handle.await {
            Ok(pair) => pair,
            Err(e) => (format!("step {}", i + 1), Some(format!("error: {e}"))),
        };
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format!("[{}] {label}\n", i + 1));
        match result {
            Some(text) => out.push_str(&text),
            None => out.push_str(
                "not runnable in a batch (outside the repository, or not a lookup); \
                 call this tool on its own",
            ),
        }
        out.push('\n');
    }
    Ok(out.trim_end().to_string())
}

/// Name a step by its most identifying argument, so the model can tell which
/// result is which without re-reading the arguments it sent.
fn step_label(tool: &str, args: &Value) -> String {
    match ["path", "query", "pattern", "dir", "name"]
        .iter()
        .find_map(|key| args.get(key).and_then(Value::as_str))
    {
        Some(detail) => format!("{tool} {detail}"),
        None => tool.to_string(),
    }
}

/// Run one read-only call without the approval machinery. `None` sends the
/// call back to the sequential pass, which can prompt for outside-repo paths
/// and report argument errors.
fn read_only_call(
    repo_root: &Path,
    policy: &Policy,
    ctx: &SessionCtx,
    name: &str,
    arguments: &str,
) -> Option<String> {
    let args: Value = serde_json::from_str(arguments).ok()?;
    let str_arg = |key: &str| args[key].as_str().map(str::to_string);
    let resolve_dir =
        |dir: &Option<String>| match dir.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
            Some(dir) => resolve_in_repo(repo_root, policy, dir),
            None => Some(Ok(repo_root.to_path_buf())),
        };

    let result = match name {
        "read_file" => {
            let path = str_arg("path")?;
            if !edits::exists_anywhere(repo_root, &path) {
                return Some(missing_path(repo_root, &path));
            }
            match resolve_in_repo(repo_root, policy, &path)? {
                Ok(target) => cached_read(
                    ctx,
                    &target,
                    args["start_line"].as_u64().map(|n| n as usize),
                    args["end_line"].as_u64().map(|n| n as usize),
                ),
                Err(e) => Err(e),
            }
        }
        "list_files" => match missing_dir(repo_root, &str_arg("dir")) {
            Some(dir) => return Some(missing_path(repo_root, &dir)),
            None => match resolve_dir(&str_arg("dir"))? {
                Ok(base) => list_files(&ctx.probe, &base),
                Err(e) => Err(e),
            },
        },
        "search_files" => {
            let query = str_arg("query")?;
            match missing_dir(repo_root, &str_arg("dir")) {
                Some(dir) => search_files(&ctx.probe, repo_root, policy, &query, repo_root)
                    .map(|hits| widened(&dir, hits)),
                None => match resolve_dir(&str_arg("dir"))? {
                    Ok(base) => search_files(&ctx.probe, repo_root, policy, &query, &base),
                    Err(e) => Err(e),
                },
            }
        }
        "find_files" => {
            let pattern = str_arg("pattern")?;
            match missing_dir(repo_root, &str_arg("dir")) {
                Some(dir) => bash_tools::find(repo_root, repo_root, &pattern, MAX_FIND_HITS)
                    .map(|hits| widened(&dir, hits)),
                None => match resolve_dir(&str_arg("dir"))? {
                    Ok(base) => bash_tools::find(repo_root, &base, &pattern, MAX_FIND_HITS),
                    Err(e) => Err(e),
                },
            }
        }
        "recall" => recall(ctx, &str_arg("name")?),
        "read_skill" => read_skill(ctx, &str_arg("name")?),
        _ => return None,
    };
    Some(result.unwrap_or_else(|e| format!("error: {e:#}")))
}

pub(crate) fn tool_names(allow_edits: bool, has_approver: bool) -> Vec<String> {
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
            return Ok("the user declined to answer; proceed with your best judgement".to_string());
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

/// The sandbox switches as the model passed them, with the session's yolo state
/// and configured timeout folded in.
fn run_opts(args: &Value, ctx: &SessionCtx) -> RunOpts {
    RunOpts {
        turbo: args["turbo"].as_bool().unwrap_or(false),
        yolo: args["yolo"].as_bool().unwrap_or(false) || ctx.yolo,
        timeout_secs: ctx.limits.command_timeout_secs as u64,
    }
}

/// Where a command runs and who can approve it, shared by every exec tool.
struct ExecEnv<'a> {
    repo_root: &'a Path,
    policy: &'a Policy,
    approver: Option<&'a UiSender>,
    credentials: &'a aster_policy::CommandGrants,
    /// Where an "always" answer is written, when the session persists at all.
    store: Option<&'a Store>,
    /// No sandbox means no credential boundary to ask about.
    yolo: bool,
}

/// Sandbox switches and the per-command timeout.
#[derive(Clone, Copy)]
struct RunOpts {
    turbo: bool,
    yolo: bool,
    timeout_secs: u64,
}

/// Every execution goes through the same `Decision` path as edits: deny rules
/// override the mode, ask-style modes prompt, headless runs without an
/// approver deny.
async fn authorize_exec(env: &ExecEnv<'_>, binary: &str, args: &[String]) -> Result<()> {
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    match env.policy.evaluate(&Action::Exec {
        binary,
        args: &arg_refs,
    }) {
        Decision::Allow => Ok::<(), anyhow::Error>(()),
        Decision::Deny { reason } => bail!("{reason}"),
        Decision::Prompt { preview } => {
            if !request_approval(env.approver, preview, None)
                .await
                .allowed()
            {
                bail!(
                    "command `{binary}` needs user approval; it was rejected or this run cannot ask"
                );
            }
            Ok(())
        }
    }?;
    authorize_credentials(env, binary, args).await
}

/// A tool that keeps its credentials outside the repository is asked about
/// rather than refused. The sandbox denies those directories by default, which
/// used to fail the command outright: `gh` could not read `~/.config/gh`, so
/// every GitHub operation died even though a core skill prescribes `gh`.
///
/// An approval covers one command and one directory, so approving `gh` never
/// lets the next `cat` read the token.
async fn authorize_credentials(env: &ExecEnv<'_>, binary: &str, args: &[String]) -> Result<()> {
    if env.yolo {
        return Ok(());
    }
    let command = aster_sandbox::command_name(binary);
    for dir in aster_sandbox::credentials_for(binary, args) {
        if env.credentials.allows(&command, &dir) {
            continue;
        }
        let preview = format!(
            "`{command}` needs to read credentials outside the repository:\n  {}",
            crate::edits::display_home(&dir)
        );
        match request_approval(env.approver, preview, Some(dir.clone())).await {
            Answer::No => bail!(
                "`{command}` needs to read {} and was not allowed to; it was rejected, \
                 or this run has no way to ask. Preauthorize it with \
                 `permissions.allow_credentials: [\"{command}:{}\"]` in aster.yaml",
                crate::edits::display_home(&dir),
                crate::edits::display_home(&dir),
            ),
            Answer::Yes => env.credentials.grant(&command, dir),
            Answer::Always => {
                env.credentials.grant(&command, dir.clone());
                if let Some(store) = env.store
                    && let Err(e) = store
                        .credential_grants(env.repo_root)
                        .add(Path::new(&format!("{command}\t{}", dir.display())))
                {
                    tracing::warn!("could not persist the credential grant: {e:#}");
                }
            }
        }
    }
    Ok(())
}

/// Run sandboxed unless yolo, returning the raw streams for the caller to shape.
async fn run_raw(
    env: &ExecEnv<'_>,
    binary: &str,
    args: &[String],
    opts: RunOpts,
) -> Result<aster_sandbox::CommandOutput> {
    if opts.yolo {
        let output = tokio::process::Command::new(binary)
            .args(args)
            .current_dir(env.repo_root)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await
            .context("running command")?;
        return Ok(aster_sandbox::CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code(),
            timed_out: false,
        });
    }
    let profile = aster_sandbox::SandboxProfile::new(env.repo_root)
        .timeout(opts.timeout_secs)
        .network(!opts.turbo)
        .allow_credentials(
            env.credentials
                .dirs_for(&aster_sandbox::command_name(binary)),
        );
    let config = aster_sandbox::SandboxConfig::new(profile);
    aster_sandbox::run_command(&config, binary, args).await
}

/// Notes appended to a command result for failure classes models misread:
/// pipe-masked build failures, buried first errors, auth failures worth zero
/// retries, and sandbox denials that look like broken tools.
fn command_coaching(output: &aster_sandbox::CommandOutput, sandboxed: bool) -> Vec<String> {
    let mut notes = Vec::new();
    let failed = output.exit_code != Some(0);
    let combined = format!("{}\n{}", output.stdout, output.stderr);

    if let Some(line) = first_error_line(&combined) {
        if output.exit_code == Some(0) {
            notes.push(format!(
                "note: exit code 0 comes from the last command in the pipe; the \
                 build or test run itself failed. First error: {line}. The \
                 `build-triage` skill (read_skill) has the full protocol."
            ));
        } else {
            notes.push(format!(
                "note: first error in the output: {line}. Fix the first error \
                 before any later one; the `build-triage` skill (read_skill) \
                 has the full protocol."
            ));
        }
    }

    let lower = combined.to_lowercase();
    if failed
        && [
            "unauthorized",
            "authentication failed",
            "invalid credentials",
            "not logged in",
            "please log in",
            "token expired",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
    {
        notes.push(
            "note: this is an auth failure; retrying the same command will not \
             help. Tell the user what to log in to, and continue with what does \
             not need it."
                .into(),
        );
    }

    let denial = [
        "permission denied",
        "permissiondenied",
        "eperm",
        // What macOS actually prints when Seatbelt refuses a read.
        "operation not permitted",
    ]
        .iter()
        .any(|marker| lower.contains(marker))
        // ssh's "Permission denied (publickey)" is auth, not the sandbox.
        && !lower.contains("publickey");
    if sandboxed && failed && denial {
        notes.push(
            "note: this command ran inside the sandbox, which only allows writes \
             to the repository, temp directories, and build caches. A permission \
             error here usually means the sandbox blocked a path, not that the \
             tool or network is broken. Prefer a path the sandbox allows; if the \
             task truly needs the blocked path, say so to the user instead of \
             switching tools."
                .into(),
        );
    }
    notes
}

/// The first line of `output` that looks like a compiler or test failure.
fn first_error_line(output: &str) -> Option<String> {
    output
        .lines()
        .find(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("error:")
                || trimmed.starts_with("error[")
                || trimmed.contains("error TS")
                || trimmed.starts_with("FAILED")
                || trimmed.contains("panicked at")
        })
        .map(|line| line.trim().to_string())
}

/// Render a timed-out command for the model: the partial output is the
/// evidence, and the coaching stops the two observed dead ends (diagnosing
/// from nothing, or re-running with a longer timeout).
fn render_timeout(output: &aster_sandbox::CommandOutput, timeout_secs: u64) -> String {
    let mut result = format!("error: command timed out after {timeout_secs}s\n");
    if output.stdout.is_empty() && output.stderr.is_empty() {
        result.push_str("(no output before the timeout)\n");
    } else {
        result.push_str("output before the timeout:\n");
        if !output.stdout.is_empty() {
            result.push_str("stdout:\n");
            result.push_str(&truncate_head(&output.stdout, MAX_STREAM_CHARS));
            result.push('\n');
        }
        if !output.stderr.is_empty() {
            result.push_str("stderr:\n");
            result.push_str(&truncate_head(&output.stderr, MAX_STREAM_CHARS));
            result.push('\n');
        }
    }
    result.push_str(
        "Do NOT re-run this command with a longer timeout. Kill any leftover \
         processes first, then run a narrower or faster variant (scope it to \
         one target, bound its output, or skip the slow step). The \
         `build-triage` skill (read_skill) has the full protocol.",
    );
    result
}

async fn run_command_tool(
    env: &ExecEnv<'_>,
    binary: &str,
    args: &[String],
    opts: RunOpts,
) -> Result<String> {
    authorize_exec(env, binary, args).await?;
    let output = run_raw(env, binary, args, opts).await?;
    if output.timed_out {
        return Ok(render_timeout(&output, opts.timeout_secs));
    }
    let exit_code = output.exit_code.unwrap_or(-1);
    let mut result = String::new();
    if !output.stdout.is_empty() {
        result.push_str("stdout:\n");
        result.push_str(&truncate(&output.stdout, MAX_STREAM_CHARS));
    }
    if !output.stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str("stderr:\n");
        // Compilers and test runners put the verdict last, so keep the tail.
        result.push_str(&truncate_head(&output.stderr, MAX_STREAM_CHARS));
    }
    result.push_str(&format!("\nexit code: {exit_code}"));
    for note in command_coaching(&output, !opts.yolo) {
        result.push('\n');
        result.push_str(&note);
    }
    if result.is_empty() {
        return Ok("(no output)".into());
    }
    Ok(result)
}

async fn run_tests_tool(
    env: &ExecEnv<'_>,
    runner: Option<&str>,
    filter: Option<&str>,
    opts: RunOpts,
) -> Result<String> {
    let cmd = crate::test_runner::detect(env.repo_root, runner, filter)?;
    authorize_exec(env, &cmd.binary, &cmd.args).await?;
    let output = run_raw(env, &cmd.binary, &cmd.args, opts).await?;
    if output.timed_out {
        return Ok(render_timeout(&output, opts.timeout_secs));
    }
    let result = crate::test_runner::parse(
        cmd.runner,
        &output.stdout,
        &output.stderr,
        output.exit_code.unwrap_or(-1),
    );
    serde_json::to_string_pretty(&result).context("serializing test results")
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

/// Seed the session's credential grants from `permissions.allow_credentials`
/// (written `<command>:<dir>`) and the persisted store, whose entries are
/// `<command>\t<dir>`. A malformed entry is dropped rather than failing the
/// run: a typo in aster.yaml should not stop the agent from starting.
pub(crate) fn configured_credentials(
    permissions: &aster_policy::PermissionsConfig,
    repo_root: &Path,
) -> aster_policy::CommandGrants {
    let configured = permissions
        .allow_credentials
        .iter()
        .filter_map(|entry| entry.split_once(':'))
        .map(|(command, dir)| (command.trim().to_string(), edits::expand_home(dir.trim())));
    let persisted = crate::persist::store()
        .map(|store| store.credential_grants(repo_root).load())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entry| {
            let text = entry.to_string_lossy().into_owned();
            let (command, dir) = text.split_once('\t')?;
            Some((command.to_string(), PathBuf::from(dir)))
        });
    aster_policy::CommandGrants::new(configured.chain(persisted))
}

/// The directory an approval covers: the file's parent, or the directory itself.
fn grant_root(resolved: &Path) -> PathBuf {
    if resolved.is_dir() {
        return resolved.to_path_buf();
    }
    resolved.parent().unwrap_or(resolved).to_path_buf()
}

/// Sync resolution for in-repo reads, usable off the async loop. `None` means
/// the path leaves the repo and needs the approval flow.
fn resolve_in_repo(repo_root: &Path, policy: &Policy, path: &str) -> Option<Result<PathBuf>> {
    let (resolved, scope) = match edits::resolve_anywhere(repo_root, path) {
        Ok(pair) => pair,
        Err(e) => return Some(Err(e)),
    };
    if !matches!(scope, edits::Scope::InRepo) {
        return None;
    }
    let root = repo_root.canonicalize().unwrap_or_default();
    let relative = resolved.strip_prefix(&root).unwrap_or(&resolved);
    if let Decision::Deny { reason } = policy.evaluate(&Action::Read {
        path: &relative.to_string_lossy(),
    }) {
        return Some(Err(anyhow::anyhow!("{reason}")));
    }
    Some(Ok(resolved))
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
    if let Some(result) = resolve_in_repo(repo_root, policy, path) {
        return result;
    }
    let (resolved, scope) = edits::resolve_anywhere(repo_root, path)?;
    match scope {
        edits::Scope::InRepo => {}
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

/// `read_numbered`, but a repeat read of an unchanged range returns a pointer
/// to the copy already in the conversation. Re-sending a file the model can
/// still see is the single largest source of wasted context.
fn cached_read(
    ctx: &SessionCtx,
    target: &Path,
    start: Option<usize>,
    end: Option<usize>,
) -> Result<String> {
    let modified = fs::metadata(target).and_then(|m| m.modified()).ok();
    let key = format!("{}:{start:?}:{end:?}", target.display());
    let seen = ctx
        .reads
        .lock()
        .ok()
        .and_then(|reads| reads.get(&key).copied());
    // Only a byte-identical situation is skipped: no mtime, or a changed one,
    // reads for real.
    if let Some(previous) = seen
        && previous.is_some()
        && previous == modified
    {
        return Ok(format!(
            "[unchanged since you read it earlier in this turn — scroll up for {}]",
            target.display()
        ));
    }
    let body = read_numbered(target, start, end)?;
    if let Ok(mut reads) = ctx.reads.lock() {
        reads.insert(key, modified);
    }
    Ok(body)
}

/// Read a file as text, converting document formats (PDF, Office, EPUB, RTF)
/// to Markdown via anydoc. CSV is exempt: it reads raw so line numbers keep
/// matching the bytes on disk for edits.
fn read_text_or_document(target: &Path) -> Result<String> {
    let format = target
        .extension()
        .and_then(|e| e.to_str())
        .and_then(anydoc::Format::from_extension);
    if let Some(format) = format
        && !matches!(format, anydoc::Format::Csv)
    {
        return anydoc::to_markdown(target)
            .map_err(|e| anyhow::anyhow!("converting {} to Markdown: {e}", target.display()));
    }
    match fs::read_to_string(target) {
        Ok(text) => Ok(text),
        // A document with a missing or misleading extension: sniff the bytes.
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
            let bytes =
                fs::read(target).with_context(|| format!("reading {}", target.display()))?;
            match anydoc::Format::from_bytes(&bytes) {
                Some(format) => anydoc::to_markdown_bytes(&bytes, format).map_err(|e| {
                    anyhow::anyhow!("converting {} to Markdown: {e}", target.display())
                }),
                None => bail!(
                    "{} is a binary file, not readable as text",
                    target.display()
                ),
            }
        }
        Err(e) => Err(e).with_context(|| format!("reading {}", target.display())),
    }
}

fn read_numbered(target: &Path, start: Option<usize>, end: Option<usize>) -> Result<String> {
    let content = read_text_or_document(target)?;
    let lines: Vec<&str> = content.lines().collect();
    let from = start.unwrap_or(1).max(1) - 1;
    // An open-ended read is windowed rather than truncated mid-file, so the
    // model knows exactly where to resume instead of re-reading blindly.
    let requested_end = end.unwrap_or(lines.len());
    let to = requested_end.min(lines.len()).min(from + READ_WINDOW_LINES);
    if from >= to {
        bail!("empty range: the file has {} lines", lines.len());
    }
    let mut body = lines[from..to]
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{:>5} | {l}", from + i + 1))
        .collect::<Vec<_>>()
        .join("\n");
    if to < lines.len() {
        body.push_str(&format!(
            "\n\n[showing lines {}-{to} of {}; call read_file again with start_line={} for more]",
            from + 1,
            lines.len(),
            to + 1,
        ));
    }
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

/// Keep the end instead of the beginning: build and test failures state the
/// verdict last, and that is the part worth spending context on.
fn truncate_head(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut cut = text.len() - max;
    while !text.is_char_boundary(cut) {
        cut += 1;
    }
    format!("... [truncated]\n{}", &text[cut..])
}

/// Schema for the `agent` tool: fan out N tasks to sub-agents.
fn agent_tool_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "agent",
            "description": "Fan out self-contained tasks to named sub-agents in parallel. Each agent starts with a fresh context and cannot see this conversation. For broad investigation, fan several cheap explorer agents out in one call, then pass their raw reports to `synthesizer` in a second call. Limit batch size to avoid overwhelming the system.",
            "parameters": {
                "type": "object",
                "properties": {
                    "tasks": {
                        "type": "array",
                        "description": "One or more agent tasks to run in parallel",
                        "items": {
                            "type": "object",
                            "properties": {
                                "agent": { "type": "string", "description": "Name of the agent to invoke" },
                                "task": { "type": "string", "description": "Self-contained task for the agent" }
                            },
                            "required": ["agent", "task"]
                        },
                        "minItems": 1
                    }
                },
                "required": ["tasks"]
            }
        }
    })
}

/// Dispatch one `agent` tool call: run the swarm, cap reports, return JSON.
/// Streams `agent_status` events so the UIs can render live per-agent progress.
#[allow(clippy::too_many_arguments)]
async fn dispatch_agent_tool(
    repo_root: &Path,
    client: &AiClient,
    arguments: &str,
    policy: &Policy,
    _grants: &Grants,
    ctx: &SessionCtx,
    call_id: &str,
    events: Option<&ChatEventSink>,
) -> String {
    let args: Value = match parse_arguments(arguments) {
        Ok(v) => v,
        Err(e) => return format!("error: agent arguments were not valid JSON: {e}"),
    };
    let Some(tasks_val) = args.get("tasks").and_then(Value::as_array) else {
        return "error: agent tool requires a `tasks` array".to_string();
    };
    let mut tasks: Vec<crate::agents::AgentTask> = Vec::new();
    let max_per_turn = ctx.swarm.max_per_turn;
    for entry in tasks_val.iter().take(max_per_turn) {
        let Some(agent) = entry.get("agent").and_then(Value::as_str) else {
            continue;
        };
        let Some(task) = entry.get("task").and_then(Value::as_str) else {
            continue;
        };
        tasks.push(crate::agents::AgentTask {
            agent: agent.to_string(),
            task: task.to_string(),
        });
    }
    if tasks.is_empty() {
        return "error: agent tool needs at least one valid task with `agent` and `task` fields"
            .to_string();
    }

    let over_cap = tasks_val.len().saturating_sub(max_per_turn);
    let deps = crate::agents::AgentDeps {
        client: client.clone(),
        repo_root: repo_root.to_path_buf(),
        policy: Arc::new(policy.clone()),
        grants: Arc::new(Grants::default()),
        credentials: ctx.credentials.clone(),
        probe: ctx.probe.clone(),
        environment: ctx.environment.clone(),
        limits: ctx.limits,
        swarm: ctx.swarm.clone(),
        session_registry: ctx.agents.clone(),
    };

    // Seed the UI with the whole batch up front so it can show "2/3"-style
    // progress; run_swarm then reports each task's completion as it lands.
    if let Some(sink) = events {
        let total = tasks.len();
        for t in &tasks {
            sink(json!({
                "type": "agent_status",
                "call_id": call_id,
                "agent": t.agent,
                "task": t.task,
                "status": "running",
                "done": 0,
                "total": total,
            }));
        }
    }
    let on_complete = |p: crate::agents::AgentProgress| {
        if let Some(sink) = events {
            let mut ev = json!({
                "type": "agent_status",
                "call_id": call_id,
                "agent": p.agent,
                "task": p.task,
                "status": match p.status {
                    crate::agents::ProgressStatus::Done => "done",
                    crate::agents::ProgressStatus::Failed => "error",
                },
                "done": p.done,
                "total": p.total,
            });
            if let Some(report) = p.report {
                ev["report"] = Value::String(report);
            }
            if let Some(err) = p.error {
                ev["error"] = Value::String(err);
            }
            sink(ev);
        }
    };
    let mut reports = crate::agents::run_swarm(tasks, &ctx.agents, &deps, on_complete).await;

    // Cap each report so the total fits in MAX_TOOL_RESULT_CHARS.
    let count = reports.len().max(1);
    let per_report = MAX_TOOL_RESULT_CHARS / count;
    for r in &mut reports {
        if let Some(ref mut report) = r.report
            && report.len() > per_report
        {
            *report = truncate(report, per_report);
        }
    }

    let mut result = serde_json::to_string(&reports).unwrap_or_default();
    if over_cap > 0 {
        result.push_str(&format!(
            "\n\n({over_cap} additional task(s) skipped: per-turn cap is {max_per_turn})"
        ));
    }
    result
}

#[cfg(test)]
#[path = "tests/chat_test.rs"]
mod tests;
