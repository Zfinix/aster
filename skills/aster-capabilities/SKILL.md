---
name: aster-capabilities
description: The complete map of what Aster can do and the exact command, flag, slash command, key, or config for each: install and sign-in, the chat TUI, permission modes and rules, every agent tool, review and fix, sessions, memory, config/keys/provider/model, mom.yaml routing, skills, plugins, MCP, web tools, sub-agents and `aster run`, cron and reminders, Telegram remote, /goal loops, and the browser, VS Code, Zed, and desktop surfaces. Use when someone asks "what can Aster do", "how do I X with aster", "is there a command for", or before recommending a workflow, so the answer names a real capability instead of guessing.
---

# Everything Aster can do

Aster is a self-hostable agent harness for software work: one `aster` binary that chats, edits, runs commands, reviews diffs, remembers, and runs on a schedule, in a terminal, a browser, VS Code, Zed, or a desktop window. Every subcommand takes `--json` (before or after it) and turns errors into `{"ok":false,"error":…}`. Every command that runs a model takes `--model` and `--effort off|low|medium|high|xhigh|max|ultra`.

When a question is "can Aster do X", find X below and answer with the command. When unsure whether a flag still exists, run `aster <cmd> --help`; it is the source of truth.

## Install, upgrade, sign in

```sh
curl -fsSL https://withaster.dev/install | sh     # binary install
cargo install --path crates/aster-cli              # from source, Rust 1.85+
aster upgrade                                      # swap in the latest release
aster upgrade --version 0.4.0 --force              # pin or reinstall
aster init                                         # wizard: provider + key into ~/.aster
aster init --local                                 # config in this repo instead
aster init --yes --force                           # default aster.yaml, no questions
aster login                                        # GitHub (for --pr and --comment)
aster login codex | openrouter | zai               # provider sign-in via browser
aster logout                                       # drop every stored login
aster status                                       # what the next turn runs with
aster announce [--dismiss id,id]                   # undismissed release notes as JSON
```

Config lives in `./aster.yaml` (repo) merged over `~/.aster/aster.yaml`; keys live in `.env` files, never in yaml. Precedence: CLI flags > env > aster.yaml > defaults.

## Chat

```sh
aster                                # TUI in the current repo
aster "why does the build fail?"     # one-shot, prints and exits (--print / -p)
echo "explain this repo" | aster     # piped stdin is the prompt
aster --continue                     # resume this repo's latest session
aster --resume [ID]                  # pick a session, or resume one by id
aster --session ID "next question"   # persist into a named session
aster --messages-json history.json   # caller owns the transcript (- for stdin)
aster --compact --messages-json h.json   # fold history into a summary, print it
aster --stream                       # NDJSON events out, approval replies in
aster --no-tools                     # plain chat, no read/search/edit
aster --no-mcp                       # start instantly; /mcp connects later
aster --allow-edits                  # let a scripted turn edit files
aster --permission-mode plan|manual|auto|edit|yolo
```

Chat works outside a repo too, with less to look at.

### Keys in the TUI

| Key | Does |
| --- | --- |
| `enter` | Send; while a turn runs it queues the message |
| `esc` | Interrupt the running turn; `esc esc` quits |
| `shift+tab` | Step to the next permission mode |
| `ctrl+j` | Newline without sending |
| `ctrl+o` | The `/switch` panel |
| `@` | Mention a repo file (images and documents attach as content) |
| `↑ ↓` | Move the cursor, then step through past messages |

### Slash commands

| Command | Does |
| --- | --- |
| `/switch` | Thinking, mode, effort, model, provider in one panel |
| `/model [id]`, `/m` | Switch model or open the picker |
| `/provider [id\|url]`, `/p` | Switch endpoint, then pick a model |
| `/mode [name]` | plan, manual, auto, edit, yolo |
| `/effort [level]` | Set or cycle the reasoning budget |
| `/thinking` | Print the model's thinking in full or not |
| `/yolo` | Toggle yolo (guardrails off, red theme) |
| `/mom [resume]` | Show the mom.yaml policy state; `resume` re-arms it after a manual `/model` |
| `/resume`, `/r` | Reopen a saved session |
| `/clear`, `/c` | Start fresh |
| `/compact` | Fold earlier turns into a summary |
| `/status` | Session, model, context, token usage |
| `/diff`, `/d` | Uncommitted changes |
| `/mcp` | MCP servers and their tools; connects them if `--no-mcp` skipped it |
| `/skills` | Pick a skill to load |
| `/memory` | What Aster remembers here |
| `/theme` | Dark or light |
| `/welcome` | Show or hide the session header, saved to `ui.welcome` |
| `/goal <condition>` | Loop turns until a separate judge model says the condition is met (see Goals) |
| `/help`, `/quit` | The list above; exit |

Installed skills that declare a command also appear as slash commands (for example `/write-tests`).

## Permission modes and rules

One setting gates edits and commands: `shift+tab` in chat, `--permission-mode` per run, `permissions.mode` in aster.yaml.

| Mode | Does |
| --- | --- |
| `plan` | Explore and present a plan. Never edits or runs |
| `manual` | Ask before every edit and command (denies when headless) |
| `auto` | Edit and run, pause on risky commands (sudo, rm, curl, ssh…) |
| `edit` | Like auto but commands are trusted; only a rule stops one. Default |
| `yolo` | No rules, no sandbox. Asks first |

Rules use one language for all tools, precedence deny > ask > allow > built-ins > mode:

```yaml
permissions:
  mode: edit
  allow: ["Bash(cargo test:*)", "Edit(src/**)", "Read(**/.env)"]
  ask:   ["Edit(migrations/**)", "Bash(git push:*)"]
  deny:  ["Bash(npm publish:*)", "Edit(infra/**)"]
  use_default_rules: true          # asks on .git/**, workflows, hooks; refuses secrets
  additional_directories: ["~/Downloads"]   # readable outside the repo without asking
  allow_credentials: ["gh:~/.config/gh"]    # sandbox credential dirs a command may read
```

A `Bash` rule matches inside `bash -lc "…"` too. Outside yolo, commands run in a sandbox: writes limited to the repo and temp, secrets stripped from the environment, network on with per-domain approval.

## What the agent can do inside a turn

These are the tools the model calls. Knowing them tells you what to ask for.

| Tool | Does |
| --- | --- |
| `read_file` | Read with line numbers, optional `start_line`/`end_line`. PDF, Word, PowerPoint, Excel, ODF, EPUB, RTF convert to Markdown. Outside-repo and `~` paths prompt |
| `list_files`, `find_files`, `search_files` | Directory listing, glob search, regex content search (smart-case, grouped by file) |
| `explore` | Several lookups in one call |
| `edit_file` | Search-and-replace edits, gated by the permission mode |
| `ast_grep`, `ast_edit` | Structural search and rewrite over syntax trees |
| `lsp_definitions`, `lsp_references`, `lsp_diagnostics` | Language-server navigation and errors |
| `run_command`, `run_tests` | Shell in the sandbox (timeout `agent.command_timeout_secs`); the test runner |
| `agent` | Spawn sub-agents from the roster (see Sub-agents) |
| `ask_user` | Structured question with options |
| `update_plan`, `exit_plan_mode` | Visible step plan; the plan-mode approval handshake |
| `remember`, `recall`, `forget` | Durable memory (see Memory) |
| `read_skill` | Load a skill's body on demand |
| `open_preview` | Open a URL or file in the browser (`ASTER_NO_BROWSER=1` prints the URL instead) |
| `security_scan` | Static security scan of the working tree |
| MCP tools | Anything a configured server exposes, injected progressively; `web/search` and `web/extract` ship built in |

Limits: `agent.max_tool_rounds` (60), `agent.command_timeout_secs` (300), `agent.compact_budget_chars` (192000), env `ASTER_MAX_TOOL_ROUNDS`, `ASTER_COMMAND_TIMEOUT`, `ASTER_COMPACT_BUDGET`.

## Review

Hypothesize, retrieve evidence from a local symbol index, verify by trying to refute, shape. Findings carry file, line, severity, category, title, suggestion, confidence.

```sh
aster review                               # current branch
aster review --range main..HEAD
git diff HEAD~1 | aster review --diff -
aster review --pr 42 [--repo owner/repo] [--token T]
aster review --pr 42 --comment [-y]        # post inline PR comments
aster review --tui                         # browse findings
aster review --json                        # data for CI
aster review --stream                      # NDJSON progress for UIs
aster review -i "src/**/*.rs" -x "**/*.lock" --min-confidence 0.6
aster review --no-index                    # faster, less evidence
aster review --effort high
```

Config under `review:` in aster.yaml: `model`, `hypothesis_model`, `verify_model`, `base_url`, `min_confidence`, `max_diff_bytes`, `analyzers: [semgrep, ast-grep]`, `astgrep_rules`, `effort`, `web_search`, `focus_areas`, `include`, `exclude`. `model: auto` picks from OpenRouter's live rankings (`ASTER_ROUTER_TIER=cheap|balanced|strong`, see `aster model router`). Env: `ASTER_VERIFY_MODEL`, `ASTER_HYPOTHESIS_MODEL`, `ASTER_VERIFY_CONCURRENCY`.

## Fix

```sh
aster review --json | aster fix                # dry run: preview the patches
aster review --json | aster fix --apply        # write them
aster fix --findings-json findings.json --apply --repo-root .
```

## Sessions

Every chat, ACP thread, and scheduled run is a session under `~/.aster`.

```sh
aster sessions [list] [--all]          # this repo, or every project
aster sessions show ID
aster sessions rename ID "title"
aster sessions delete ID
aster sessions prune [--keep N] [--older-than DAYS]   # empties always go
aster sessions import [--from claude|codex|cursor|opencode|hermes] [--dry-run]
```

## Memory

Two layers: short project facts always in context, named blocks listed by title and read on demand. The agent writes with `remember`, reads with `recall`, retracts with `forget`.

```sh
aster memory [list]
aster memory add "we deploy from release, never main"
aster memory add "prefers terse replies" --title tone
aster memory show tone
aster memory remove tone
```

## Config, keys, provider, model

```sh
aster config                          # form in a terminal, table when piped
aster config list | get KEY | set KEY VALUE [--global|--local] | unset KEY
aster config path | edit [--global|--local]
aster config providers | provider [ID --model M] | models [--capabilities] | model [ID]
aster config keys [--all] | key [VAR [VALUE]] [--local]

aster key list [--all] | get VAR | set VAR [VALUE] [--stdin] [--local] | unset VAR | path

aster provider list
aster provider use openai [--model gpt-5.5]   # endpoint and model together
aster model list [--capabilities] | use ID | recommended
aster model router [--tier cheap|balanced|strong]
```

Any OpenAI-compatible endpoint works. A key var named for the endpoint (`ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`) beats the shared `ASTER_API_KEY`. `ASTER_MODEL` and `ASTER_BASE_URL` in the shell outrank the file and are reported when set.

## mom.yaml: model routing per turn

Drop `mom.yaml` in the repo root or `~/.aster/`. Entries describe intent (`power`, `thinking`, `prefer`), `switch` rules fire on conditions like `planning`, `stuck`, `looping`, `model-down`, and a cheap router judges the rest. Switches show as `mom:` notes in chat.

```sh
aster mom check                 # validate, show what each entry resolves to
aster mom route "refactor the auth module"   # which entry the router would pick
```

Spec: `specs/mom.md`. Logs: `~/.aster/logs/mom-router.jsonl`, `mom-switches.jsonl`.

## Skills

Folders with a `SKILL.md`; titles are indexed, bodies load when relevant. Five core skills are built in and listed; eleven more about the agent's own conduct are internal and never shown. A skill installed under `<skills root>/internal/<name>/` is internal too. Ten optional skills are bundled off by default; `aster skills bundled` marks the internal ones, which install into `internal/`.

```sh
aster skills list [-p|-g]
aster skills add owner/repo | git-url | ./dir | claude-code|cursor|…  [-p] [-s name] [--all] [-y] [--force] [-l]
aster skills add                        # wizard listing skills found on this machine
aster skills find react [--owner org]   # search GitHub, install interactively
aster skills use owner/repo@skill       # print a skill without installing
aster skills bundled [name…] [--force]  # list or turn on optional built-ins
aster skills update [name…]
aster skills remove [name…] [--all] [-y]
aster skills init my-skill              # scaffold my-skill/SKILL.md
```

Skills you write are installed right after writing (`aster skills add ./dir --all --yes --force`), not saved to memory. Format rules: `aster-skill-authoring`.

## Plugins

Agent Plugins packages (skills plus MCP servers) install unchanged; their servers appear as `<plugin>/<server>`.

```sh
aster plugins add owner/repo | ./dir [-p] [--plugin name] [--all] [-y] [-l] [--force]
aster plugins list [-p|-g]
aster plugins remove name [--purge] [-y]
aster plugins validate [./dir]
```

## MCP servers

Declared under `mcp.servers` in aster.yaml: `command`+`args`+`env` for local, `url` (+`headers`, `type: sse`) for remote. Tools are injected progressively within `mcp.context_tokens` / `mcp.inventory_percent`; `mcp.tools.allow|deny` filter by `server/tool` with globs. WebMCP bridges tools a page registers in your own Chrome (`mcp.webmcp`, Chrome started with `--remote-debugging-port=9222`).

```sh
aster mcp list [--no-connect]
aster mcp enable NAME | server/tool
aster mcp disable NAME | server/tool     # globs allowed
aster mcp import [--from claude|codex|cursor|opencode|hermes]
aster mcp remove [NAME]
aster mcp login NAME                     # OAuth for a remote server
```

## Web as Markdown

`search` and `extract` need no key. `crawl` needs `FIRECRAWL_API_KEY`, `CONTEXT_DEV_API_KEY`, or the Cloudflare pair; `sitemap` and `screenshot` need `CONTEXT_DEV_API_KEY`. `aster key list --all` shows every web key and what it buys. The same five are the agent's `web/*` tools.

```sh
aster web search "query" [--limit 5] [--region us-en] [--safesearch strict]
aster web extract https://…
aster web crawl https://… [--max-pages 100] [--max-depth N] [--url-regex R] [--main-content-only] [--follow-subdomains] [--no-pdfs] [--stop-after-ms 80000]
aster web sitemap docs.rs [--max-links 500] [--url-regex R]
aster web screenshot https://… [--full-page]
```

## Sub-agents and headless runs

The `agent` tool fans work out to personas: `scout` (read-only recon), `cartographer` (architecture mapping), `sentinel` (skeptical review), `forge` (applies a described change), `scribe` (docs only), `prism` (synthesis of collector reports). Custom agents are an `AGENT.md` directory; see `docs/SWARM.md`. Limits under `agents:` (`max_concurrent`, `max_per_turn`, `agent_timeout_secs`, `collector_model`).

```sh
aster run sentinel "review yesterday's commits on main" [--notify] [--cwd DIR] [--schedule NAME]
```

`aster run --help` mentions `aster agents`; that command does not exist, the roster above is the list.

## Scheduled runs and reminders

```yaml
schedules:
  - name: nightly-review
    cron: "0 9 * * *"        # five fields, local time
    agent: sentinel
    task: "review yesterday's commits on main"
    notify: true
```

```sh
aster cron install | list | remove NAME | run NAME   # launchd on macOS, cron on Linux
aster remind "stand up" "in 30m"                     # or "in 2h", "at 18:00"
```

## Remote control over Telegram

```sh
ASTER_TELEGRAM_TOKEN=… ASTER_REMOTE_USERS=123,456 aster remote telegram [--mode manual]
aster remote telegram --token T --user 123 --mode auto
```

Long-polling, no public URL. Approval prompts arrive as buttons.

## Goals: loop until a judge agrees

In chat, `/goal <condition>` keeps the turn loop running; after each turn a separate cheap judge model rules `met`, `not_yet`, or `impossible`, so the worker never certifies itself. `ASTER_GOAL_MAX_TURNS` caps the loop (default 20).

## Surfaces

| Surface | How |
| --- | --- |
| Terminal | `aster` |
| Browser | `aster serve [--port 8080] [--host 0.0.0.0] [--no-open]`; default `http://localhost:4187`, token in the URL off loopback |
| VS Code / Cursor | `editors/vscode` extension: sidebar, tab, or window; `cmd+shift+a` opens, `cmd+alt+k` command menu, `cmd+alt+n` new, `cmd+alt+r` reopen, `alt+a` sends the selection as an @-mention; review findings into the Problems pane; settings `aster.binaryPath`, `aster.minConfidence`, `aster.publishDiagnostics`, `aster.extraArgs` |
| Zed | `aster acp [--permission-mode M] [--model M] [--no-mcp] [--trace]` as a custom agent server in `settings.json` |
| Desktop | `desktop/` Tauri app around the same CLI |

All surfaces resolve model, provider, mode, and keys from the same aster.yaml, so `provider use` switches everywhere at once.

## Environment-only knobs

`ASTER_API_KEY`, `ASTER_BASE_URL`, `ASTER_MODEL`, `ASTER_EFFORT`, `ASTER_MAX_TOKENS` (`off` lifts the cap), `ASTER_SEED`, `ASTER_TIMEOUT_SECS`, `ASTER_MAX_RETRIES`, `ASTER_DEADLINE_SECS`, `ASTER_WEB_SEARCH=1`, `ASTER_NO_BROWSER`, `ASTER_NO_UPDATE_CHECK`, `ASTER_EDITOR`, `ASTER_UI_DIR`, `ASTER_MCP_EXTRA`, `ASTER_VISION_MODEL`, `ASTER_ASTGREP_RULES`, `ASTER_PRICE_PROMPT_PER_M`, `ASTER_PRICE_COMPLETION_PER_M`, `OTEL_EXPORTER_OTLP_ENDPOINT`. `.env.example` lists them with notes.

## When something fails

```sh
aster status                                          # what will run, and from where
tail -5 ~/.aster/logs/provider-errors.jsonl | jq      # every failed provider request
aster config list                                     # each setting and its source
aster mcp list --no-connect                           # servers without spawning them
```

## Where to read more

`docs/CONFIG.md` (every key), `docs/MCP.md`, `docs/MEMORY.md`, `docs/SWARM.md`, `docs/PLUGINS.md`, `docs/ALGORITHM.md` (review), `docs/ANALYZERS.md`, `specs/mom.md`. Sibling skills go deeper on one area: `aster-cli`, `aster-config`, `aster-chat-sessions`, `aster-review-ci`, `aster-fix-workflow`, `aster-planning`, `aster-skill-authoring`.
