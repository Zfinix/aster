---
name: aster-self
description: Aster's own files and settings, where config, keys, skills, sessions, and memory live and the command that changes each. Use for any question about Aster itself.
---

# Aster, about itself

You are Aster. Answer questions about your own files and settings from this map;
do not search the disk for them. When the answer depends on the current
directory, confirm with one command (`aster config path`, `aster key path`,
`aster status`) and quote what it prints. `aster <command> --help` is the source
of truth for flags.

## Where things live

`~` is the home directory (`%USERPROFILE%` on Windows). The data root is
`$XDG_DATA_HOME/aster` when that is set, otherwise `~/.local/share/aster`.

| What | Global | Project (repo root) |
| --- | --- | --- |
| Config | `~/.aster/aster.yaml` | `aster.yaml` (or `aster.yml`, `.aster.yaml`) |
| API keys | `~/.aster/.env` | `.env` |
| MCP servers | `~/.aster/mcp.json` | `.mcp.json`, or `mcp.servers` in `aster.yaml` |
| Model policy | `~/.aster/mom.yaml` | `mom.yaml`, then `.agents/mom.yaml` |
| Scheduled runs | `~/.aster/cron/` | `cron` block in `aster.yaml` |
| Skills | `~/.local/share/aster/skills/<name>/SKILL.md` | `.aster/skills/<name>/SKILL.md` |
| Sub-agents | `~/.local/share/aster/agents/` | `.aster/agents/` |
| Plugins | `~/.local/share/aster/plugins/` | `.aster/plugins/` |
| Instructions | `~/.local/share/aster/AGENTS.md` (also `CLAUDE.md`, `ASTER.md`) | `AGENTS.md`, `CLAUDE.md`, `ASTER.md` |
| Memory | `~/.local/share/aster/memory/` | `ASTER.md` |
| Sessions | `~/.local/share/aster/sessions/<repo-slug>/` | |
| Sign-ins | `~/.local/share/aster/credentials.json` | |

A project file wins over the global one. For settings the order is CLI flags,
then shell env, then the project `aster.yaml`, then the global one, then
defaults. A project skill shadows a global skill of the same name. API keys are
never read from `aster.yaml`.

## Changing things

Prefer these commands over hand-editing: they check the file still parses.

- Settings: `aster config list` shows every key, its value, and where it came
  from. `aster config set KEY VALUE --global|--local`, `aster config unset KEY`,
  `aster config edit --global|--local`. Plain `aster config` opens a form.
- Model and provider: `aster model` and `aster provider` list and switch them.
  The default model for every repo is
  `aster config set review.model <id> --global` (the key is `review.model`, not
  `model`).
- Keys: `aster key list`, `aster key set VAR` (prompts without echo),
  `aster key unset VAR`. These write `~/.aster/.env` by default; `--local`
  writes the repo `.env`. There is no `--global` flag on `aster key`.
- Skills: `aster skills add <owner/repo|path|agent> [-p]`, `aster skills list`,
  `aster skills remove NAME`, `aster skills init NAME`. A new skill can also be
  written straight to the skills folder above; it loads next session.
- Plugins: `aster plugins add|list|remove`.
- MCP: `aster mcp add|list|enable|disable|remove|login`, `aster mcp import` to
  copy servers from another coding tool.
- Memory: `aster remember "fact"`, `aster memory list|add|remove|show`.
- Instructions: edit the `AGENTS.md` for the scope you want.
- Binary: `aster upgrade`.

A change to config, skills, plugins, or MCP takes effect in the next session.
After editing a config file by hand, run `aster config list` to confirm it
still parses.
