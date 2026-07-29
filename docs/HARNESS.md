# Harness design

How Aster's harness semantics get fixed: sessions, memory, approvals, structured
interaction, delegation, and scheduling. This document slots around
[ROADMAP.md](ROADMAP.md) rather than replacing it — the roadmap owns the tool
registry, sandbox, dispatch, trace, and eval workstreams; this document owns the
UX and semantics the roadmap does not cover, and says where each phase hangs off
the roadmap's sequencing.

The philosophy is simple: we do not reinvent the wheel. Every mechanism below is
adapted from something proven — either a modern harness (Claude Code, Codex CLI,
Amp, opencode) or an older idea from computer science that turns out to be the
right shape for agents: event sourcing for session persistence, capability
attenuation for tool scoping, contracts for delegation. Where there is
controlled evidence, we follow it:

- One level of progressive disclosure is enough. A second routing level never
  helps and sometimes breaks accuracy outright
  ([arXiv:2607.17598](https://arxiv.org/abs/2607.17598)). Aster's existing
  skills shape (index of name plus description, body via `read_skill`) is
  already correct; it needs a budget, not more depth.
- Fresh subagent per subgoal with an independent finish gate beats one context
  carrying everything ([StateAct, arXiv:2607.22798](https://arxiv.org/abs/2607.22798)).
- Harnesses that tune their own scaffolds invent failures to fix
  ([Phantom Guardrails, arXiv:2607.13083](https://arxiv.org/abs/2607.13083)).
  Aster never edits its own prompts, skills, or agent definitions mid-session,
  and policy enforces that.

## Where the harness is shaky today

- The TUI unconditionally auto-resumes the repo's latest session
  (`resume_or_new`, [tui/chat.rs:327](../crates/aster-cli/src/tui/chat.rs)),
  while headless chat is explicit and ephemeral
  ([chat.rs:530](../crates/aster-cli/src/chat.rs)). Worse, `aster chat
  --continue` in a terminal is silently ignored: `run()` branches to the TUI
  before the flag is consulted (chat.rs:293).
- Sessions accumulate forever. No retention, no pruning, and listing fully
  parses every transcript ([sessions.rs:29](../crates/aster-cli/src/sessions.rs)).
- Memory is global across all repos and has no size limit, so it can only rot.
- `aster-agents` and `aster-mcp` are complete, tested, and wired to nothing.
  The agents registry even advertises an `agent` tool that does not exist
  ([registry.rs:61](../crates/aster-agents/src/registry.rs)).
- There is no way for the model to ask the user a structured question, show a
  plan, maintain a todo list, or delegate work.
- "Always" on an edit approval promotes the whole session to `Mode::Edit`
  ([tui/chat.rs:849](../crates/aster-cli/src/tui/chat.rs)) — a global
  escalation from one keystroke.

Nine phases fix this. Phases 1–4 are pure persistence and UX work that can ship
in parallel with the roadmap's sandbox workstream. Phase 5 is the roadmap's own
tool-registry phase, referenced not redesigned. Phases 6–9 build on it.

---

## Phase 1 — Session log plus index, retention, fork

**Gap.** JSONL transcripts are already a crash-safe write-ahead log
([transcript.rs](../crates/aster-persist/src/transcript.rs)), but every query
about sessions re-reads the logs, nothing is ever deleted, and there is no
lineage between sessions.

**Shape.** The Codex split: the log stays truth, a SQLite index answers
queries. One file at `<config>/aster/sessions.sqlite`, one table:

```sql
sessions(id TEXT PRIMARY KEY, project_slug TEXT, path TEXT,
         created_at TEXT, updated_at TEXT, model TEXT,
         title TEXT, user_turns INTEGER, bytes INTEGER,
         forked_from_id TEXT)
```

Dependency is `rusqlite` with the bundled feature — `aster-persist` is a
synchronous crate and stays one. The index is a cache, never truth:
`Store::reindex` rebuilds it from headers (reuse `read_header`,
[lib.rs:174](../crates/aster-persist/src/lib.rs)), and a queried id missing
from the index triggers reindex automatically. Deleting the sqlite file loses
nothing.

Fork adds `forked_from_id: Option<String>` to `SessionMeta` with serde
defaults, so `TRANSCRIPT_VERSION` stays 1-compatible. `Store::fork` copies the
`.jsonl`, mints a new ULID, and records lineage. Compaction is already
non-destructive — the `Summary` event appended to the log is the checkpoint
record — so nothing changes there.

Retention comes from `aster.yaml`:

```yaml
sessions:
  retention_days: 30
  max_per_project: 200
```

The sweep runs on `Store::open` via one indexed query and the existing
`delete_session`.

**Done when.** Listing no longer parses transcripts; deleting the index and
relisting reproduces identical rows; the sweep deletes only past-threshold
files; fork lineage is queryable.

## Phase 2 — The TUI never auto-resumes

**Gap.** Implicit infinite append into one growing file per repo, and a
`--continue` flag the TUI ignores.

**Shape.** Default is a new session, always. `tui::run_chat` gains a
`SessionChoice { New, Latest, Id(String) }` populated from `--continue` and a
new `--resume <id>` flag, restoring parity with the headless contract:
`--session` keeps resume-or-create, `--resume` is resume-only and errors if the
id is absent, `--continue` means latest.

In-session, `/resume` opens a picker overlay fed by the Phase 1 index (id,
age, turns, title — cheap now), and `/fork` branches the current session via
`Store::fork`. The approval overlay in `tui/chat.rs` is the rendering pattern
to copy.

Resuming something big and stale should not silently re-pay for the whole
context: when the transcript exceeds `COMPACT_BUDGET_CHARS` and `updated_at`
is more than an hour old, seed history as the latest summary plus the last
`COMPACT_KEEP_TAIL` turns instead of the full replay (make
`SessionTranscript::latest_summary` public). This is Claude Code's
resume-from-summary gate: cache expiry, not size alone, is the natural
compaction boundary.

**Done when.** A fresh TUI launch creates a second session file rather than
appending to the newest; `--continue` seeds prior history (regression test for
the current bug); the gate test with a synthetic oversized transcript seeds
summary-first.

## Phase 3 — "Always" on an edit grants a path, not a mode

**Gap.** `Answer::Always` on an edit approval flips the session to
`Mode::Edit`. One keystroke, global blast radius.

**Shape.** `edit_file` currently sends `scope: None`, which is exactly why the
TUI falls back to mode promotion. It passes `scope =
Some(grant_root(resolved))` instead, and a second session-only `Grants`
instance (`edit_grants`, reusing
[grants.rs](../crates/aster-policy/src/grants.rs) unchanged) is consulted
before prompting. `Always` inserts into `edit_grants` only and is not
persisted — "stop asking about this directory for this session" is the right
blast radius for a write. The mode-promotion branch is deleted.

**Done when.** Two edits in one directory produce one prompt; the session mode
never changes on `Always`.

## Phase 4 — Memory scoped per project, with an enforced budget

**Gap.** One global memory directory shared by every repo, injected in full,
growing forever.

**Shape.** Grants already got this right
([lib.rs:36](../crates/aster-persist/src/lib.rs)): per-project by slug. Memory
follows. `<config>/aster/memory/` stays global; `memory/projects/<slug>/` is
added via `Store::project_memory(repo_root)`. `MemoryStore::new(dir)` is
already directory-parameterized, so the whole change is where the directory
comes from. Injection composes global then project `ASTER.md`, block indexes
merged with project shadowing global on name collision, and `recall` resolves
project first. One disclosure level, unchanged.

The budget is enforced the way Claude Code enforces its index: measure before
write, and put the failure in front of the model.
`MEMORY_CONTEXT_BUDGET_CHARS = 8_000` for the injected portion,
`MAX_BLOCKS_IN_INDEX = 50`. `remember` and `append_project` compute the
prospective `load_context` size before writing; over budget returns an error
the model sees verbatim: "memory is full (N/8000 chars): consolidate or delete
a block first". No silent truncation. `aster memory` gains `--global` and
`delete <name>` — a budget that can fill needs a drain.

**Done when.** Two repos have disjoint project memory; a project block shadows
a global block of the same name; an over-budget write errors and leaves files
untouched.

## Phase 5 — Tool registry

This is [ROADMAP.md](ROADMAP.md) workstream 6, phase 1, built as specced
there. This document adds only the one property later phases depend on: the
allowlist is enforced at dispatch, not just by omitting schemas.

```rust
pub enum Allowlist { All, EditGated(bool), Named(Vec<String>) }
```

`Named` is what `AgentDef::tools` plugs into in Phase 8, and a tool absent
from it must fail at `Registry::dispatch`, not merely be hidden from
`Registry::defs`. That is the difference between an allowlist and a
capability: the explorer agent physically cannot call `edit_file`, regardless
of what the model asks for.

## Phase 6 — Structured questions, a visible plan, and a plan-mode handshake

**Gap.** The model cannot ask the user anything except yes/no on an approval,
plan mode is just "edits denied", and there is no visible task state.

**Shape.** Three tools, one channel change.

The approval mpsc generalizes to `UiRequest { Approval(ApprovalRequest),
Question(QuestionRequest) }`. The `ask_user` tool takes:

```json
{"header": "≤12 chars", "question": "...", "multiSelect": false,
 "options": [{"label": "...", "description": "..."}]}
```

with 2–4 options validated at the schema. The front end always appends an
"Other (type it)" option. The answer is a trichotomy borrowed from MCP
elicitation, because "user said no" and "user walked away" are different
facts: `Accepted(Vec<String>) | Declined | Cancelled`. The tool result is
JSON in-band (`{"outcome":"accepted","choices":[...]}`), never an error.
Headless returns declined immediately; the `--stream` path emits a
`{"type":"question"}` line and reads one reply line, mirroring
`stdio_approver`.

`update_plan` maintains steps with `pending | in_progress | done`, held on
`SessionCtx`, rendered as a TUI header strip, emitted as a `{"type":"plan"}`
NDJSON event. Shared visible state, no extra routing.

`exit_plan_mode` is registered only while the session mode is `Plan`. It
presents the plan as an approval preview; on yes the mode flips to
`stricter(configured, Manual)` (reuse `Mode::stricter`) and tool definitions
re-issue next round so `edit_file` appears; on no the model is told to revise.
Headless declines. Plan mode stops being vibes and becomes a handshake.

**Done when.** Question answered, declined, and cancelled paths are tested;
headless `ask_user` declines; `exit_plan_mode` is absent from definitions
outside `Plan`; a 1-option or 5-option call is rejected.

## Phase 7 — Skills index budget, and the MCP bridge finally wired

**Gap.** The skills index grows linearly with skill count with no cap, and
`aster-mcp` — catalog, 6% inventory budget, single bridge tool with a
target-substitution guard — has no consumer.

**Shape.** `render_index(budget_tokens)` reuses `aster_mcp::estimated_tokens`
and the `ProgressiveConfig::inventory_budget_tokens` pattern. Over budget,
entries keep full name-plus-description in discovery order (project before
global, which `SkillSet::discover` already encodes) until the budget is hit,
then one trailing line: "…and N more; ask the user to trim .aster/skills". No
search tool for skills — one level plus `read_skill` is the whole mechanism.

MCP servers come from `aster.yaml`:

```yaml
mcp:
  servers:
    - name: github
      command: ["npx", "-y", "@modelcontextprotocol/server-github"]
```

The transport is hand-rolled stdio JSON-RPC in `aster-cli/src/mcp.rs` —
initialize, `tools/list`, `tools/call`, roughly 200 lines on tokio process
plus line-delimited framing. Three methods do not justify an SDK dependency.
It implements `aster_mcp::McpInvoker`; at session start the catalog is built,
`Injector::inject` output lands in the system prompt next to the skills index,
and `bridge_tool_definition` registers as one registry tool whose call is
`Injector::handle`. Search and describe answer locally and never prompt;
execute prompts like an outside read, and `Always` persists a per-project MCP
grant (`<config>/aster/grants/<slug>-mcp.json`, reusing the `GrantStore`
shape). Headless denies.

**Done when.** A 100-skill index stays under budget with the overflow line; a
fake in-process invoker proves execute prompts and search/describe never do.

## Phase 8 — Delegation as a contract

**Gap.** `aster-agents` parses `name`, `description`, `model`, `tools`,
`max_rounds`, and `verify`, and nothing reads them.

**Shape.** A `run_agent` tool — `{"agent", "task", "context"?}` — where the
delegation is a contract built entirely from fields `AgentDef` already
parses:

- **Tools:** `Allowlist::Named(def.tools.unwrap_or(DEFAULT_TOOLS))` filters
  the Phase 5 registry at dispatch. `run_agent` is never in a subagent's
  allowlist: depth 1, no recursion, no runaway chains.
- **Rounds:** `def.max_rounds.unwrap_or(8)` replaces `MAX_TOOL_ROUNDS` for
  the sub-loop, same forced finish.
- **Model:** `def.model` through the existing per-call override
  (`complete_tools_stream_with`).
- **Context:** fresh — the agent body plus the tools prompt. No parent
  history, no memory injection. The StateAct result is the reason: a subagent
  with its own narrow context outperforms a parent context dragging
  everything.
- **Return:** structured, fixed by the harness:
  `{"agent", "summary", "rounds_used", "tools_denied", "usage"}`. The parent
  transcript records only the call and this result; the subagent gets its own
  session file with `forked_from_id` pointing at the parent — Phase 1's
  lineage field doing double duty.
- **Verify:** when `def.verify`, one adversarial refute pass patterned on the
  `aster-harness` verify stage, appending a caveat rather than looping. The
  full refute and confidence gate migrates in with the roadmap's trace work.

Subagent events surface on the parent's sink as `{"type":"agent_event"}` so
the TUI can render a nested spinner. `aster run <agent> "<task>"` is the
headless entry, printing the structured result. The `render_index` prompt text
is corrected to name `run_agent`.

One guard, from the Phantom Guardrails result: default policy denies edits
under `.aster/skills/`, `.aster/agents/`, and the global config directory. No
agent rewrites its own harness inputs mid-session.

**Done when.** The roadmap's own criterion — the explorer's `edit_file`
attempt returns a dispatch refusal — plus the round cap honored, the subagent
session file carrying lineage, and the index text matching the registered
tool.

## Phase 9 — Scheduled runs

**Gap.** Nothing in Aster can run without a human at the keyboard.

**Shape.** The OS scheduler is a wheel we do not reinvent — no resident
daemon. Schedules live in `aster.yaml`:

```yaml
schedules:
  - name: nightly-review
    cron: "0 9 * * *"
    agent: reviewer
    task: "review yesterday's commits on main"
```

`aster cron install | list | remove | run <name>`: install generates launchd
plists on macOS and crontab entries on Linux, each invoking `aster run
<agent> "<task>" --json`; `run` executes one immediately for testing. Every
scheduled run records its own session tagged with the schedule name in
`SessionMeta`, subject to Phase 1 retention. Headless semantics apply
unchanged: no approver means prompts deny and `ask_user` declines, so a
scheduled agent is read-only unless its policy says otherwise.

**Done when.** `install` produces a valid plist/crontab entry, `run` records
a tagged session, and a scheduled reviewer completes end to end with zero
prompts.

---

## Sequencing

| Order | Phase | Depends on | Roadmap interaction |
| ----- | ----- | ---------- | ------------------- |
| 1 | Session index, retention, fork | — | none (persist-only) |
| 2 | Session-selection UX | 1 | none |
| 3 | Approval scoping fix | — | none |
| 4 | Memory scoping and budget | — | none |
| 5 | Tool registry | — | is roadmap phase 1 |
| 6 | ask_user, update_plan, plan exit | 5 | precedes sandbox prompting needs |
| 7 | Skills budget, MCP wiring | 5 | registry gives MCP its registration path |
| 8 | run_agent contract | 5, 1 | is roadmap phase 3 |
| 9 | Scheduled runs | 8 | none |

Phases 1–4 ship independently and in parallel with the roadmap's sandbox
work. Phase 5 is the shared hinge. The evidence rule from
[ROADMAP.md](ROADMAP.md) applies: every phase above names its verification,
and a phase that cannot is not worth starting.

## Sources

Mechanisms adapted from: Claude Code
([sessions](https://code.claude.com/docs/en/sessions),
[memory](https://code.claude.com/docs/en/memory),
[sub-agents](https://code.claude.com/docs/en/sub-agents),
[permission modes](https://code.claude.com/docs/en/permission-modes)), Codex
CLI ([session resumption and
forking](https://deepwiki.com/openai/codex/4.4-session-resumption-and-forking),
[sandboxing](https://developers.openai.com/codex/concepts/sandboxing)), the
MCP [elicitation
spec](https://modelcontextprotocol.io/specification/2025-06-18/client/elicitation),
and Anthropic's [code execution with
MCP](https://www.anthropic.com/engineering/code-execution-with-mcp). Evidence:
[arXiv:2607.17598](https://arxiv.org/abs/2607.17598) (one disclosure level),
[arXiv:2607.22798](https://arxiv.org/abs/2607.22798) (StateAct),
[arXiv:2607.13083](https://arxiv.org/abs/2607.13083) (Phantom Guardrails).
