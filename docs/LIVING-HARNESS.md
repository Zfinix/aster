# The living harness

Aster runs models a tier below Claude. The harness has to supply the judgment
the model lacks: knowledge as predefined skills, recovery as feedback that
carries the answer, and pacing as gates and nudges. This document is the design
for that, grounded in a transcript study of real Claude Code sessions (Aug 2026)
across four corpora: this repo,
serverpod-website plus portfolio (frontend, verification-heavy), and
serverpod-docs plus feature-pages (skills and optimization). Counts below come
from those transcripts.

It slots around [HARNESS.md](HARNESS.md): that document owns sessions, memory,
approvals, and delegation; this one owns what happens inside a turn when things
fail, finish, or drift.

## The core finding

Every error chain three or more deep in the study shared one signature: the
retry ignored the instruction in the error text. Claude blind-retried a
stale-file edit four times before doing the Read the error told it to do.
Meanwhile the fastest recovery observed was the harness doing the work for the
model: `File does not exist. cwd is X. Did you mean code_review?` recovered in
one step, every time.

The rule that follows, for weaker models especially: do not tell the model what
to do next. Do the lookup and embed the result in the error. Telling costs a
round and can be ignored; embedding costs nothing and cannot.

## What our own bad sessions showed

- A deploy session hit a sandbox tempdir denial (`bun install` cannot write to
  `$TMPDIR`), then a command timeout whose partial output was discarded
  ([chat.rs](../crates/aster-cli/src/chat.rs), `run_raw` bails on timeout).
  With zero evidence the model invented "no network", skipped the typecheck it
  had promised, and reported done. Three gaps compounded: sandbox config,
  discarded timeout output, no verification doctrine.
- The same task in the VSCode surface ended its turn with
  `ask_user("How do you want to deploy to Cloudflare?")` after the user had
  already said "deploy this to cloudflare".
- Every session starts blind: no git status, branch, date, or platform. Only a
  package-manager note.

## Pillar 1: errors that carry the answer

From 80 hard errors plus 13 build failures masked by piped exit codes in the
instalog corpus:

| Failure | Decorator |
|---|---|
| `edit_file` mismatch | Embed the fuzzy-closest region of the file (about 10 lines either side) in the error: "re-issue using this exact text". Stale file: embed the diff since last read. |
| Path not found | Always append cwd, nearest-name suggestion, and sibling entries. `missing_path` already does part of this; make it universal, including `run_command` cd failures. |
| Build or test output | Parse stdout, not exit codes. `error[E\d+]`, `error TS`, `FAILED`, `panicked` with exit 0 means a pipe masked the compiler (13 cases observed). Append "BUILD FAILED: first error at file:line". |
| Timeout | Return the partial stdout/stderr tail plus: do NOT re-run with a longer timeout; kill leftovers, then run a narrower variant. Claude wasted 10 minutes escalating 2m to 8m on a hung `cargo metadata`. |
| Auth errors | Tag non-retryable: "AUTH ERROR, retrying will not help. Tell the user, continue with what does not need it." Claude burned three identical retries on `railway login`. |
| Sandbox denial | Name the sandbox as the cause and offer the escalation path, so it never reads as a network or tooling failure. |
| Repeated action | Extend the repeat-lookup pointer to commands and reads: "you have run this exact command 3 times with identical output." 31% of Claude's Reads were redundant; one file was read 23 times. |
| User rejection | Keep the wording "STOP what you are doing and wait": 14/14 compliance observed. |

## Pillar 2: verification gates

From 104 edit-containing turns in the frontend corpus: Claude verified code
edits near-100% of the time (prose exempt), verification was behavioral rather
than compilational (curl the running page and grep for the expected string,
64 times; poll-until-listening loops, 21; scoped `analyze | tail`, 61), and it
reported over a red check 3 times out of 73.

- Post-edit gate: a turn that edited code files and ran no check command after
  the last edit gets an injected nudge naming the detected check command.
  Freshness counts: a check that ran before the last edit does not.
- Red-check gate: the turn cannot end quietly on a failing check. Root cause in
  one sentence, fix, re-run the same command. One escape hatch: declaring the
  failure environmental or pre-existing, with evidence.
- Record and replay: the harness remembers assertions that passed earlier in
  the session (curl/grep checks, test filters) and suggests re-running them
  after later edits nearby. Every user blowup in the study was one of four
  sins: substituting an easier artifact for the asked action, inventing details
  beyond the reference, regressing adjacent working behavior, or side effects
  outside the verified surface. Replay catches the last two mechanically.
- Final-message doctrine: verdict first ("Fixed. 15/15 pass."), concrete
  evidence, then what was deliberately not done. "Should work" is banned;
  either "verified by <command>" or "unverified because <reason>".

## Pillar 3: built-in skills and the memory loop

The winning skill format in the study: numbered imperative rules, each with a
literal write-this/never-this counter-example, about 6KB. Skills flipped
behavior instantly and persisted for dozens of messages. Only 4 of roughly 120
available skills ever fired, so the catalog itself is a standing context cost:
ship few, targeted built-ins.

Shipped (via `include_str!` in
[aster-skills](../crates/aster-skills/src/lib.rs), manifests under
[builtins/](../crates/aster-skills/builtins/)) as two tiers, because the index
is a standing context cost:

**Core, always in the index (9):** git-workflow, gh-pr-workflow,
verify-before-done, build-triage, batched-bash, cli-toolbox, context-economy,
correction-protocol, security-hygiene. The bar: earns its place on a routine
coding turn. An installed skill with the same name shadows its built-in.

**Optional, bundled but not indexed (9):** package-managers,
supply-chain-safety, dependency-upgrade, debug-systematically, refactor-safely,
write-tests, background-processes, i-have-adhd, skill-creator.
`aster skills bundled` lists them; `aster skills bundled <name>` materializes
one into a skills root, after which discovery treats it like any installed
skill.

Routing is split by observability. Events the harness can detect mechanically
(build failure, timeout) carry a pointer to the matching skill in their
coaching note. Language and tone are the model's call: the index instruction
makes a match-on-meaning scan mandatory before the first action of every turn
and tells the model to batch `read_skill` into `explore`, so loading costs no
extra round. Keyword tables were tried and removed; they cannot enumerate
English.

The memory loop closes it: correction-protocol ends every taken correction
with a `remember` save. Teach once, comply forever: in the study, a correction
written to memory was honored in every later session with zero re-prompting,
while unsaved corrections were re-typed across sessions in increasingly
frustrated caps.

## Environment context

Injected at session start (`environment_note` in
[chat.rs](../crates/aster-cli/src/chat.rs)): platform and arch, today's date,
branch and default branch, a bounded `git status --porcelain` snapshot, the
last five commits, the package manager each lockfile pins, and the project's
own verbs (Justfile/Makefile/Taskfile presence, package.json script names).
The study's models started acting immediately from this context instead of
spending rounds on discovery; date injection also stops training-cutoff dates
leaking into commits and docs.

## Phases

- **P1, mechanical — shipped**: timeouts return partial output with coaching,
  the sandbox inherits `TMPDIR` and allows bun/yarn/pnpm caches, the error
  decorators above (fuzzy-region edit errors, first-error extraction,
  pipe-masked failures, auth tags, sandbox-denial naming), the environment
  block.
- **P2, doctrine and skills — shipped**: prompt sections in
  [aster-agent.md](../crates/aster-cli/prompts/aster-agent.md) (shape of a
  reply, verifying work, fidelity, taking a correction) plus the 18 built-in
  skills in two tiers.
- **P3, state loops**: read-before-edit and stale-file tracking, the
  verification gate with edit/verify ordering, replayable assertions, the
  repeat-action guard for commands, plan staleness nags, memory-on-correction
  nudges.
- **P4, infrastructure**: a compaction template whose summary preserves all
  user messages as constraints (the studied template's key trick), and one
  [live eval case](../crates/aster-eval/src/live.rs) per feedback loop so a
  regression in any of them fails an eval rather than waiting for the next bad
  session.
