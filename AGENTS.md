# Aster

Rust workspace, edition 2024, stable toolchain. Crates live in `crates/` and are
prefixed `aster-` (the exception is `symbol-extractor`). The `crates/aster-cli`
crate is the binary; everything else is a library.

Human-facing setup and PR etiquette live in `CONTRIBUTING.md`. This file is the
working agreement for agents editing this repo.

## Global rules

- No useless comments in code.
- Match the project's existing style and patterns before writing anything new.
- No em dashes (—) in any writing.
- Never add `Co-Authored-By: Claude/Cursor` trailers to commits or PRs, and
  strip them if found.
- Use Conventional Commits for every message.

## Rust conventions

- Read the crate you are touching and follow its existing patterns before
  introducing a new one.
- Inline variables into `format!` braces: `format!("{name}")`, not
  `format!("{}", name)`.
- Collapse nested `if` statements.
- Prefer method references over closures: `.map(str::trim)`, not
  `.map(|s| s.trim())`.
- Avoid bare `bool` and ambiguous `Option` parameters that force callers to
  write `foo(false)` or `bar(None)`. Prefer enums, newtypes, or named methods so
  the callsite reads on its own. `aster_policy::Mode` and
  `PermissionModeArg` are the shape to copy.
- Make `match` statements exhaustive. Avoid wildcard arms over enums we own, so
  adding a variant produces a compile error instead of silent fallthrough.
- New traits get a doc comment explaining their role and what implementors are
  expected to do. `McpInvoker` in `crates/aster-mcp/src/lib.rs` is the example.
- Prefer private modules with an explicit `pub` crate API. Keep the exported
  surface as small as the callers need.
- Do not create helper functions that are called exactly once.
- Keep the core dependency-light. No vector DB, no external services in the
  review path. A new heavy dependency needs a written justification in the PR.

## Module size

- Target modules under 500 LoC, excluding tests.
- Past roughly 800 LoC, add new functionality in a new module rather than
  extending the file, unless there is a documented reason not to.
- This applies hardest to the files that already attract unrelated changes:
  - `crates/aster-cli/src/chat.rs`
  - `crates/aster-cli/src/skills.rs`
  - `crates/aster-harness/src/lib.rs`
  - `crates/aster-cli/src/review.rs`
  - `crates/aster-mcp/src/lib.rs`
- When extracting from a large module, move the related tests and type docs with
  the code so the invariants stay next to what owns them.
- `chat.rs` is orchestration. Prefer new modules under `crates/aster-cli/src/`
  over new standalone functions there.

## Resist adding to `aster-cli`

`aster-cli` is the largest crate, so it is always the path of least resistance:
the imports are there, the helpers are there, and nothing forces you out. That is
how it got large.

Before adding a new concept, feature, or API to `aster-cli`, consider whether:

- An existing library crate is the right home. Tool execution, context
  assembly, session state, and policy decisions are library concerns, not CLI
  concerns.
- It is time to add a crate to the workspace. Refactor as needed to make that
  happen.

`aster-cli` should be argument parsing, terminal I/O, and wiring. If a change
would still be correct with no terminal attached, it probably belongs in a
library crate.

The same applies in review: push back on changes that grow `aster-cli` without
needing to.

## Model-visible context

Aster builds a context that is sent to the model on every inference request.
Every regression here is expensive and hard to see, so these are invariants, not
preferences.

1. **No history rewrite.** Context is built up incrementally. The only
   permitted rewrite is the summarize-and-replace path in `compact_if_needed`,
   and it must record a `record_summary` event so the transcript stays honest
   about what was dropped.
2. **Avoid changes that cause cache misses.** Anything prepended or inserted
   near the head of the request invalidates the provider's prompt cache for the
   whole conversation. Append instead. If a change moves or reorders early
   context, say so in the PR.
3. **Nothing unbounded.** Everything injected into context has a bounded size
   and a hard cap. Existing caps live as `const` at the top of their module:
   `MAX_TOOL_RESULT_CHARS`, `MAX_SEARCH_HITS`, `MAX_LIST_ENTRIES`,
   `COMPACT_BUDGET_CHARS` in `chat.rs`; `inventory_budget_tokens` in
   `aster-mcp`. New injections declare their own.
4. **No single item over ~10K tokens.**
5. **Flag any new individual item that can exceed ~1K tokens** in the PR
   description. Those need a human to look at them.
6. **Growth with the repo is unbounded growth.** `SkillSet::render_index`,
   agent registries, and memory blocks all render one line per discovered item.
   Anything that scales with the number of skills, agents, servers, or memory
   blocks in a user's project needs a cap and a documented truncation rule.

The progressive injection in `aster-mcp` is the pattern to follow for anything
new that is potentially large: an inventory under a token budget, a search tool
to expand it on demand, and `estimated_tokens` to measure rather than guess.

## Code review rules

### Breaking changes

Check these surfaces explicitly. Breaking one is a user-visible break even when
the code compiles:

- CLI commands, subcommands, and flags (`crates/aster-cli/src/main.rs` and each
  command's `*Args`)
- `aster.yaml` loading and defaults, and `aster.yaml.example`
- The NDJSON event stream emitted by `--stream` (the desktop app and the VS Code
  extension both consume it)
- Transcript and session formats in `aster-persist`, including resuming an
  existing session
- Policy decisions and permission modes in `aster-policy`
- The MCP bridge tool schema and its routing contract
- `ASTER_*` environment variables

### Change size

Unless the change is mechanical, keep it under 800 changed lines. For complex
logic changes, aim for 500.

If it is larger, work out whether it splits into reviewable stages and land the
smallest coherent one first. Base that on the actual diff and its call sites,
not a guess.

## Tests

- Behavior changes get tests. Prefer integration tests over unit tests for
  anything that changes agent behavior: `crates/aster-harness/tests/e2e.rs`
  runs the pipeline end to end against a mock provider, and
  `crates/aster-persist/tests/roundtrip.rs` covers session durability. Extend
  those rather than reaching for a unit test on an internal helper.
- Compare whole objects with `assert_eq!` rather than checking fields one at a
  time.
- Do not add tests for statically defined values.
- Do not add negative tests for logic that was removed.
- Avoid test-only functions in the main implementation. If a test needs a
  seam, the seam should be justified by the production code.
- Avoid mutating process environment in tests. Aster reads a lot of `ASTER_*`
  env, and tests run in parallel in one process. Pass the resolved config down
  instead.
- New test modules go in a sibling file, referenced explicitly, so the filename
  says what it covers:

  ```rust
  #[cfg(test)]
  #[path = "decision_tests.rs"]
  mod tests;
  ```

  This applies to new test modules only. Do not rewrite existing inline
  `#[cfg(test)] mod tests { ... }` blocks just to follow it.
- Tests must pass on Linux, macOS, and Windows unless the feature is explicitly
  OS-specific. Shelling out to `git`, `rg`, `semgrep`, or `ast-grep` is the
  usual way this breaks.

## TUI conventions

TUI code is `crates/aster-cli/src/tui/`, built on ratatui 0.29.

- Use `Stylize` helpers: `"text".dim()`, `.bold()`, `.cyan()`, `.underlined()`.
  Prefer them over constructing `Span::styled` with an explicit `Style`.
- `Span::styled` is fine when the style is computed at runtime.
- Do not use `.white()`. Use the default foreground so the output works on light
  and dark terminals.
- Prefer `"text".into()` for spans and `vec![...].into()` for lines when the
  target type is obvious. Use `Line::from` / `Span::from` when it is not.
- Do not refactor between equivalent forms without a readability gain. Follow
  the conventions already in the file.
- Wrap with `Paragraph::wrap(Wrap { trim: false })` rather than splitting text
  by hand. For single-line elision use `console::truncate_str` with an `…`
  suffix, the way `skills.rs` does.
- Shared drawing lives in `crates/aster-cli/src/tui/helpers.rs`. Check it before
  writing a new banner, input box, chip, or dim line.
- Terminal state is owned by `tui/guard.rs`. Anything that can leave the
  terminal in raw mode on a panic or an early return goes through the guard.

## Commands

The `Makefile` is the interface. Do not invent cargo invocations.

- `make fmt` after finishing code changes anywhere in the repo. Do not ask for
  approval to run it.
- `make lint` runs clippy with `-D warnings`, workspace-wide.
- `make test` runs the workspace test suite. For a scoped run while iterating,
  `cargo test -p aster-cli` and friends are fine.
- `make check` is fmt-check plus lint plus test, and is exactly what CI gates
  on. Run it before saying a change is done. Ask before running it if the user
  is mid-iteration, since it is a full workspace build.
- `make desktop-check` typechecks the Tauri frontend. Run it if you touched
  `desktop/`. The Rust CI job does not cover it.
- `make install` rebuilds and overwrites the `aster` on PATH. The desktop app
  and the VS Code extension both shell out to it, so a stale binary makes your
  changes invisible to them.

Rust builds here are slow and the cargo lock makes them look stuck. Be patient
with `make check` and `make test`. Do not kill them by PID.

Do not re-run tests after `make fmt`.

## Documentation

`docs/` holds architecture and design notes: `ARCHITECTURE.md`, `ALGORITHM.md`,
`ANALYZERS.md`, `MCP.md`, `MEMORY.md`, `ROADMAP.md`. Update the relevant one
when you change the thing it describes. User-facing product documentation lives
on the site, not in `docs/`.

Prompts are content, not code. They live in `crates/aster-cli/prompts/` and are
pulled in with `include_str!`. Do not inline prompt text into Rust source.

## Commits

Conventional Commits: `type(scope): summary`, imperative mood, lowercase, no
trailing period. Types: feat, fix, perf, refactor, docs, test, chore, build, ci,
style, revert.
