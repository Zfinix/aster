
# ✳ Aster

**An open-source coding agent you run yourself, with the model you choose.**

[![CI](https://github.com/zfinix/aster/actions/workflows/ci.yml/badge.svg)](https://github.com/zfinix/aster/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](./LICENSE)

![Aster demo animation](./aster.gif)

Aster is a terminal agent that reads your code, answers questions, edits files,
runs commands, and reviews your changes. It works with any OpenAI-compatible
provider: OpenRouter, OpenAI, Groq, Anthropic, or a model running on your own
machine.

Three ideas shape it:

- **You own the whole thing.** Your key, your model, your machine. Sessions,
  memory, and skills are plain files on your disk. There is no hosted control
  plane, no vector database, and no telemetry. Point it at a local model and it
  works with no network at all.
- **A harness, not a prompt.** Tools, permissions, history, memory, and
  retrieval are shared infrastructure. Chat, review, and fix all inherit them,
  so a new capability is a small addition rather than another agent rebuilding
  the same scaffolding.
- **Make it prove things.** An agent that sounds right is not the same as one
  that is right. Aster spends extra model effort trying to disprove its own
  findings before showing them to you, and it tells you when it ran out of room
  instead of quietly wrapping up.

> **Status: early, building in the open.** Chat, review, memory, skills,
> permissions, and MCP have landed. Expect rough edges.

## Install

```bash
curl -fsSL https://withaster.dev/install | sh
```

Or build it yourself (Rust 1.85 or newer):

```bash
git clone https://github.com/zfinix/aster && cd aster
cargo install --path crates/aster-cli
```

## Start here

```bash
aster init     # pick a provider, paste your key, done
cd your-repo
aster          # opens the chat
```

`aster init` writes `~/.aster/aster.yaml` and stores your key in `~/.aster/.env`,
so it applies to every repo. Prefer environment variables? Skip `init` and export
these instead:

```bash
export ASTER_API_KEY=sk-...
export ASTER_BASE_URL=https://openrouter.ai/api/v1
export ASTER_MODEL=anthropic/claude-sonnet-5
```

Then just talk to it:

```text
❯ where does the retry logic live?
❯ add a test for the empty-input case
❯ why is that finding critical?
```

Aster reads files, searches the repo, runs commands, and edits code when you let
it. Ask it something outside a repo and it still works, it just has less to look at.

## The chat

| Key | What it does |
| --- | --- |
| `enter` | Send. `esc` interrupts a running turn. |
| `esc esc` | Quit (two presses, so a stray one does not). |
| `shift+tab` | Step to the next permission mode. |
| `ctrl+j` | Newline without sending. |
| `@` | Mention a file from this repo. |
| `↑` | Step back through what you have sent. |

Type `/` for commands:

| Command | What it does |
| --- | --- |
| `/model` | Switch model, or pick from what the provider serves. |
| `/provider` | Switch to a different endpoint, then pick a model. |
| `/mode` | Choose how freely the agent edits (also `shift+tab`). |
| `/effort` | Reasoning budget: `off`, `low`, `medium`, `high`. |
| `/resume` | Reopen one of this repo's earlier sessions. |
| `/clear` | Start fresh. |
| `/help` | Everything above, in the terminal. |

Outside the TUI:

```bash
aster chat "why is finding 2 critical?"   # one answer, then exit
echo "explain this repo" | aster          # piped input is the prompt
aster chat --continue                     # pick up the last session
aster chat --resume                       # choose a session from a list
```

## How much it is allowed to do

One setting decides how freely the agent edits. Change it with `shift+tab`
mid-chat, `--permission-mode` for one run, or `permissions.mode` in `aster.yaml`.

| Mode | What it does |
| --- | --- |
| `plan` | Explores and proposes. Never edits. |
| `manual` | Asks before every edit. |
| `auto` | Edits what is safe, stops to ask about anything risky. |
| `edit` | Edits without asking (the default). |

Whatever the mode, commands run in a sandbox: writes are limited to the repo and
temp directories, and secrets are stripped from the environment. (`/yolo` turns
the sandbox off. It asks three times first, and turns the chat red.) Writes to
`.git/`, workflow files, and hooks are blocked, and reads of key and env files
are blocked. Widen or narrow any of it under `permissions` in
[`aster.yaml.example`](./aster.yaml.example).

## Review your changes

Review is Aster's most developed capability. It does not just ask a model to
skim a diff, it tries to disprove what the model claims.

```bash
aster review                              # the current branch
aster review --range main..HEAD           # an explicit range
git diff HEAD~1 | aster review --diff -   # a diff on stdin
aster review --pr 42                      # a GitHub PR (run `aster login` first)
aster review --pr 42 --comment            # post the findings as PR comments
aster review --tui                        # browse findings interactively
aster review --json | aster fix --apply   # let it fix what it found
```

A finding looks like this:

```text
  ✳ 1 finding worth your attention.

  HIGH  correctness  1/1  74%
  Unchecked index can panic on an empty slice
  crates/aster-index/src/grep.rs:58
```

With `--json`, the same thing comes out as data for CI or another tool:

```json
[
  {
    "file_path": "crates/aster-index/src/grep.rs",
    "line": 58,
    "severity": "high",
    "category": "correctness",
    "title": "Unchecked index can panic on an empty slice",
    "suggestion": "Handle the PoisonError instead of unwrapping.",
    "confidence": 0.74
  }
]
```

### How review works

```mermaid
flowchart LR
    A[HYPOTHESIZE] --> B[RETRIEVE] --> C[VERIFY] --> D[SHAPE]
```

1. **Hypothesize.** A cheap model over-produces candidate defects from the diff.
   A candidate without a concrete failure scenario is dropped.
2. **Retrieve.** Pull only the evidence that candidate needs: the changed hunk, a
   window of source, the enclosing symbol, and references from a local SQLite and
   FTS5 symbol index. No repo-wide walk.
3. **Verify.** A second call, prompted to *refute*, kills plausible-but-wrong
   findings. A candidate survives only above `--min-confidence` (default 0.5).
   Point `ASTER_VERIFY_MODEL` at a stronger model for a real second opinion.
4. **Shape.** Deduplicate, rank by severity times confidence, and emit.

The expensive model is spent only on challenging what survived, never on the
whole diff. Full write-up in [docs/ALGORITHM.md](./docs/ALGORITHM.md).

One caveat worth knowing: the confidence gate filters the verifier's
*self-reported* confidence. That is a useful heuristic, not a calibrated
probability, so treat the number as a ranking signal rather than odds.

## The rest of the commands

| Command | What it is for |
| --- | --- |
| `aster memory` | Facts Aster should keep between sessions. `add`, `list`, `show`, `remove`. |
| `aster sessions` | Past conversations. `list`, `show`, `delete`, `prune`. |
| `aster skills` | Reusable instructions the agent loads on demand. `add`, `list`, `find`, `remove`. |
| `aster mcp` | MCP servers that give the agent more tools. `list`, `enable`, `disable`. |
| `aster web` | Fetch a page or crawl a site as Markdown. |
| `aster fix` | Turn review findings into edits. Dry run unless you pass `--apply`. |
| `aster login` | Link GitHub, for reviewing and commenting on PRs. |

Two flags work everywhere, before or after the subcommand: `--json` for
machine-readable output, and `--effort` for the reasoning budget.

```bash
aster --json sessions list
aster --effort high review
```

### Memory

```bash
aster memory add "we deploy from the release branch, never main"
aster memory add "prefers terse replies" --title tone
aster memory list
aster memory remove tone
```

Short facts go into project memory, which is always in context. Named blocks are
listed by title and read in full only when the agent needs them.

### Skills

Skills are folders with a `SKILL.md` telling the agent how to do something
specific. Aster reads the titles and loads the body only when it is relevant.

```bash
aster skills find react          # search GitHub for skills
aster skills add owner/repo      # install from a repo
aster skills add ./my-skill -p   # install into this project only
aster skills add claude-code     # import from another agent on this machine
aster skills list
```

Any agent key from the [skills](https://github.com/vercel-labs/skills) registry
works as a source, so skills already installed for Claude Code, Cursor, Codex,
Gemini CLI, and the rest come across as-is. `aster skills add` with no source
lists the ones it finds installed.

### MCP servers

MCP servers add tools like a browser or a GitHub client. Declare them under
`mcp.servers` in `aster.yaml`, then:

```bash
aster mcp list              # what is configured, and what it offers
aster mcp enable chrome     # turn one on
aster mcp disable chrome    # turn it off, keep the config
```

Tools are injected progressively, so a large server does not eat your context.
See [docs/MCP.md](./docs/MCP.md).

## Configuration

Order of precedence: **CLI flags, then environment, then `aster.yaml`, then
defaults.** API keys are read from the environment only, never from the yaml.

`aster.yaml` is read from your repo root, then `~/.aster/aster.yaml` for
everything else. Copy [`aster.yaml.example`](./aster.yaml.example) to get started.

| Env var | What it does | Default |
| --- | --- | --- |
| `ASTER_API_KEY` | Your provider key (required) | none |
| `ASTER_BASE_URL` | Any OpenAI-compatible endpoint | `https://openrouter.ai/api/v1` |
| `ASTER_MODEL` | The model to use | `openai/gpt-4o-mini` |
| `ASTER_EFFORT` | Thinking budget: `off`, `low`, `medium`, `high` | `low` |
| `ASTER_MAX_TOOL_ROUNDS` | Tool calls in one turn before it must answer | `60` |
| `ASTER_COMMAND_TIMEOUT` | Seconds a single command may run | `300` |
| `ASTER_VERIFY_MODEL` | A stronger model for review's verify pass | same as `ASTER_MODEL` |
| `ASTER_HYPOTHESIS_MODEL` | A cheap model for review's first pass | same as `ASTER_MODEL` |
| `ASTER_MAX_TOKENS` | Cap generated tokens (`off` disables) | `8000` |
| `ASTER_SEED` | Fixed sampling seed (`off` disables) | `0` |

[`.env.example`](./.env.example) lists every variable with notes.

## Going deeper

The three ideas at the top are the short version. The long version, with the
diagrams and the reasoning behind each decision, lives in the docs:

| Doc | What it covers |
| --- | --- |
| [ARCHITECTURE.md](./docs/ARCHITECTURE.md) | How the crates fit together and what a turn does end to end. |
| [ALGORITHM.md](./docs/ALGORITHM.md) | The review pipeline and its cost model. |
| [HARNESS.md](./docs/HARNESS.md) | Sessions, memory, approvals, and delegation. |
| [MEMORY.md](./docs/MEMORY.md) | What Aster remembers, and how it is disclosed. |
| [MCP.md](./docs/MCP.md) | Progressive tool injection: one bridge, schemas on demand. |
| [ANALYZERS.md](./docs/ANALYZERS.md) | Wiring semgrep and ast-grep into review. |
| [ROADMAP.md](./docs/ROADMAP.md) | What is next. |

## Repository layout

```text
crates/
  aster-cli/         the `aster` command-line interface
  aster-ai/          provider-agnostic OpenAI-compatible chat client
  aster-harness/     the verification-first review pipeline
  aster-index/       code index: SQLite + FTS5 + ripgrep
  aster-analyzers/   static analysis (semgrep / ast-grep)
  symbol-extractor/  tree-sitter symbol extraction (14 languages)
  aster-persist/     sessions and project memory
  aster-skills/      on-demand instructions
  aster-agents/      specialized agent definitions
  aster-policy/      read, write, and command permissions
  aster-mcp/         progressive MCP tool injection
  aster-models/      shared domain types
desktop/             the desktop app (Tauri)
editors/vscode/      the VS Code extension
docs/                architecture, algorithm, memory, MCP, roadmap
```

## Contributing

Contributions are welcome. [CONTRIBUTING.md](./CONTRIBUTING.md) has the dev setup
and the three checks CI runs: `fmt`, `clippy`, `test`. For security issues, see
[SECURITY.md](./SECURITY.md).

## License

[Apache-2.0](./LICENSE). See [NOTICE](./NOTICE).
