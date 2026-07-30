# Roadmap

What Aster becomes next, and why. Each workstream states the gap, the shape of
the fix, and what "done" looks like.

Today the harness can read a repo, retrieve evidence, call a model, verify its
own output, and gate file writes through policy. It cannot run anything, cannot
dispatch the agents it already parses, cannot measure itself over time, and
cannot tell you which layer failed when a run goes wrong. Those four gaps are
the roadmap.

The harness UX and semantics work — session resume and retention, memory
scoping, structured questions, plan mode, delegation contracts, scheduled
runs, background agents — is designed in [HARNESS.md](HARNESS.md) and slots
around the sequencing below.

---

## 1. `aster-sandbox`: isolated execution

**Status.** The crate exists and `run_command` is integrated into the chat tool
loop. Backend detection (Seatbelt, bubblewrap, process-level fallback), profile
compilation, secret filtering, and timeout enforcement are implemented. Remaining:
an escape-test suite and the platform-tradeoffs writeup.

**Gap.** Aster has no execution tool. The tool surface is read, list, search,
edit, remember, recall, read_skill ([chat.rs](../crates/aster-cli/src/chat.rs)).
The only subprocesses are `git` and `semgrep`, both spawned directly with no
isolation. An agent that cannot run the test suite cannot verify its own fix,
which caps every implementation workflow at "probably compiles".

**Shape.** A new crate owning process execution, with platform backends behind
one trait:

```text
aster-sandbox/
  lib.rs        Sandbox trait: spawn(Command, Limits) -> Handle
  profile.rs    fs read/write allowlists, net policy, env allowlist, limits
  seatbelt.rs   macOS: sandbox-exec profile generation
  landlock.rs   Linux: Landlock LSM path rules + seccomp-bpf syscall filter
  nsjail.rs     Linux fallback: user namespaces + cgroups v2 when LSM absent
  passthrough.rs  explicit opt-out, never the default
```

A `Profile` is compiled from `aster.yaml` the same way `Policy` is compiled from
`permissions:`, and carries:

- filesystem: readable roots, writable roots (repo + a scratch dir), everything
else denied
- network: off by default, allowlist by host when a task needs a package fetch
- process: wall-clock timeout, memory cap, max spawned children, no new privs
- environment: allowlisted vars only, secrets never inherited

**Policy integration.** `Action` gains an `Exec { command, profile }` variant so
execution goes through the same `Decision` path as edits, including `Mode::Ask`
prompting in the TUI and denial when headless. Policy decides *whether*; the
sandbox decides *within what bounds*. Keeping those separate matters: a
permitted command still runs confined.

**Tools it unlocks.** `run_command`, then `run_tests` as a typed wrapper that
parses results into structured output instead of raw stdout.

**Done when.** A fix run can edit a file, run the project's test command inside
the sandbox, read the failure, and iterate, with an escape attempt (write
outside the repo, unlisted network call, fork bomb, timeout overrun) failing
closed on both macOS and Linux and covered by tests.

**Evidence it leaves behind.** A public escape-test suite green on both
platforms in CI; a writeup of the backend tradeoffs (Landlock over seccomp-only,
where Seatbelt's undocumented behavior bites, why an absent LSM refuses to run
rather than degrading to passthrough); and a measured per-spawn overhead
number.

---

## 2. `aster-eval`: measurement as infrastructure

**Gap.** [eval_models.rs](../crates/aster-harness/examples/eval_models.rs) is
a good benchmark and the wrong shape. It hardcodes one fixture, one set of
planted bugs, one axis (model choice), and prints to stdout. Nothing persists,
so nothing regresses visibly.

**Shape.** Promote it to a crate with the fixture, the runner, and the scoring
separated:

- **Fixture format.** A case is a directory: a repo snapshot or diff, a task
prompt, and an expectation file (planted defects with location and keyword
signatures, or an assertion command that must exit zero after a fix run).
- **Runner.** Executes a case against a *configuration*, not just a model. A
configuration is the full tuple: model, system prompt version, tool set,
retrieval settings, verify concurrency, confidence threshold. That is what
makes it an ablation harness rather than a model comparison.
- **Scoring.** Recall and precision against planted defects for review cases;
assertion exit status for fix cases; plus latency, token counts, and cost
pulled from the existing `UsageSnapshot`
([aster-ai/src/lib.rs:227](../crates/aster-ai/src/lib.rs#L227)).
- **Storage.** Results into SQLite next to the index, keyed by config hash and
git SHA, so `aster eval compare <sha> <sha>` shows movement.

**CLI.** `aster eval run --suite review --config configs/*.yaml`, and
`aster eval ablate --vary verify_model,min_confidence` to sweep one axis while
holding the rest fixed.

**Done when.** CI runs a small suite on every PR touching prompts or the harness
and comments the delta in recall, cost, and p50 latency.

**Evidence it leaves behind.** A public fixture suite anyone can run, and
published ablation results: verify stage on vs off, confidence-threshold sweep,
retrieval-budget sweep, cheap-hypothesis plus expensive-verify vs a single
strong model. Numbers with the harness that produced them.

---

## 3. `aster-trace`: attribution across the stack

**Gap.** Diagnostics are `tracing::debug!` lines. When a run produces a bad
result there is no way to answer the only question that matters: was that the
harness, the prompt, the model, or the provider?

**Shape.** Structured spans over a run tree, emitted to a JSONL file per session
alongside the existing transcript in [aster-persist](../crates/aster-persist/):

```text
run
  stage: hypothesize   model, tokens in/out, latency, retries, finish_reason
  stage: retrieve      queries issued, hits, bytes fed to the model
  stage: verify        per candidate, verdict, confidence, refute reason
  tool:  read_file     args digest, result bytes, truncated?, duration
  tool:  run_command   exit code, sandbox denials, duration
```

Each span carries a failure class so failures are attributable rather than
merely logged: `provider_error`, `schema_violation` (model returned unparseable
output), `context_overflow`, `tool_error`, `policy_denied`, `sandbox_denied`,
`timeout`.

**Surfacing.** `aster trace <session>` renders the tree with token and time
attribution per stage. Feed the same records into `aster-eval` so an eval
regression points at the stage that moved.

**Done when.** A failed run can be classified into one of those buckets without
re-running it, and truncated tool results and context-overflow compactions are
both visible as events rather than silent.

**Evidence it leaves behind.** A failure taxonomy over a few hundred real runs
with the split published: how much of what reads as model failure is actually
context overflow, schema violation, or provider error. Nobody publishes this
breakdown.

---

## 4. Long-horizon tasks: orchestration and durable state

**Gap.** [chat.rs](../crates/aster-cli/src/chat.rs) is a bounded loop with a
round cap that ends in "stop using tools and answer now", and compaction that
summarizes the head of the history. That is correct for chat and insufficient
for a task that spans hours.

**Shape.**

- **Task graph.** A run decomposes into steps with explicit state persisted
after each one, so a killed process resumes rather than restarts. The existing
`SessionCtx` record stream is most of the write-ahead log already; it needs a
resumable reader.
- **Subagents.** Fan out an isolated context per unit of work (per file, per
finding, per test failure) and return structured results to the parent. The
verify stage already fans out with bounded concurrency
([lib.rs:63](../crates/aster-harness/src/lib.rs#L63)); generalize that into a
reusable primitive instead of one hardcoded stage.
- **Worktree isolation.** Parallel agents that write need separate git
worktrees, then a merge or reject step. This pairs with the sandbox: each
worktree is a separate writable root.
- **Context budget manager.** Replace character-count compaction with a real
budget: reserve for system prompt, skills, memory, and tool results, evict by
policy rather than by position, and record every eviction as a trace event.

**Done when.** A multi-file fix task survives a process kill and resumes, and
parallel subagents on separate worktrees produce a reviewable combined diff.

**Evidence it leaves behind.** A recorded kill-and-resume of a multi-hour task,
and measured wall-clock and token cost for parallel fan-out against the serial
baseline on the same task.

---

## 5. `aster-agents`: definitions with dispatch

**Status.** The crate exists. `AgentDef` parsing, `AgentRegistry::discover`
(project → user → built-in roots), and built-in explorer/reviewer/fixer agents
are done. Dispatch (`aster run`, `run_agent` tool) is not yet wired — the
registry renders an index that references an `agent` tool that does not exist.

**Gap.** [aster-agents](../crates/aster-agents/) parses `AGENT.md`
frontmatter, resolves project over user over builtin roots, and renders an
index. Nothing consumes it. No other crate depends on it, so `model`, `tools`,
`max_rounds`, and `verify` on
[AgentDef](../crates/aster-agents/src/def.rs) are declared and never honored.
Agents exist on disk and cannot be run.

**Shape.**

- **Dispatch.** `aster run <agent> "<task>"` and a `run_agent` tool that lets a
parent agent delegate. Each dispatch builds a fresh context: the agent's body
as the system prompt, its `model` override through
`complete_tools_with`, its `max_rounds` as the loop cap.
- **Tool scoping.** `AgentDef::tools` becomes the actual allowlist the tool
registry filters on, so an `explorer` agent physically cannot edit. This is
the second half of the policy story: policy bounds the *user's* session,
agent scoping bounds a *delegated* one.
- **Sandbox profile per agent.** An agent that can run commands declares which
profile it gets. A `fixer` may run the test suite; an `explorer` may not run
anything.
- **Structured returns.** A delegated agent returns a typed result to its
parent, not a wall of transcript. The parent sees a summary and named
artifacts, which is what keeps fan-out from destroying the parent's context.
- **Isolation.** Concurrent agents that write get separate worktrees, shared
with workstream 4.
- **Verification.** `AgentDef::verify` routes the agent's output through the
existing refute-and-confidence-gate path rather than returning first-draft
output.

**Done when.** `aster run explorer "where is auth handled"` works, a parent
agent can delegate to it, the explorer's attempt to edit is refused by scoping
rather than by prompt instruction, and each delegated run appears as its own
subtree in the trace.

**Evidence it leaves behind.** A count of scoping violations refused that a
prompt instruction alone would have allowed, and the measured parent-context
saving from structured returns against raw transcript return.

---

## 6. Harness primitives and surface

Cross-cutting work that the above depends on.

- **Tool registry.** Tool schemas are inline JSON in `tool_defs`, and dispatch
is a `match` on a string in `exec_tool`. Extract a `Tool` trait with schema,
policy action, and handler so MCP tools, builtin tools, and future sandboxed
tools register through one path and appear identically in traces and evals.
- **Prompt versioning.** [prompts.rs](../crates/aster-harness/src/prompts.rs)
should carry a version identifier that lands in every trace and eval record.
Prompt changes are the most common cause of quality movement and currently the
least attributable.
- **Headless protocol.** A stable JSON event stream on stdout so CI, the VS Code
extension, and the desktop app consume one interface. The event shapes emitted
in `chat.rs` are the seed; they need a documented schema and a version field.
- **Deterministic replay.** Record provider responses per run and replay them, so
harness changes can be tested without spending tokens and without model
nondeterminism. This is what makes the eval suite cheap enough to run per PR.

**Evidence it leaves behind.** A documented, versioned event schema other tools
can implement against, and byte-identical reruns of a recorded session at zero
token cost.

---

## Sequencing

| Phase | Work                             | Why first                                                      |
| ----- | -------------------------------- | -------------------------------------------------------------- |
| 1 | Tool registry, prompt versioning | Everything downstream registers or records through these |
| 2 | `aster-sandbox` + `run_command` | Unblocks verification-by-execution, the largest capability gap |
| 3 | Agent dispatch and tool scoping | Cheap once the registry exists, and it makes `aster-agents` real |
| 4 | `aster-trace` | Needed to debug phases 2, 3, and 6 |
| 5 | `aster-eval` + replay | Turns 1 to 4 into measured movement instead of vibes |
| 6 | Task graph, delegation, worktrees | Long-horizon work, safe only once 2 to 4 exist |
| 7 | Context budget manager | Refines phase 6 under real workloads |

Phase 2 is the highest-value single addition: it converts Aster from an agent
that reads code into an agent that can prove its own work.

## The evidence rule

Every workstream states what it leaves behind for someone who will never open
this repository: a test suite, a number, or a writeup. Code in a crate is not
evidence of anything on its own. Two workstreams finished to that standard are
worth more than seven half-built, so an epic that cannot name its artifact is
not worth starting.
