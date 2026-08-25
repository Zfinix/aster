# Harness design

This document says how Aster fixes its harness semantics: sessions, memory,
approvals, structured interaction, delegation, scheduling, and background runs.
It sits next to [ROADMAP.md](ROADMAP.md). The roadmap owns the tool registry,
sandbox, dispatch, trace, and eval workstreams. This document owns the UX and
semantics that the roadmap does not cover, and it says where each phase fits
into the roadmap sequence.

The philosophy is simple: we do not re-invent the wheel. Every mechanism below
comes from something proven. Some come from modern harnesses (Claude Code,
Codex CLI, Amp, opencode). Some come from older computer science ideas that
fit agents well: event sourcing for session persistence, capability
attenuation for tool scoping, and contracts for delegation. Where controlled
evidence exists, we follow it:

- One level of progressive disclosure is enough. A second routing level never
  helps and sometimes breaks accuracy
  ([arXiv:2607.17598](https://arxiv.org/abs/2607.17598)). The existing Aster
  skills shape is already correct: an index of name plus description, and the
  body through `read_skill`. It needs a budget, not more depth.
- A fresh subagent per subgoal, with its own finish gate, beats one context
  that carries everything ([StateAct, arXiv:2607.22798](https://arxiv.org/abs/2607.22798)).
- Harnesses that tune their own scaffolds invent failures to fix
  ([Phantom Guardrails, arXiv:2607.13083](https://arxiv.org/abs/2607.13083)).
  Aster never edits its own prompts, skills, or agent definitions during a
  session. Policy enforces this.

## Where the harness is weak today

- `aster sessions list` still reads every transcript to compute titles and
  turn counts ([sessions.rs:145](../crates/aster-cli/src/sessions.rs)).
  `Store::list_sessions` reads headers only
  ([lib.rs:172](../crates/aster-persist/src/lib.rs)). But nobody built the
  SQLite index from Phase 1, so listing cost still grows with history.
- Memory is global across all repos and has no context budget.
  [memory.rs](../crates/aster-persist/src/memory.rs) has no per-project path.
  Phase 4 is not built, so memory can only rot.
- Tool dispatch is a string match in `exec_tool`
  ([chat.rs:2679](../crates/aster-cli/src/chat.rs)) over the whole surface:
  `read_file`, `explore`, `run_command`, `run_tests`, `open_preview`,
  `agent`, and more. It is not a registry. Allowlists filter which
  definitions a sub-agent sees. Nothing refuses a disallowed name at
  dispatch itself.
- Prompts have no versions. Nothing records which prompt text produced a
  transcript, so you cannot tie a regression to a prompt change.
- There is no deterministic replay. You can resume a transcript and seed it
  as history, but you cannot re-run a recorded run step by step.

The phases below fix these problems. Status lines mark what has shipped.
Phases 1 to 4 are persistence and UX work. They can ship in parallel with the
roadmap sandbox workstream. Phase 5 is the roadmap tool-registry phase,
referenced but not redesigned here. Phases 6 to 10 build on it.

---

## Phase 1: Session log plus index, retention, fork

**Status.** Partly shipped. `aster sessions prune --keep/--older-than` covers
deletion ([sessions.rs:48](../crates/aster-cli/src/sessions.rs)). The index,
fork lineage, and `aster.yaml` retention are not built.

**Gap.** JSONL transcripts are already a crash-safe write-ahead log
([transcript.rs](../crates/aster-persist/src/transcript.rs)). But every query
about sessions re-reads the logs, nothing deletes old files, and sessions
have no lineage records.

**Shape.** Use the Codex split: the log stays the truth, and a SQLite index
answers queries. One file at `<config>/aster/sessions.sqlite`, one table:

```sql
sessions(id TEXT PRIMARY KEY, project_slug TEXT, path TEXT,
         created_at TEXT, updated_at TEXT, model TEXT,
         title TEXT, user_turns INTEGER, bytes INTEGER,
         forked_from_id TEXT)
```

The dependency is `rusqlite` with the bundled feature. `aster-persist` is a
synchronous crate and stays one. The index is a cache, never truth.
`Store::reindex` rebuilds it from headers (reuse `read_header`,
[lib.rs:174](../crates/aster-persist/src/lib.rs)). If a queried id is missing
from the index, reindex runs automatically. Deleting the sqlite file loses
nothing.

Fork adds `forked_from_id: Option<String>` to `SessionMeta` with serde
defaults, so `TRANSCRIPT_VERSION` stays compatible at version 1. `Store::fork`
copies the `.jsonl` file, mints a new ULID, and records the lineage.
Compaction is already non-destructive. The `Summary` event appended to the log
is the checkpoint record, so nothing changes there. Compaction is not the only
context mechanism. Within a turn, a reserve-and-evict budget stubs out old
tool results, and every eviction becomes a transcript event
([budget.rs](../crates/aster-cli/src/budget.rs),
[chat.rs:1714](../crates/aster-cli/src/chat.rs)).

Retention comes from `aster.yaml`:

```yaml
sessions:
  retention_days: 30
  max_per_project: 200
```

The sweep runs on `Store::open`. It uses one indexed query and the existing
`delete_session`.

**Done when.** Listing no longer parses transcripts. Deleting the index and
re-listing gives identical rows. The sweep deletes only past-threshold files.
Fork lineage is queryable.

## Phase 2: The TUI never auto-resumes

**Status.** Mostly shipped. The TUI defaults to a new session
(`resume_or_new`, [tui/chat.rs:698](../crates/aster-cli/src/tui/chat.rs)).
`--continue`, `--resume <id>`, `--session`, and the bare `--resume` picker all
work ([chat.rs:830](../crates/aster-cli/src/chat.rs)). `/fork` and the
summary-first resume gate are not built.

**Gap.** Sessions appended forever into one growing file per repo, and the
TUI ignored the `--continue` flag.

**Shape.** The default is always a new session. `tui::run_chat` gets a
`SessionChoice { New, Latest, Id(String) }`, filled from `--continue` and a
new `--resume <id>` flag. This restores parity with the headless contract:
`--session` keeps resume-or-create, `--resume` is resume-only and errors if
the id does not exist, and `--continue` means latest.

In-session, `/resume` opens a picker overlay fed by the Phase 1 index (id,
age, turns, title; cheap now). `/fork` branches the current session through
`Store::fork`. Copy the approval overlay pattern in `tui/chat.rs`.

Resuming a big stale transcript should not silently pay for the whole context
again. When the transcript exceeds `COMPACT_BUDGET_CHARS` and `updated_at` is
more than an hour old, seed history as the latest summary plus the last
`COMPACT_KEEP_TAIL` turns, not the full replay. Make
`SessionTranscript::latest_summary` public. This copies Claude Code's
resume-from-summary gate: cache expiry, not size alone, is the natural
compaction boundary.

**Done when.** A fresh TUI launch creates a second session file instead of
appending to the newest. `--continue` seeds prior history (regression test for
the old bug). The gate test with a synthetic oversized transcript seeds
summary-first.

## Phase 3: "Always" on an edit grants a path, not a mode

**Status.** Shipped. `Always` on an out-of-repo write inserts a
directory-scoped grant ([chat.rs:4115](../crates/aster-cli/src/chat.rs)). The
TUI promotes the mode only when a decision carries no scope. That case is the
deliberate in-repo case
([tui/chat.rs:1750](../crates/aster-cli/src/tui/chat.rs)).

**Gap.** `Answer::Always` on an edit approval switched the whole session to
`Mode::Edit`. One keystroke gave global reach.

**Shape.** `edit_file` currently sends `scope: None`. That is why the TUI
falls back to mode promotion. It will pass `scope =
Some(grant_root(resolved))` instead, and consult a second session-only
`Grants` instance (`edit_grants`, reuse
[grants.rs](../crates/aster-policy/src/grants.rs) unchanged) before prompting.
`Always` inserts into `edit_grants` only and is not persisted. "Stop asking
about this directory for this session" is the right blast radius for a write.
Delete the mode-promotion branch.

**Done when.** Two edits in one directory produce one prompt. The session
mode never changes on `Always`.

## Phase 4: Memory scoped per project, with an enforced budget

**Gap.** One global memory directory shared by every repo, injected in full,
growing forever.

**Shape.** Grants already solve this correctly
([lib.rs:36](../crates/aster-persist/src/lib.rs)): per-project by slug.
Memory follows the same pattern. `<config>/aster/memory/` stays global. Add
`memory/projects/<slug>/` through `Store::project_memory(repo_root)`.
`MemoryStore::new(dir)` is already directory-parameterized, so the whole
change is where the directory comes from. Injection composes global then
project `ASTER.md`. Block indexes merge, and a project block shadows a global
block of the same name. `recall` resolves project first. One disclosure
level, unchanged.

Enforce the budget the way Claude Code enforces its index: measure before
write, and show the failure to the model. Set
`MEMORY_CONTEXT_BUDGET_CHARS = 8_000` for the injected portion and
`MAX_BLOCKS_IN_INDEX = 50`. `remember` and `append_project` compute the
prospective `load_context` size before writing. An over-budget write returns
an error the model sees verbatim: "memory is full (N/8000 chars): consolidate
or delete a block first". No silent truncation. `aster memory` gains
`--global` and `delete <name>`, because a budget that can fill needs a drain.

**Done when.** Two repos have disjoint project memory. A project block
shadows a global block of the same name. An over-budget write errors and
leaves files untouched.

## Phase 5: Tool registry

This is [ROADMAP.md](ROADMAP.md) workstream 6, phase 1, built as specced
there. This document adds one property that later phases depend on: the
allowlist is enforced at dispatch, not only by omitting schemas.

```rust
pub enum Allowlist { All, EditGated(bool), Named(Vec<String>) }
```

`Named` is what `AgentDef::tools` plugs into in Phase 8. A tool absent from
it must fail at `Registry::dispatch`, not merely be hidden from
`Registry::defs`. That is the difference between an allowlist and a
capability: the explorer agent physically cannot call `edit_file`, whatever
the model asks for.

## Phase 6: Structured questions, a visible plan, and a plan-mode handshake

**Status.** Shipped. `ask_user`
([chat.rs:2518](../crates/aster-cli/src/chat.rs)), `update_plan`
(chat.rs:2493), and `exit_plan_mode` (chat.rs:2558) are live when an approver
is present. `exit_plan_mode` appears only while edits are denied.

**Gap.** The model could ask the user only yes/no questions on an approval.
Plan mode was only "edits denied". There was no visible task state.

**Shape.** Three tools, one channel change.

The approval mpsc generalizes to `UiRequest { Approval(ApprovalRequest),
Question(QuestionRequest) }`. The `ask_user` tool takes:

```json
{"header": "≤12 chars", "question": "...", "multiSelect": false,
 "options": [{"label": "...", "description": "..."}]}
```

with 2 to 4 options validated at the schema. The front end always appends an
"Other (type it)" option. The answer is a trichotomy borrowed from MCP
elicitation, because "user said no" and "user walked away" are different
facts: `Accepted(Vec<String>) | Declined | Cancelled`. The tool result is JSON
in-band (`{"outcome":"accepted","choices":[...]}`), never an error. Headless
returns declined immediately. The `--stream` path emits a
`{"type":"question"}` line and reads one reply line, mirroring
`stdio_approver`.

`update_plan` maintains steps with `pending | in_progress | done`, held on
`SessionCtx`, rendered as a TUI header strip, emitted as a `{"type":"plan"}`
NDJSON event. Shared visible state, no extra routing.

`exit_plan_mode` is registered only while the session mode is `Plan`. It
presents the plan as an approval preview. On yes, the mode flips to
`stricter(configured, Manual)` (reuse `Mode::stricter`) and tool definitions
re-issue next round so `edit_file` appears. On no, the model revises the plan.
Headless declines. Plan mode stops being vibes and becomes a handshake.

**Done when.** Question answered, declined, and cancelled paths are tested.
Headless `ask_user` declines. `exit_plan_mode` is absent from definitions
outside `Plan`. A call with 1 or 5 options is rejected.

## Phase 7: Skills index budget, and the MCP bridge finally wired

**Status.** Shipped for MCP: the runtime speaks stdio, streamable-http, and
sse transports ([mcp.rs:928](../crates/aster-cli/src/mcp.rs)) and applies
tool-level filters across servers (mcp.rs:885). The skills index caps each
entry's description ([lib.rs:280](../crates/aster-skills/src/lib.rs)) but does
not budget the whole index.

**Gap.** The skills index grew linearly with skill count with no cap.
`aster-mcp` (catalog, 6% inventory budget, single bridge tool with a
target-substitution guard) had no consumer.

**Shape.** `render_index(budget_tokens)` reuses `aster_mcp::estimated_tokens`
and the `ProgressiveConfig::inventory_budget_tokens` pattern. Over budget,
entries keep full name plus description in discovery order (project before
global, which `SkillSet::discover` already encodes) until the budget is hit,
then one trailing line: "…and N more; ask the user to trim .aster/skills".
No search tool for skills. One level plus `read_skill` is the whole mechanism.

MCP servers come from `aster.yaml`:

```yaml
mcp:
  servers:
    - name: github
      command: ["npx", "-y", "@modelcontextprotocol/server-github"]
```

The transport is hand-rolled stdio JSON-RPC in `aster-cli/src/mcp.rs`:
initialize, `tools/list`, `tools/call`, roughly 200 lines on tokio process
plus line-delimited framing. Three methods do not justify an SDK dependency.
It implements `aster_mcp::McpInvoker`. At session start the catalog is built,
`Injector::inject` output lands in the system prompt next to the skills index,
and `bridge_tool_definition` registers as one registry tool whose call is
`Injector::handle`. Search and describe answer locally and never prompt.
Execute prompts like an outside read, and `Always` persists a per-project MCP
grant (`<config>/aster/grants/<slug>-mcp.json`, reusing the `GrantStore`
shape). Headless denies.

**Done when.** A 100-skill index stays under budget with the overflow line. A
fake in-process invoker proves execute prompts and search/describe never do.

## Phase 8: Delegation as a contract

**Status.** Shipped as the `agent` tool
([chat.rs:4198](../crates/aster-cli/src/chat.rs)). It takes a `tasks` array
and fans the batch out to sub-agents in parallel, capped per turn. `aster run`
and the verify pass are not built.

**Gap.** `aster-agents` parsed `name`, `description`, `model`, `tools`,
`max_rounds`, and `verify`, and nothing read them.

**Shape.** An `agent` tool taking `{"tasks": [{"agent", "task"}]}`, where
each delegation is a contract built entirely from fields `AgentDef` already
parses:

- **Tools:** the allowlist (`def.tools`, defaulting to `DEFAULT_TOOLS`)
  filters the definitions the sub-loop sees
  ([agents.rs:97](../crates/aster-cli/src/agents.rs)). `agent` is registered
  only when the session is not itself a sub-agent (chat.rs:1735): depth 1, no
  recursion, no runaway chains. Refusing a disallowed name at dispatch itself
  still waits on the Phase 5 registry.
- **Rounds:** `def.max_rounds.unwrap_or(8)` caps the sub-loop
  (agents.rs:111), same forced finish. The loop's no-progress guard applies
  too: identical, all-error, and lookup-only rounds are counted and the model
  is steered before the cap is reached
  ([chat.rs:1509](../crates/aster-cli/src/chat.rs)).
- **Model:** `def.model` overrides the child client's model (agents.rs:89).
- **Context:** fresh. The agent body plus the tools prompt. No parent
  history, no memory injection. The StateAct result is the reason: a subagent
  with its own narrow context outperforms a parent context dragging
  everything.
- **Return:** each agent hands back a report, and the parent sees one tool
  result carrying the capped reports (chat.rs:4228). The subagent session
  file with `forked_from_id` pointing at the parent is not built; it waits on
  Phase 1's lineage field.
- **Verify:** not built. When it lands, `def.verify` runs one adversarial
  refute pass patterned on the `aster-harness` verify stage, appending a
  caveat rather than looping.

Subagent events surface on the parent's sink as `{"type":"agent_status"}` so
the UIs can render live per-agent progress. `aster run <agent> "<task>"` as
the headless entry is not built. The `render_index` prompt text names `agent`
and matches the registered tool
([registry.rs:59](../crates/aster-agents/src/registry.rs)).

One guard, from the Phantom Guardrails result: default policy denies edits
under `.aster/skills/`, `.aster/agents/`, and the global config directory. No
agent rewrites its own harness inputs during a session.

**Done when.** The roadmap's own criterion holds: the explorer's
`edit_file` attempt returns a dispatch refusal. Plus the subagent session file
carries lineage. The round cap and the index text matching the registered tool
are already in.

## Phase 9: Scheduled runs

**Gap.** Nothing in Aster can run without a human at the keyboard.

**Shape.** The OS scheduler is a wheel we do not re-invent, so there is no
resident daemon. Schedules live in `aster.yaml`:

```yaml
schedules:
  - name: nightly-review
    cron: "0 9 * * *"
    agent: sentinel
    task: "review yesterday's commits on main"
```

`aster cron install | list | remove | run <name>`: install generates launchd
plists on macOS and crontab entries on Linux. Each entry invokes `aster run
<agent> "<task>" --json`. `run` executes one immediately for testing. Every
scheduled run records its own session tagged with the schedule name in
`SessionMeta`, subject to Phase 1 retention. Headless semantics apply
unchanged: no approver means prompts deny and `ask_user` declines, so a
scheduled agent is read-only unless its policy says otherwise.

One-shot deferral rides the same machinery: `aster defer "18:00" "<task>"`
(or `"+45m"`) installs a self-removing entry that fires once, runs the task
as a tagged session, and uninstalls itself. Deferring is scheduling with a
count of one, not a second subsystem.

**Done when.** `install` produces a valid plist or crontab entry. `run`
records a tagged session. A scheduled reviewer completes end to end with zero
prompts.

## Phase 10: Background agents

**Gap.** Phase 8 delegation blocks: `agent` holds the parent turn open until
the batch finishes, and `aster run` would occupy the terminal. Phase 9 covers
work with nobody at the keyboard. Neither covers the case in between, where
you want a long job started and want to keep working while it runs.

**Shape.** A background agent is a Phase 8 delegation whose result is
collected later instead of awaited. Everything about the contract is
unchanged; only the timing and the reporting differ.

- **Spawn.** The `agent` tool gains `background: true` and returns a handle
  immediately rather than the result: `{"agent", "run_id", "status":
  "running"}`. `aster run <agent> "<task>" --detach` is the headless entry,
  printing the run id.
- **Identity.** Each background run gets its own session file with
  `forked_from_id` pointing at the parent, the same lineage field Phase 1
  already writes. A background run is a session, so `aster sessions list`,
  retention, and `--resume` all apply to it without new machinery.
- **Reporting.** Progress is condensed, never a live transcript. A background
  run renders as one transcript cell that rewrites in place:

  ```text
  • explorer · 5m 16s · running
    └ Thought for 4s, read 1 file, ran 1 shell command
    └ Update(crates/aster-cli/src/tui/bottom_pane/model_picker.rs) +2 −1
  ```

  Thinking collapses to a duration, tool activity to a count per kind, and
  writes to a path with a line delta. The rule: a background agent costs the
  reader a constant number of rows no matter how long it runs.
- **Completion.** A finished run surfaces as a summary cell at the next turn
  boundary, never mid-stream. Interleaving another agent's output into a
  reply the user is reading is the one thing that makes concurrency
  unreadable. The summary is the Phase 8 structured return rendered as
  **Changes** (per file, with deltas) and **How it works**, which is what a
  reader needs to decide whether to keep the work.
- **Collection.** The parent reads a finished run with `agent_result(run_id)`.
  Until then the result is not in the parent's context at all, which is the
  point: ten background runs cost the parent ten handles, not ten
  transcripts.
- **Approvals.** A background run has no approver. Headless semantics from
  Phase 9 apply unchanged: prompts deny, `ask_user` declines, so a background
  agent is read-only unless its own policy grants otherwise. Queueing an
  approval behind a run the user has stopped watching would stall it
  invisibly, so it is refused instead.
- **Isolation.** A background agent that writes gets its own git worktree,
  shared with the roadmap's workstream 4. Two agents writing the same tree
  concurrently is the failure this exists to prevent.
- **Lifecycle.** `aster agents list | stop <run_id> | result <run_id>`, and
  `/agents` in the TUI. Leaving the TUI with runs in flight prompts once and
  either stops them or detaches them to finish on their own. A run whose
  process dies leaves its session file, so the partial work is recoverable
  rather than lost.
- **Concurrency.** A cap from `aster.yaml`, defaulting low. Depth stays 1:
  a background agent cannot spawn another, same as Phase 8.

**What the literature says.** Every choice above has published evidence
behind it, and two of them are settled by it rather than by taste.

- **Concurrent agents conflict, and cross-agent pairs conflict worst.** In
  33,596 agent-authored PRs across 2,807 repositories, 40.2% of repositories
  contain temporally overlapping agent PR pairs, rising to 53.4% within a
  one-week window. Replaying real three-way merges on 747 pairs, cross-agent
  pairs textually conflicted at 41.7% against 19.8% for pairs from the same
  agent, with non-overlapping confidence intervals; 84.4% of conflicted files
  were source rather than dependency manifests, and nearly 42% of conflicts
  were structural (modify/delete, add/add)
  ([arXiv:2607.04697](https://arxiv.org/abs/2607.04697)). Worktree isolation
  is not a precaution here. It is the measured failure mode.
- **Integration cost belongs to the repository, not the agent.** Across
  930,000+ agent PRs, roughly half the variance in integration friction stays
  with the repository after controlling for contribution, author, size, and
  agent. Agent-authored contributions concentrate that friction about twice
  as much as human ones (ICC 0.30 against 0.16)
  ([arXiv:2606.28235](https://arxiv.org/abs/2606.28235)). A concurrency cap is
  therefore a repository-level setting, which is why it lives in `aster.yaml`
  rather than in an agent definition.
- **Asynchronous review detects; only synchronous review prevents.** Comparing
  monitors of coding agents across both deployment modes: reviewing after the
  fact still catches most sabotage (an untrained git-diff monitor misses 11.6%
  of attempts at a 1% false-positive budget, a structural monitor 3.5%, a
  trained one under 1%), but a monitor that runs synchronously and blocks
  before execution or merge drove joint task-plus-covert success from 74.4% to
  0.0% with no measurable loss of legitimate completion
  ([arXiv:2607.14570](https://arxiv.org/abs/2607.14570)). The gap is not
  detection rate. An async monitor has nothing left to stop. Nobody watches a
  background run while it works, so its output is gated at integration and it
  stays read-only by default.
- **Unbounded loops are the failure mode of unattended runs.** A static
  analysis of 6,549 agent repositories confirmed 68 infinite agentic loops
  across 47 projects, whose consequence is cost exhaustion and unbounded
  context growth ([arXiv:2607.01641](https://arxiv.org/abs/2607.01641)). The
  round cap, the depth-1 rule, and the concurrency cap are all bounds on this.
- **Oversight should thin out, not stop.** Placing human oversight stages in a
  long workflow is an optimization problem. The optimal schedule spaces them
  with non-decreasing gaps rather than uniformly
  ([arXiv:2607.16530](https://arxiv.org/abs/2607.16530)). Turn-boundary
  reporting approximates this: attention is cheap early, expensive later.
- **Stale file snapshots are the concurrency bug agents hit.** Append-only
  trajectories pin file contents at read time, so concurrent edits leave the
  agent reasoning about a file that no longer exists in that form; keeping a
  synchronized registry and injecting current contents instead cut input
  tokens 9-50% and reasoning cycles up to 37%
  ([arXiv:2607.22711](https://arxiv.org/abs/2607.22711)). A parent with
  background writers running is exactly this situation.
- **Orchestrator-guided parallelism beats a fixed fan-out.** A shepherd agent
  steering a population of search agents, each on its own git branch, matched
  or beat state-of-the-art on 13/15 open-ended tasks, and adapting parallelism
  by depth beat fixed serial or parallel scaling
  ([arXiv:2607.02807](https://arxiv.org/abs/2607.02807)). The cap should
  eventually be adaptive rather than a constant.
- **Structural enforcement beats instructions.** Twelve parallel agents stayed
  reproducible because deterministic verifier scripts enforced isolation and
  immutability in code that fails loudly, not in prose the agents were asked
  to follow, at about 1% wall-clock overhead
  ([arXiv:2606.27416](https://arxiv.org/abs/2606.27416)). This is also why
  tool scoping is an allowlist rather than a prompt.

Two designs worth tracking rather than adopting yet. **Claim Plane** treats
concurrent change as a pre-write admission problem: workers declare a typed
`ChangeIntent` before implementing, and a deterministic control plane admits
compatible intents and serializes overlap
([arXiv:2607.21909](https://arxiv.org/abs/2607.21909)). It is the right shape
for the merge problem above, but its evaluation is six pairs and the author
states the sample is too small for comparative claims. **MAST** gives 14
failure modes over 1,600+ annotated multi-agent traces
([arXiv:2503.13657](https://arxiv.org/abs/2503.13657)), and a protocol layer
built on it cut failures up to 69.6% on function-level development
([arXiv:2510.12120](https://arxiv.org/abs/2510.12120)). MAST is the taxonomy
to classify background-run failures against once there are enough to count.

**Done when.** A background explorer runs to completion while the parent
answers two unrelated questions. Its progress cell never grows past its row
budget. Its result arrives at a turn boundary rather than mid-reply. A
parallel writing agent's diff lands in its own worktree.

**Evidence it leaves behind.** Parent context cost of N background runs
against N inline delegations on the same tasks, the wall-clock saving against
running them in sequence, and the observed conflict rate on concurrent writing
runs against the 19.8% same-agent baseline above.

## Phase 11: Loops and goals

**Gap.** Phase 9 runs a task on a clock and Phase 10 runs one in the
background, but nothing in Aster repeats. A task that needs polling ("check
the deploy every 5 minutes") or convergence ("keep going until the tests
pass") ends after one turn, and the human is the loop.

**Shape.** Two primitives with different stop conditions, both session-side,
learned from what shipped elsewhere. Claude Code splits the same ground into
`/loop` (re-run on an interval, self-paced when no interval is given) and
`/goal` (re-run until a cheap separate model judges a condition met).
ChatGPT's scheduled tasks and Cursor's Automations are the unattended cloud
versions. The Ralph loop is the community's proof that a bash `while` around
a fresh-context agent converges on large jobs.

- **`/loop <interval> <task>`** in the TUI: re-send the task as a new turn
  each time the interval elapses, firing only between turns, never mid-turn,
  with no catch-up for missed fires. With no interval, the model picks the
  next delay itself at the end of each iteration and may stop the loop when
  the work is done. `/loop list` and `/loop stop` manage them. Loops are
  session state: they die with the session and are capped by a fire budget
  and an expiry, so a forgotten loop cannot spend forever.
- **`/goal <condition>`** in the TUI: after each turn, the condition and the
  transcript go to the collector model, which returns met, not yet, or
  impossible, with a reason. Not yet starts the next turn with the reason as
  guidance; met and impossible end the loop and say why. The judge being a
  different, cheap model from the worker is the point: the implementer does
  not certify its own work, which is Aster's review thesis applied to
  completion. Guardrails: several consecutive turns without a tool call stop
  the loop, unrecoverable errors clear the goal, and the condition may carry
  its own budget clause ("or stop after 20 turns").
- **`aster loop "<task>" --until "<condition>" [--max-runs N]`** headless:
  the Ralph shape. Each iteration is a fresh session that reads the task and
  any plan file from disk, works one turn to the round cap, and ends with the
  same judge. Fresh context per iteration is deliberate: the loop converges
  by re-reading the repo, not by dragging a growing transcript. Composes
  with Phase 9: a schedule entry may invoke `aster loop`, which is how
  "every night, keep fixing until the suite is green" is spelled.

Headless semantics apply unchanged everywhere: no approver means prompts
deny, so an unattended loop is read-only unless policy grants otherwise.

**Rendering.** A loop the user cannot see is a loop the user cannot trust,
so the display is part of the contract, not a follow-up. Everything below is
fed by stream events (`loop_status`, `goal_verdict`), the same way
`agent_status` feeds the swarm card: the CLI emits state, each surface draws
it.

- **Standing state.** While a loop or goal is active, the status area
  carries one line that answers "what is running and when does it act
  next": `◎ goal · 14m · 5 turns · not yet: two auth tests still red`, or
  `↻ loop · every 5m · next fire 3m 10s`. Esc cancels the pending fire;
  the countdown makes Esc mean something.
- **Iteration cells.** Each fire or goal turn renders as one compact
  transcript cell: iteration number, duration, and the verdict with its
  reason, colored by outcome. A quiet iteration ("no change") does not get
  a fresh cell: consecutive quiet fires collapse into a streak row that
  rewrites in place, so an overnight loop costs the reader rows for what
  happened, not rows for time passing.

  ```text
  ↻ #7 · 41s · fixed test_login_expiry, 2 files
  ↻ #8-#12 · quiet · suite still red: test_refresh_token
  ◎ met · 1h 12m · 13 turns · all tests in test/auth pass
  ```

- **Panel card.** The VS Code webview gets a loop card in the swarm card's
  design language: a header with the condition or cadence and the live
  countdown, then an iteration timeline. One row per fire with a verdict
  dot (running, not yet, met, impossible/failed), duration, and the
  judge's one-line reason, expandable to that iteration's turn. Met and
  impossible close the card with a terminal row stating the verdict and
  the totals (turns, time, tokens).
- **Lists.** `/loop list` in the TUI and the panel render the same table:
  id, cadence or condition, next fire, fires remaining before the cap.
  `aster cron list` prints installed schedules with their next run.
  Sessions started by a schedule or a headless loop carry their origin tag
  in `SessionMeta`, so `aster sessions list` already answers "what ran
  last night" with no new machinery.
- **Headless.** `aster loop` prints one line per iteration (number,
  verdict, reason) and exits nonzero on impossible or cap-death, so a
  scheduled loop is scriptable and its outcome greppable from the log.

**Done when.** A `/loop 5m` fires between turns and survives nothing it
should not. A `/goal` on a failing test suite runs unattended to green and
the judge's verdicts are visible per turn. `aster loop --until` converges on
a seeded multi-defect repo within its run budget. Every loop form dies on
its cap instead of running forever. An overnight loop's transcript costs the
reader a constant number of rows per event, not per fire.

**Evidence it leaves behind.** Turns and tokens to convergence against a
single long turn on the same task, the judge's false-met and false-impossible
rates on seeded conditions, and how often the no-progress guard fires.

---

## Sequencing

| Order | Phase | Depends on | Roadmap interaction |
| ----- | ----- | ---------- | ------------------- |
| 1 | Session index, retention, fork | - | none (persist-only) |
| 2 | Session-selection UX | 1 | none |
| 3 | Approval scoping fix | - | none |
| 4 | Memory scoping and budget | - | none |
| 5 | Tool registry | - | is roadmap phase 1 |
| 6 | ask_user, update_plan, plan exit | 5 | precedes sandbox prompting needs |
| 7 | Skills budget, MCP wiring | 5 | registry gives MCP its registration path |
| 8 | agent contract | 5, 1 | is roadmap phase 3 |
| 9 | Scheduled runs | 8 | none |
| 10 | Background agents | 8, 1 | needs roadmap 4 worktrees to write |
| 11 | Loops and goals | 9 (headless form), none (TUI forms) | none |

Phases 1 to 4 ship independently and in parallel with the roadmap sandbox
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

Phase 10 evidence: [arXiv:2607.04697](https://arxiv.org/abs/2607.04697)
(concurrent agent PRs and merge conflict rates),
[arXiv:2606.28235](https://arxiv.org/abs/2606.28235) (integration friction is
repository-level), [arXiv:2607.14570](https://arxiv.org/abs/2607.14570)
(synchronous against asynchronous monitoring),
[arXiv:2607.01641](https://arxiv.org/abs/2607.01641) (infinite agentic loops),
[arXiv:2607.16530](https://arxiv.org/abs/2607.16530) (oversight placement),
[arXiv:2607.22711](https://arxiv.org/abs/2607.22711) (CORVUS, stale
trajectory snapshots), [arXiv:2607.02807](https://arxiv.org/abs/2607.02807)
(SwarmResearch), [arXiv:2606.27416](https://arxiv.org/abs/2606.27416) (Glite
ARF, verifier-driven parallel agents),
[arXiv:2607.21909](https://arxiv.org/abs/2607.21909) (Claim Plane,
preliminary), [arXiv:2503.13657](https://arxiv.org/abs/2503.13657) (MAST) and
[arXiv:2510.12120](https://arxiv.org/abs/2510.12120) (SEMAP).

Phase 11 mechanisms adapted from: Claude Code
([scheduled tasks and /loop](https://code.claude.com/docs/en/scheduled-tasks),
[/goal](https://code.claude.com/docs/en/goal)), ChatGPT scheduled tasks,
Cursor Automations, GitHub Copilot cloud-agent schedules, and the
[Ralph loop](https://ghuntley.com/ralph/) (fresh-context iteration until a
verifiable done state).