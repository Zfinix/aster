# Aster Skill Compiler

Status: draft v0 spec
Target: `aster-distill` crate + integration into `aster-agents`, `aster-cli`
Date: 2026-08-25

## Thesis

Every agent re-derives the same task from scratch, every time, forever. Aster
already records everything needed to stop doing that: append-only session
transcripts with full tool calls and per-event token usage. The skill compiler
closes the loop. It mines those transcripts for repeated successful procedures,
compiles them into executable, parameterized skills with built-in verification,
admits them only through a hard test gate, and routes repeat tasks to the
compiled skill with the model as fallback. The result is measurable: the same
task gets cheaper and faster the more Aster is used, and the curve is the demo.

The research context makes this the right wedge. The 2026 skill-library
literature (SkillEvolBench, GRASP, PANDO, SkillBrew, SGDR) converged on two
findings: distilling trajectories into prose loses the procedural detail that
made them work (raw-trajectory replay beats text skills), and skill writing
without a validation gate is no better than no skills at all. Every published
system distills to text. Nobody ships trajectories compiled to verified code.
This spec is that system.

Design rules taken from those published failure modes:

| Rule | Source finding | Mechanism here |
|---|---|---|
| Compile to code, not prose | SkillEvolBench: text abstraction is lossy | `run.sh` + `check.sh` packages |
| Gate every admission | GRASP: ungated skills are worthless | replay gate, holdout, zero regression budget |
| Raw episodes are evidence | "Useful Memories Become Faulty": consolidation destroys | transcripts never touched; candidates reference them |
| State-grounded routing | SGDR: task-level retrieval misfires | skills surface as tools; the model routes per state |
| Bounded curated library | SkillBrew: append-only banks rot | hard cap + utility-based eviction |
| Prove the efficiency claim | PANDO's metrics | tokens/latency saved, repetition rate, per-skill stats |

## What exists already (do not rebuild)

- `aster-persist::transcript`: append-only JSONL per session at
  `~/.aster/sessions/<cwd-slug>/<ULID>.jsonl`. `TranscriptEvent::Message`
  records role, content, `tool_calls`, `tool_call_id`, timestamps, token
  usage, and reasoning. `SessionTranscript::load` parses leniently.
- `aster-skills`: a skill is a directory with `SKILL.md` (YAML frontmatter
  `name`/`description`, markdown body). Project root `.aster/skills` overrides
  the user-global root on name collision. Bodies load on demand.
- `aster-sandbox`: Seatbelt on macOS, bwrap on Linux. `run_command` with
  `SandboxConfig` returns `CommandOutput` with exit code, stdout, stderr,
  timeout flag. This is the gate's execution substrate.
- `aster-policy`: approval and command policy. The gate's static safety check
  goes through it.
- `aster-eval`: eval harness with live runs, reports, stats. The A/B
  (compiler on vs off) rides on it.
- `aster import`: converts Claude Code, Codex, Cursor, opencode, and Hermes
  histories into Aster transcripts. Day-one corpus for the miner: users
  compile skills from work they already did in other agents.
- `aster.yaml` review config: the hypothesis/verify model split and
  `min_confidence` pattern the distiller config mirrors.

## Architecture

Five stages, one new crate (`aster-distill`), one new skill kind.

```
transcripts ──> Miner ──> candidates ──> Synthesizer ──> package ──> Gate ──> skills root
                                                                       │            │
                                             (fail: stays candidate,   │            v
                                              with failure trace) <────┘        Router (tool call)
                                                                                    │
                                              live failure evidence <── fallback ───┘
```

### 1. Miner (`aster-distill::mine`)

Input: every transcript for the current repo (`repo_root` in `SessionMeta`
matches), including imported ones.

1. **Segment** each transcript into task episodes: a user turn through the
   terminal assistant turn that resolves it. Summaries and evictions bound
   segments; they never substitute for events.
2. **Label** episode success conservatively. An episode counts as successful
   only when it contains verification evidence: a tool call whose output shows
   exit code 0 on a test/build/check command, or an explicit verified outcome.
   Absence of a user correction is necessary but not sufficient. When in
   doubt, the episode is unlabeled and ignored. Noisy labels poison
   everything downstream, so precision beats recall here.
3. **Skeletonize**: reduce the episode to its tool-call sequence with
   normalized args. Concrete paths, branch names, identifiers, and literals
   become typed slots. The skeleton is the unit of comparison.
4. **Cluster** skeletons across episodes (Jaccard over command-template
   n-grams is enough for v0). A cluster with `min_episodes` successful
   members (default 3) becomes a candidate, written to
   `.aster/distill/candidates/<id>/` with references to source sessions and
   event ranges. Raw transcripts are never copied, mutated, or summarized.

### 2. Synthesizer (`aster-distill::synthesize`)

One LLM pass per candidate (model from `distill.model`, same
OpenAI-compatible client as everything else). Input is the raw episodes, not
summaries. Output is a compiled skill package:

```
.aster/skills/<name>/            project-scoped, wins over global on collision
  SKILL.md                       frontmatter: name, description, kind: compiled,
                                 version, params (JSON Schema), plus a body
                                 documenting preconditions and when NOT to use it
  run.sh                         the procedure, parameterized, POSIX sh
  check.sh                       postconditions derived from what success looked
                                 like in the episodes; exit 0 = pass
  cases/                         one file per source episode: recorded param
                                 bindings + expected postconditions
  provenance.json                source session ids, event ranges, synthesis
                                 model, timestamps, gate verdicts
```

Constraints enforced at synthesis time:

- `run.sh` may only touch the network if a source episode did, and the
  manifest must declare it.
- Episodes are split before synthesis: holdout cases are withheld from the
  model and only the gate sees them. With exactly `min_episodes` members,
  leave-one-out.
- v0 compiles shell. A later version can target `rust-script` or a small DSL,
  but shell covers the terminal-agent trajectory space and keeps replay
  trivial.

### 3. Gate (`aster-distill::gate`)

Admission requires all of the following, executed, not judged:

1. **Replay.** Every case (synthesis and holdout) runs in an isolated git
   worktree via `aster-sandbox::run_command`, and `check.sh` passes on every
   one. Failure budget: zero. This is a hard rule, not a tunable default;
   the GRASP result says the gate is where all the value lives.
2. **Holdout.** The withheld cases pass. This is what catches
   over-parameterization to the episodes the model saw.
3. **Static safety.** `run.sh` clears `aster-policy`: no credential paths, no
   writes outside the worktree, no undeclared network, no destructive
   commands without idempotence guards.
4. **Library regression.** Previously admitted skills re-verify (cheap, they
   are deterministic). Any regression blocks admission.

Verdicts land in `provenance.json`. A failed candidate stays in
`.aster/distill/candidates/` with its full failure trace and is retried only
when new evidence (new episodes) arrives. It never enters the skills root.

### 4. Router (integration, not a new subsystem)

Compiled skills surface to the model as first-class function-calling tools,
one per skill, schema from the `params` frontmatter, description from the
skill description. No learned router in v0: the model already sees the
current state and decides, which is exactly the state-grounded routing the
literature says task-level retrieval gets wrong. Execution inside the tool is
deterministic: sandbox, `run.sh`, then `check.sh`.

- **Success**: tool returns output + check verdict. Tokens spent: one tool
  round-trip instead of the full trajectory.
- **Check failure**: tool returns the failure evidence; the model falls back
  to doing the task manually. The failed invocation is appended to the
  skill's evidence file.
- **Demotion**: two consecutive live check failures move the skill back to
  candidates automatically. The world changed; the compiler re-synthesizes
  from the union of old and new evidence.

### 5. Curation and telemetry

- Library cap: `distill.max_compiled` (default 20) per project. Admission at
  cap evicts the lowest-utility skill. Utility = invocations x mean tokens
  saved, exponentially decayed.
- Every invocation logs, via `aster-telemetry`: skill name, verdict, latency,
  and tokens saved (mean source-episode cost minus tool round-trip cost, both
  computable from `EventUsage`).
- `aster skills compiled --stats` prints per-skill: uses, success rate,
  fallback rate, tokens and dollars saved, last verified. `--json` feeds the
  launch chart: one task, five runs, cost collapsing.

## CLI surface

```
aster distill                 mine + synthesize + gate for the current repo;
                              prints admitted and rejected with reasons
aster distill status          candidates, evidence counts, gate failures
aster skills compiled         list compiled skills
aster skills compiled --stats efficiency numbers, --json for machines
aster skills verify           re-run every gate; nonzero exit on regression (CI)
```

Config in `aster.yaml`:

```yaml
distill:
  auto: false          # run distill in the background when a session ends
  min_episodes: 3
  max_compiled: 20
  model: anthropic/claude-sonnet-5
```

## Milestones

- **M1, miner.** Segmentation, labeling, skeletons, clustering.
  `aster distill status` surfaces real candidates from dogfooding transcripts
  (seed the corpus with `aster import`). Exit: at least one genuine repeated
  pattern found in real history.
- **M2, package + gate.** Synthesizer emits packages; `aster skills verify`
  replays them in worktrees. Exit: first skill admitted through the full gate.
- **M3, routing.** Compiled skills exposed as tools, fallback and demotion
  wired. Exit: the five-run cost curve on a real task, screenshot-ready.
- **M4, loop + proof.** `distill.auto`, curation cap, stats command, A/B via
  `aster-eval` (compiler on vs off on a fixed task set), Terminal Bench and
  SkillEvolBench numbers. Exit: launch post with the curve and the benchmark
  table.

## Risks

- **Sparse local corpus.** A fresh install has few transcripts. Mitigation:
  `aster import` seeds from Claude Code, Codex, Cursor, opencode, Hermes.
  This is also the onboarding pitch: Aster compiles skills from work you
  already did elsewhere.
- **Label noise.** Mislabeled successes compile broken skills. Mitigation:
  verification-evidence-only labeling, and the gate catches what labeling
  misses. Two independent filters, both must fail for a bad skill to land.
- **Staleness.** Repos drift, skills rot. Mitigation: `aster skills verify`
  in CI, live demotion after two failures, re-synthesis from accumulated
  evidence.
- **Over-parameterization.** The model fits run.sh to the episodes it saw.
  Mitigation: holdout cases are structural, not optional.
- **Safety of generated shell.** Mitigation: policy check at the gate,
  sandbox at every execution, worktree isolation at replay, declared-network
  manifest. The sandbox is a boundary, not a guarantee; the policy layer
  stays in front, same as everywhere else in Aster.

## Competition (checked 2026-08-25)

Three tiers exist. None ships the compile-gate-route loop in a real harness.

- **SKILL.md distillers** (skilldistill on PyPI, Hivemind, the
  generating-skills-from-logs meta-skill): mine Claude Code transcripts into
  prose SKILL.md drafts. They prove demand for exactly this loop and inherit
  exactly the failure the literature documented: text skills, no execution,
  no gate. Hivemind is the closest in loop automation (auto-mine on session
  end) and still outputs prose.
- **Research frameworks.** EvoSkill (sentient-agi, ~1.1k stars, Apache 2.0)
  is the serious OSS neighbor: it evolves skills from failed trajectories
  against benchmark validation sets. Different loop: offline
  benchmark-optimization, instruction-folder skills, and by its own docs no
  runtime routing or fallback. GSE (arXiv 2608.06153, Aug 2026) has
  replay-driven verification, the closest published idea to the gate, applied
  to two narrow SE tasks on OpenHands, no product. CODESKILL
  (arXiv 2605.25430) trains an RL skill-bank policy, heavyweight, not a
  shipped runtime. AXIS (Microsoft, 2024) compiled UI trajectories into
  executable macros, prior art for the compile idea, never a coding harness.
- **Big harnesses.** No evidence as of August 2026 that Claude Code, Codex,
  or Cursor ship native session-to-skill compilation; Claude Code's ecosystem
  of third-party distillers shows the demand sitting unserved. The standing
  risk is one of them shipping it natively; the defenses are speed,
  model-agnosticism, local-first, and the gate rigor none of the ecosystem
  tools attempt.

What stays uniquely ours if we ship fast: executable-first packages with
postcondition checks, the zero-budget sandboxed replay gate on the user's own
history, verification-aware routing with fallback and demotion, cost
telemetry as a product surface, and `aster import` turning every competitor's
transcript archive into our training corpus.

## Positioning

"Open source coding agent in Rust" is a crowded axis. "The coding agent that
compiles itself" is not. This directly answers the open problems the
skill-library papers published, none of which ship executable gated skills.
The launch artifact is one chart and one table: the cost curve, and the A/B
benchmark row. Both fall out of M3 and M4 rather than needing separate work.
