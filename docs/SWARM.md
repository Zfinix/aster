# Sub-agents

The `agent` tool lets a chat turn fan work out to named specialists that run in
parallel and report back. Each sub-agent starts with a fresh context and cannot
see the parent conversation, so every task must be self-contained. The parent
weighs the reports; it does not treat them as verified truth.

The system prompt teaches one dispatch rule: when work is divisible, split it
into distinct, non-overlapping slices and send them as one `agent` call with
one task per slice, so the batch covers the whole. A job that splits cleanly is
never handed to a single agent, and when the user asks to spin up or use
agents, the model fans out even for work it could do itself.

## The roster

Six built-in personas ship in the binary, grouped by category. Each is a
directory under `crates/aster-agents/builtins/<name>/` holding one `AGENT.md`.

| Persona | Category | Edits | Role |
| --- | --- | --- | --- |
| **Scout** | recon | no | Fast read-only reconnaissance: "where does X live", "how does Y work", answered from repository evidence. |
| **Cartographer** | recon | no | Architecture mapping: traces a flow end to end, names module boundaries, recommends where a change should land. |
| **Sentinel** | review | no | Skeptical review: tries to refute a suspected defect before reporting it, and only accepts one with a concrete failure scenario. |
| **Forge** | build | yes | Applies a specific, well-described change with the minimal edit. Edits stay policy-gated and may prompt for approval. |
| **Scribe** | docs | yes | Writes or updates documentation so it matches what the code actually does. Touches docs only, never code. |
| **Prism** | synthesis | no | The expensive pass: merges raw collector reports, resolves conflicts by reading the repo, spot-checks claims, outputs one curated result. |

The intended shape for broad work is collectors first, synthesis second: fan
several cheap Scouts (or Cartographers, Sentinels) out in one call, then pass
their raw reports to Prism in a second call. Prism runs on the session model;
collectors run on the cheaper `agents.collector_model` when one is configured.

The agent index the model sees is generated from the registry and grouped by
category, so custom agents slot into the same listing.

## How a batch runs

One `agent` tool call carries a `tasks` array of `{agent, task}` pairs.
Dispatch works through `crates/aster-cli/src/agents.rs`:

1. One call runs at most `agents.max_per_turn` tasks (default 24). Overflow is
   deferred, not lost: the tool result names the count and tells the model to
   re-send the rest in following `agent` calls, so large jobs run in waves.
   The cap is per call, and it protects report quality: the result budget
   divides across the batch, so each wave keeps readable reports, while
   `max_concurrent` bounds true parallelism anyway.
2. The whole batch is announced up front with one `running` status event per
   task, so UIs can show progress totals immediately.
3. Tasks run concurrently, bounded by `agents.max_concurrent` (default 8), each
   under `agents.agent_timeout_secs` (default 300s). A timeout or error fails
   that task alone; the rest keep running.
4. Each completion emits a `done` or `error` status event as it lands. The tool
   result the model sees preserves input order regardless of completion order,
   and caps each report so the combined result fits the 24k-char tool budget.

An unknown agent name fails its task with `unknown agent: <name>` instead of
poisoning the batch.

### What a sub-agent gets

A sub-agent is the same user in the same session, minus the conversation:

- Its `AGENT.md` body as the system prompt, and only the tools its frontmatter
  allows. Without a `tools` list it gets the read-only set (`read_file`,
  `list_files`, `search_files`, `find_files`, `read_skill`).
- The parent's policy and credential approvals. Out-of-repo write grants are
  not inherited; a sub-agent asks for its own.
- Its model resolves as: the definition's `model`, else
  `agents.collector_model`, else the session model.
- A round budget of its own (`max_rounds`, default 8), and no `agent` tool:
  swarms do not nest.
- No skills, no memory, no MCP servers. The task text is its whole world.

## Stream events

The CLI's `--stream` output carries two swarm event types, consumed by the
editors and the TUI:

```json
{"type": "agent_status", "call_id": "…", "agent": "scout", "task": "…",
 "status": "running" | "done" | "error", "report": "…", "error": "…",
 "done": 1, "total": 5}
```

Identity within a call is `agent` plus `task`, because one batch may run the
same persona several times with different tasks. The `running` events for the
whole batch arrive before any work starts; `report` rides on `done` and
`error` on `error`.

```json
{"type": "agent_activity", "call_id": "…", "agent": "scout", "task": "…",
 "line": "search_files stdio"}
```

Activity is the live feed of what a running sub-agent is doing. The dispatcher
translates the child's own stream into display lines: each tool call becomes a
`name detail` line (`read_file src/chat.rs`), and narration buffers until the
next tool call, then lands as one condensed line. Consumers append lines to
that task's rolling log; the VS Code webview keeps the last 50.

## Rendering

The VS Code panel draws each `agent` call as a card:

- **Swarm**: a wired graph. An orchestrator node fans out to one node per
  task; wires are measured off the rendered nodes and carry a flowing dash
  while that task runs. Each node shows the persona's avatar with a status dot
  on the rim (blue running, green done, red failed), the capitalized name, and
  a subtitle that is the latest activity line while running, the task when
  settled, or the error when failed. Clicking a running node opens its live
  tail; clicking a finished one opens its report, which can also pop out to a
  markdown tab.
- **Solo**: a batch of one skips the graph and header. The card is the node,
  an always-visible live tail while it works, and the report on click.

Avatars are inline SVG picked by persona name (compass, route, shield, hammer,
pencil, sparkle); unknown agents get the generic mark. The `agent` tool call's
own row is hidden once its card exists, so the swarm is stated once.

The TUI renders the same status events as per-agent rows with a progress
count, and prints the curated report when the swarm settles.

## Custom agents

Drop a directory with an `AGENT.md` in either root:

- `<repo>/.aster/agents/<name>/AGENT.md` (project)
- `~/.local/share/aster/agents/<name>/AGENT.md` (global; respects
  `XDG_DATA_HOME`)

Project definitions shadow global ones, and both shadow builtins of the same
name, so overriding Scout is just creating `scout/` in the project root.
Malformed definitions are skipped with a warning, never fatal.

The file is YAML frontmatter over a markdown body; the body becomes the
sub-agent's system prompt.

```markdown
---
name: profiler
description: Profiler, the performance specialist. Read-only. Use to find where time is spent and why.
category: recon
tools: [read_file, list_files, search_files, find_files, read_skill]
max_rounds: 10
---
You are Profiler, this project's performance specialist...
```

| Field | Required | Notes |
| --- | --- | --- |
| `name` | no | Lowercase letters, digits, hyphens; max 64 chars. Defaults to the directory name. |
| `description` | yes | Max 1024 chars. This is what the model reads when choosing an agent, so state the persona, whether it edits, and when to use it. |
| `category` | no | Free-form grouping for the agent index (builtins use `recon`, `review`, `build`, `docs`, `synthesis`). Uncategorized agents list under `other`. |
| `tools` | no | Tool allowlist. Omit for the read-only set. Include `edit_file` to let the agent edit; edits stay policy-gated. |
| `model` | no | Pin a model for this agent. Otherwise `agents.collector_model`, else the session model. |
| `max_rounds` | no | Tool rounds before the agent must answer. Default 8. |
| `verify` | no | Declares the agent's reply should face an adversarial verify pass. Parsed and carried on the definition; the automatic pass is not wired up yet. |

Unknown frontmatter keys are ignored, so newer fields do not break older
binaries.

## Configuration

Fan-out limits live under the `agents` key in `aster.yaml`
(`collector_model`, `max_concurrent`, `max_per_turn`, `agent_timeout_secs`),
each with an env override. See [CONFIG.md](./CONFIG.md#agents).
