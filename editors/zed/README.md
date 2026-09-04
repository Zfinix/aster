# Aster in Zed

Two ways to use Aster from Zed.

## As an external agent (chat, edits, approvals)

Aster speaks the Agent Client Protocol over stdio with `aster acp`, so Zed can
drive the full agent from its External Agents menu: streamed replies, tool
calls, diffs, permission prompts, the plan/manual/auto/edit/yolo mode picker,
and provider, model, and effort pickers. The model picker uses the same
humanized names as the desktop and VS Code apps, with the provider's coding
shortlist first; switching provider reloads its model list.

Add it as a custom agent in Zed's `settings.json`:

```json
{
  "agent_servers": {
    "aster": {
      "type": "custom",
      "command": "aster",
      "args": ["acp"],
      "env": {}
    }
  }
}
```

Then pick **Aster** from the `+` menu in the agent panel. Flags worth knowing:

- `--permission-mode <plan|manual|auto|edit|yolo>` sets the mode every new
  thread starts in; the picker in Zed changes it per thread.
- `--model <MODEL>` overrides the configured model.
- `--no-mcp` skips connecting MCP servers when a thread opens.
- `--trace` echoes every protocol line to stderr for debugging.

Sign in first with `aster login` (or `aster init`); a thread opened without
credentials fails with that hint. Threads are recorded as Aster sessions, so
`aster --resume <ID>` reopens one in the terminal.

### Getting the Aster logo in the menu

Zed shows a terminal icon for every agent added through settings; only agents
from the ACP Registry carry their own icon. `registry/aster/` holds the entry
and 16x16 icon ready to submit to
[agentclientprotocol/registry](https://github.com/agentclientprotocol/registry):

1. Cut a release whose binaries include `aster acp`, and set that version in
   `agent.json` (the archive URLs and `cmd` paths follow the release naming).
2. Add each archive's checksum from the `.sha256` release assets as `sha256`.
3. Copy the directory into a fork of the registry as `aster/`, run
   `uv run --with jsonschema .github/workflows/build_registry.py`, and open a PR.

Once merged, Aster appears in Zed's menu for everyone, with its icon, and the
custom settings entry can go.

## As a slash command (review only)

This directory is also a Zed extension that registers one slash command:

- `/aster-review` runs `aster review` in the worktree and inserts the findings
  as text into the assistant panel.

### Install as a dev extension

1. Install the wasm target: `rustup target add wasm32-wasip1`
2. Build: `cargo build --target wasm32-wasip1` (from this directory)
3. In Zed: command palette, then `install dev extension`, and point it at this
   directory. Zed builds and loads it.

The `aster` binary must be on your PATH. From the aster repo, `make install`.
