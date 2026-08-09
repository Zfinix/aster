# Changelog

All notable changes to Aster are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Remote MCP servers over Streamable HTTP and the deprecated HTTP+SSE binding,
  alongside the existing stdio transport. Declare one with a `url` (plus
  optional `headers`) in `aster.yaml`, `.mcp.json`, or a plugin's `mcp.json`;
  `type` picks the binding and defaults to `streamable-http`. Sessions carry the
  server's `Mcp-Session-Id` and the negotiated revision, redirects are followed
  only inside one origin so configured headers cannot leak to another host, and
  a session is ended with a `DELETE` at shutdown. `aster mcp import` now brings
  remote servers across instead of skipping them, and `aster mcp list` names
  each server's transport.
- Agent Plugins support ([spec](https://github.com/agentplugins/agent-plugins-spec)
  v1.0.0): a plugin is a directory with a `plugin.json`, skills under `skills/`,
  and MCP servers in `mcp.json`. `aster plugins add|list|remove|validate` manages
  them, user-global or per project with `-p`. A plugin's skills join the skill
  index (behind skills roots, ahead of built-ins) and its stdio MCP servers join
  the configured ones as `<plugin>/<server>`, with `${PLUGIN_ROOT}` and
  `${PLUGIN_DATA}` expanded and both supplied to the subprocess. Remote
  transports are validated and listed but skipped, since the MCP runtime is
  stdio-only.
- `mcp.servers.<name>.cwd` in `aster.yaml`, the working directory a server is
  started in.

- 18 built-in skills in two tiers. Nine core skills (git-workflow,
  gh-pr-workflow, verify-before-done, build-triage, batched-bash, cli-toolbox,
  context-economy, correction-protocol, security-hygiene) sit in every
  session's skill index; nine optional ones (package-managers,
  supply-chain-safety, dependency-upgrade, debug-systematically,
  refactor-safely, write-tests, background-processes, i-have-adhd,
  skill-creator) ship in the binary and install with
  `aster skills bundled <name>`. An installed skill of the same name shadows
  its built-in.
- A session-start environment snapshot in the system prompt: platform, date,
  git branch and status, recent commits, the package manager each lockfile
  pins, and the project's task-runner verbs (Justfile, Makefile, Taskfile,
  package.json scripts).
- Error coaching on tool results. Failed edits embed the closest matching
  region of the file with line numbers; command results flag pipe-masked build
  failures, extract the first compiler error, mark auth failures as
  non-retryable, and name the sandbox as the cause of permission denials.
- Agent doctrine sections in the system prompt: shape of a reply, verifying
  work before reporting done, command and reference fidelity, and taking a
  correction.
- [docs/LIVING-HARNESS.md](docs/LIVING-HARNESS.md): the transcript-study
  design behind all of the above.

### Fixed

- Timed-out commands now return the output they produced before the kill,
  with guidance, instead of discarding it.
- The sandbox inherits `TMPDIR` and allows the bun, yarn, and pnpm caches, so
  JavaScript installs no longer fail with permission errors.

- Three new Context.dev web tools. `web/search` searches the web and returns
  each result scraped to Markdown, `web/sitemap` lists a domain's sitemap URLs
  without scraping any pages, and `web/screenshot` captures a rendered page as
  a PNG and returns its CDN URL. All three ship as `aster web search`,
  `aster web sitemap`, and `aster web screenshot` subcommands, and search now
  prefers Context.dev over Firecrawl and Browserbase when its key is set.
- Document reading via [anydoc](https://github.com/firecrawl/anydoc).
  `read_file` converts PDF, Word, PowerPoint, Excel, OpenDocument, EPUB, and
  RTF files to Markdown, and sniffs documents hiding behind wrong extensions.
  CSV stays raw so line numbers keep matching the bytes on disk. The keyless
  `web/extract` fallback applies the same conversion to document URLs,
  detected by magic bytes rather than Content-Type.

## [0.3.0] - 2026-07-30

Aster stops being a review tool that can chat and becomes a general coding
harness. The agent now runs commands inside an OS sandbox, works through five
permission modes instead of three, reaches outside the repo through MCP servers
and the web, and drives a rebuilt terminal UI.

### Added

#### Command execution and sandboxing

- New `aster-sandbox` crate: OS-native isolation for agent-run commands.
  macOS uses Seatbelt (`sandbox-exec`), Linux uses bubblewrap (`bwrap`) when
  present, and every other platform degrades to a process-level sandbox with a
  filtered environment and a timeout, logging a warning that isolation is not
  OS-enforced.
- `run_command` tool: the agent can run CLI commands. Filesystem writes are
  confined to the repository and temp directories, secrets are stripped from
  the child environment, and `turbo: true` additionally blocks network access.
- Backend detection is automatic; the sandbox is treated as a boundary, not a
  guarantee, so policy and approval still run ahead of it.

#### Permission model

- Modes are now `plan < manual < auto < edit < yolo`, replacing
  `deny / ask / auto`.
  - `plan` explores and proposes, never edits.
  - `manual` confirms every edit.
  - `auto` applies what passes the safety check and asks about the rest.
  - `edit` applies without confirmation.
  - `yolo` bypasses the sandbox and skips policy checks, behind a confirmation
    prompt and a red theme.
- Directory grants (`aster-policy::Grants`): approving an out-of-repo directory
  once covers it, and everything nested under it, for the rest of the session.
  Seeded from `permissions.additional_directories`.
- Grants persist across runs through `aster-persist`.
- `Mode::stricter` means a CLI flag can only tighten what `aster.yaml`
  configures, never loosen it.

#### Plan mode

- The agent drafts a plan, presents it for approval, and only then executes.
- `update_plan` and `exit_plan_mode` tools; approving a plan promotes the whole
  session to edit mode rather than just the current turn.
- `ask_user` tool: structured multiple-choice questions rendered as a picker in
  the TUI.
- Ships the `aster-planning` skill that plan mode runs on.

#### MCP

- New `aster-mcp` crate implementing progressive disclosure: servers advertise
  through one bridge tool plus a context-bounded inventory, so a dozen servers
  do not flood the model's context.
- Speaks both protocol eras — the modern per-request `_meta` form
  (`2026-07-28`, `2025-11-25`) and the legacy `initialize` handshake — and
  negotiates down on `-32022` without falling back to legacy.
- Paginated `tools/list` is read to the last page.
- `aster mcp list | enable <name> | disable <name>`, with `disabled:` toggled in
  place in `aster.yaml` so comments and formatting survive.

#### Web

- New `aster-web` crate with pluggable crawl and extract providers: Firecrawl,
  Jina, Browserbase, Cloudflare Browser Rendering, context.dev, and a plain
  HTTP fallback. `WebBackend::from_env` holds every provider whose key is set
  and dispatches to the best available one.
- `aster web crawl <url>` and `aster web extract <url>` emit Markdown.
- The same providers register as agent tools.

#### Agents

- New `aster-agents` crate: `AgentDef` with frontmatter parsing, `AgentSource`,
  and an `AgentRegistry` that discovers built-in and repo-local agents.
- `DEFAULT_TOOLS` is exported so a custom agent can start from the standard
  catalog.

#### Terminal UI

- Rebuilt chat TUI: bottom pane with composer, an inline viewport that writes
  finished blocks into real scrollback, Markdown rendering with syntax
  highlighting, `@`-mentions backed by a file index, and bracketed paste.
- Slash commands: `/model`, `/provider`, `/resume`, `/mode`, `/effort`,
  `/yolo`, `/clear`, `/help`, `/quit` or `/exit`.
- Approval, plan-approval, and question prompts render as pane views with
  keyboard selection.
- Consecutive read-only tool calls collapse into a single `Explored` cell.
- Theme transitions interpolate between palettes, so entering YOLO mode visibly
  shifts the interface red.
- Live token and cost meter in the header.

#### Tooling and search

- New `aster-tools` crate: tiered dispatch that uses `rg` and `fd` when they are
  on `PATH` and falls back to embedded implementations when they are not.
- New agent tools `search_files`, `find_files`, and an upgraded `list_files`,
  all routed through that dispatch.
- A path that does not exist comes back with nearest-match suggestions instead
  of a bare error.

#### Model and provider handling

- Reasoning effort: `off | low | medium | high`, settable per run with
  `--effort`, in `aster.yaml`, or live with `/effort`.
- Tool calls emitted inline as assistant text — JSON blobs or XML-style invoke
  blocks — are parsed back into real tool calls, so models without native tool
  support no longer stall the agent loop.
- `complete_tools_with` allows a per-call model override.
- Streaming tool-call completion.
- `/provider` switches endpoint and model together, naming any provider-specific
  key that is missing.

#### CLI

- Bare `aster` launches the interactive chat TUI.
- `aster chat --resume [id]`: resume a saved session, or pick one from a list.
- Root `--json` flag for machine-readable output across commands.
- Root `--effort` flag.
- New `aster web` and `aster mcp` subcommands.

#### Desktop

- Tauri app gains a working chat UI: approval prompts, `@`-mentions, activity
  output panel, and the Paper Mono typeface.

#### Documentation

- `AGENTS.md`, `docs/HARNESS.md`, `docs/MCP.md`, `docs/ROADMAP.md`, and a TUI
  specification under `specs/`.
- `docs/ARCHITECTURE.md` rewritten for the new crate layout.

### Changed

- The chat agent is described as the Aster agent rather than the review agent;
  review is now one capability among several.
- Tool catalog is tiered — the agent is given a working set rather than every
  tool at once.
- Global environment file moved from the platform config directory to
  `~/.aster/.env`.
- `aster logout` reports `{ "ok": true, "was_logged_in": <bool> }` under
  `--json`.
- The init wizard writes a `context.dev` key when that provider is chosen.

### Removed

- The `/edits` slash command, replaced by `/mode`.
- A stale committed `aster.yaml`.

### Migration notes

- **Permission modes renamed.** `deny` and `ask` still parse as aliases for
  `plan` and `manual`, so existing `aster.yaml` files keep working. `auto` now
  means "apply what is safe, ask about the rest"; the old unconditional-apply
  behavior is `edit`, which is the new default.
- **Global `.env` moved.** Move `$XDG_CONFIG_HOME/aster/.env` (or
  `~/Library/Application Support/aster/.env` on macOS) to `~/.aster/.env`.
- **Command execution is new.** The agent can now run commands. It is sandboxed
  and policy-checked, but if that is unwanted, restrict it through
  `permissions:` in `aster.yaml`.

## [0.2.0] - 2026-07-26

Aster gains a memory. Chats are written to disk and can be listed, replayed, and
continued; durable facts survive across sessions; and skills let the agent be
taught workflows without touching its prompt.

### Added

#### Persistence

- New `aster-persist` crate holding a per-repo store under `~/.aster`.
- Session transcripts are append-only JSONL, versioned by `TRANSCRIPT_VERSION`,
  written live as a turn runs rather than flushed at the end.
- Sessions are keyed by ULID, so listing is chronological without a timestamp
  index.
- Durable memory: a project file (`ASTER.md`) plus named memory blocks, each
  with a name and description.
- Roundtrip tests covering transcript and memory persistence.

#### Sessions and memory on the CLI

- `aster sessions list` and `aster sessions show <id>`.
- `aster memory list` and `aster memory add <text> [--title <name>]` — with a
  title it creates a named block, without one it appends to project memory.
- Both support `--json` for editors and the desktop app.
- `aster chat --continue` resumes the most recent session; `--session <id>`
  targets a specific one, headless runs included.

#### Memory tools for the agent

- `remember` saves a durable fact, optionally as a named block.
- `recall` loads a memory block's full body by name.
- The system prompt lists recallable memory as names and descriptions only, so
  the agent pays for the index rather than the whole store and calls `recall`
  when it actually needs a block.

#### Skills

- New `aster-skills` crate: a skill is a directory with a `SKILL.md` — YAML
  frontmatter (`name`, `description`) over a Markdown instruction body.
  Discovery reads only the frontmatter; the body loads on demand.
- Two roots, mirroring Claude Code: project (`.aster/skills`) overrides
  user-global on a name collision.
- `read_skill` tool so the agent can pull in an instruction body mid-turn.
- Full `aster skills` command set: `add`, `list`, `remove`, `use`, `find`,
  `update`, `init`.
  - `add` takes `owner/repo`, a git URL, or a local path, with an interactive
    wizard when the source is omitted, plus `--all`, `--only`, `--dry-run`, and
    `--force`.
  - Git sources use a sparse checkout that downloads only the chosen skill
    directories.
  - `find` searches GitHub and installs interactively.
  - `update` re-fetches from a lockfile that records where each skill came from.
  - `init` scaffolds a new `SKILL.md`.
- Seven skills ship with the repo: `aster-cli`, `aster-config`,
  `aster-chat-sessions`, `aster-fix-workflow`, `aster-review-ci`,
  `aster-skill-authoring`, and a `skills/README.md`.

#### Context compaction

- Long conversations are summarized automatically once they pass a character
  budget, keeping the most recent turns verbatim and replacing the head with a
  generated summary. The compaction is recorded in the transcript, so replaying
  a session shows where it happened.

#### Desktop

- Review results render as structured rows rather than raw text
  (`ReviewRow`, `ReviewTurn`, `review-format.ts`).
- Activity panel showing what the agent is doing.
- Per-message actions.
- Session handling wired to the new persistence layer.
- Substantial styling pass.

#### Repo tooling

- `scripts/bump-version.sh` bumps the Rust workspace and the desktop app in
  lockstep, taking `patch`, `minor`, `major`, or an explicit `X.Y.Z`.
- `docs/MEMORY.md` documenting the memory model.

### Changed

- Retry middleware is header-aware: waits are timed from `Retry-After` and
  `x-ratelimit-reset`, a rate-limited `403` (GitHub's secondary limit) counts as
  transient, and the whole thing is bounded by both an attempt count and a total
  wall-clock deadline. A bare `429` with no retry hint is treated as permanent —
  it is usually a billing error wearing a rate limit's clothes — so attempts are
  not wasted on it.
- Doc comments across the workspace cut to their load-bearing content.
- Release workflow and `Makefile` updated for the new crates.

[0.2.0]: https://github.com/Zfinix/aster/compare/v0.1.0...v0.2.0
[0.3.0]: https://github.com/Zfinix/aster/compare/v0.2.0...v0.3.0
