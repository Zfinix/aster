# Aster for VS Code and Cursor

Run [Aster](https://github.com/zfinix/aster) from a chat panel in your editor's sidebar: ask questions about the code, kick off a review, and watch findings stream in. Confirmed findings also land as squiggles in the editor and entries in the Problems panel.

Works in VS Code and Cursor (Cursor consumes standard VS Code extensions).

## Requirements

The extension shells out to the `aster` CLI, so it must be installed and configured:

```bash
cargo install --path crates/aster-cli   # from the aster repo

export ASTER_API_KEY=sk-...
export ASTER_BASE_URL=https://openrouter.ai/api/v1
export ASTER_MODEL=openai/gpt-4o-mini
```

If `aster` is not on your PATH, point `aster.binaryPath` at it. The panel tells you when the binary is missing.

## The panel

The chat panel lives in the **secondary sidebar** (the right-hand pane, same place as Codex and Claude Code). Open it with **Open Aster Sidebar** from the editor's `…` menu or the command palette.

- Type a question to run one `aster chat` turn with the repo as cwd, so the agent can read and search your code. Review turns stay in the chat context, so "why is finding 2 critical?" works.
- Hit **Review** to review the working tree. Phases, verify progress, confirmed findings, and refuted candidates all stream into the thread; token spend and cost land at the end.
- Findings collapse to one row each. Expand for the description and fix, then click the location to jump there.

The Aster icon in the activity bar holds the **Findings** view: the last review's findings as a flat, severity-sorted list.

Contributing to the secondary sidebar needs VS Code 1.106+ (Cursor 3.x reports 1.128, so it qualifies). On older hosts the panel falls back into the activity bar container automatically.

### Modes

The chip next to the composer holds the same modes as the TUI, passed straight through as `--permission-mode`:

| Mode | Behavior |
| --- | --- |
| Plan | Explore the code and present a plan before editing |
| Manual | Ask for approval before each edit |
| Auto | Apply what passes the safety check, pause for anything risky |
| Edit (default) | Edit files without asking |
| Yolo | No guardrails, unrestricted |

Approval prompts appear inline in the thread; the answer goes back to the running turn over the CLI's stream protocol. Edited files are listed under the reply, click to open. Paths protected by `aster.yaml` `permissions` stay blocked in every mode but Yolo.

### The command menu

Everything the panel can do is in one filterable menu. Open it with the `/` button next to the composer, by pressing `/` in an empty composer, with `cmd+alt+k` / `ctrl+alt+k`, or from the editor's own palette as **Aster: Show Command Menu**. Type to filter; arrows and enter to pick.

| Section | Rows |
| --- | --- |
| | New conversation, Clear conversation, Compact conversation, Resume a session…, Mention a file… |
| Model | Switch model…, Switch provider…, Effort, Mode… |
| Repository | Review the working tree, Review a git range…, Review a GitHub PR…, Show uncommitted changes, Status, Memory, MCP servers… |
| Skills | Every skill the session can see, including the ones installed plugins contribute |

Rows carry their current value on the right, so the menu doubles as a readout: which model is live, which provider, how many MCP servers are on. **Effort** is set inline on its own row rather than behind another menu. A row ending in `…` opens a second panel: the model list, the provider catalog, the mode picker, or the MCP servers with their on/off state.

**Switch model…** reads the endpoint's own catalog each time it opens, so you search for a model instead of knowing its id. Search matches the readable name and the raw id alike, so `sonnet`, `anthropic`, and `claude-sonnet-5` all find the same row. A handful of vetted models sit under **Recommended**; the rest of the catalog follows. If what you typed matches nothing, the last row offers to use it as an id anyway, which is also the way out when an endpoint does not implement `/models` (the picker says so).

### Files

`@` in the composer searches the workspace: the file name is the row, the folder underneath it. You can also **drag files straight onto the composer** from the explorer, an editor tab, or your file manager; they land as `@`-mentions relative to the repo root. `alt+a` sends the active editor's selection over as a mention with its line range.

**Status**, **Show uncommitted changes**, **Memory**, and **Compact conversation** answer with a card in the thread. None of them go to the model, and none end up in the history the next turn sends. Picking a skill writes `Use the "<skill>" skill:` into the composer and leaves the task to you.

A provider picked here travels as `ASTER_BASE_URL` on the CLI runs this panel starts, so it never edits your `aster.yaml`. If that endpoint's own key is exported (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, and so on) the panel picks it up; otherwise it falls back to `ASTER_API_KEY` and says so.

## Editor commands

| Command | What it does |
| --- | --- |
| Aster: Open Aster Sidebar | Focuses the panel. Also an editor title action, which Cursor shows in the editor's `…` overflow menu |
| Aster: Review Current Branch | Reviews the current branch against its base |
| Aster: Review Git Range… | Reviews an explicit range, e.g. `main..HEAD` |
| Aster: Review GitHub PR… | Reviews a PR by number (needs `aster login` or `GITHUB_TOKEN`) |
| Aster: Cancel Review | Stops the running review |
| Aster: Clear Findings | Clears diagnostics and the Findings view |

## Settings

- `aster.binaryPath` — path to the aster binary (default: `aster` on PATH)
- `aster.minConfidence` — drop findings below this confidence; falls back to `aster.yaml`
- `aster.extraArgs` — extra args for every review, e.g. `["--no-index"]`

Provider, model defaults, include/exclude globs, analyzers, and edit permissions come from your environment and `aster.yaml`, same as the CLI. The model picked in the composer is passed as `--model` for chat turns.

## Development

```bash
cd editors/vscode
bun install
bun run build      # tsc for the extension host, vite for the webview bundle
```

`build:host` compiles `src/` (Node, CommonJS) to `out/`. `build:webview` typechecks and bundles `webview/` (React) to `media/webview/`, which the panel loads under a strict CSP. `src/protocol.ts` is the shared message contract and is compiled by both.

Open `editors/vscode` in VS Code and press F5 for an Extension Development Host, or `bun run package` for a VSIX to install via "Extensions: Install from VSIX…" (works in Cursor too).
