# Aster for VS Code and Cursor

An AI coding agent in your sidebar: ask about the code, let it make edits, and
run a verified code review that streams its findings into the chat.

Works in VS Code and Cursor.

## Getting started

1. Install the extension. The CLI it runs comes with it, so there is nothing
   else to download.
2. Open the panel with `cmd+shift+a` (`ctrl+shift+a` on Windows and Linux), pick
   a provider, and sign in or paste an API key.

That is the whole setup. If you already use Aster in the terminal, the panel
picks up your existing config and keys and skips step 2.

**Want `aster` in your terminal too?** Run **Aster: Install 'aster' Command in
PATH** from the command palette and it links the bundled binary into
`~/.local/bin`.

**On a platform without a prebuilt binary,** the panel shows an install card
with a button that fetches one for you. To do it by hand instead:

```bash
curl -fsSL https://withaster.dev/install | sh
```

Point `aster.binaryPath` at an existing binary if you would rather use your own.

## What you can do

**Chat.** Ask a question and the agent answers with your repo as its working
directory, so it can read and search the code. Type `@` to mention a file, or
drag one onto the composer from the explorer or your file manager. `alt+a`
sends the current selection with its line numbers.

**Edit.** The agent edits files directly. The chip next to the composer sets how
much it asks first:

| Mode | Behavior |
| --- | --- |
| Plan | Explores and presents a plan before touching anything |
| Manual | Asks before each edit |
| Auto | Applies safe edits, pauses on anything risky |
| Edit | Edits without asking (default) |
| Yolo | No guardrails |

Approvals appear inline in the chat. Files it changed are listed under the
reply, and clicking one opens it. Paths you protect in `aster.yaml` stay blocked
in every mode except Yolo.

**Review.** Hit **Review** to check your uncommitted work, or review a branch, a
git range, or a GitHub PR. Findings arrive as they are confirmed, one row each,
with the fix and a link to the line. Turn on `aster.publishDiagnostics` to get
them as squiggles and Problems entries too. The Aster icon in the activity bar
keeps the last review's findings as a sorted list.

Reviews stay in the chat, so you can follow up: "why is finding 2 critical?"

## The command menu

Everything the panel does lives in one filterable menu. Press `/` in an empty
composer, or `cmd+alt+k`.

| Section | Rows |
| --- | --- |
| | New, clear, or compact the conversation, resume a session, mention a file |
| Model | Switch model or provider, set effort, change mode |
| Repository | Review the working tree, a range, or a PR; uncommitted changes; status; memory; MCP servers |
| Skills | Every skill this session can see, including ones your plugins add |

Each row shows its current value on the right, so the menu doubles as a
readout of which model is live and what is turned on. **Switch model** reads
the catalog from your provider every time it opens, so you can search for a
model by name instead of remembering its id.

Status, uncommitted changes, memory, and compact all answer with a card in the
thread without spending a model call.

## Commands and shortcuts

| Command | |
| --- | --- |
| Aster: Open | `cmd+shift+a` |
| Aster: New Conversation | `cmd+alt+n` |
| Aster: Show Command Menu | `cmd+alt+k` |
| Aster: Reopen Session | `cmd+alt+r` |
| Aster: Insert @-Mention Reference | `alt+a` |

Use `ctrl` in place of `cmd` on Windows and Linux.

The panel can also be moved to a new tab, the primary editor, a new window, or
the side bar. Reviews have their own commands (review the branch, a range, or a
PR by number; cancel; clear findings), as does fixing a single finding.

## Settings

**Aster: Open Settings** opens a tab showing every `aster.yaml` key, its current
value, and where that value came from, editable in place. It is the same data
`aster config` prints in the terminal.

The extension adds four settings of its own:

- `aster.binaryPath` — use a different aster binary
- `aster.minConfidence` — hide findings below this confidence
- `aster.publishDiagnostics` — also report findings in the Problems tab (off by default)
- `aster.extraArgs` — extra arguments for every review

Everything else (provider, model, globs, analyzers, permissions) comes from
`aster.yaml` and your environment, exactly as it does for the CLI. A provider
picked in the panel applies to this panel only and never rewrites your config.

## Development

```bash
cd editors/vscode
bun install
bun run build      # tsc for the extension host, vite for the webview bundle
```

`build:host` compiles `src/` to `out/`; `build:webview` bundles the React panel
to `media/webview/`, loaded under a strict CSP. `src/protocol.ts` is the message
contract shared by both.

Press F5 for an Extension Development Host, or `bun run package` for a VSIX.
