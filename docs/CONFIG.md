# Configuration

Aster's configuration file is `aster.yaml`. This is the complete reference for
it. Every key is optional, and a file with no keys at all behaves exactly like
no file.

[`aster.yaml.example`](../aster.yaml.example) is a commented starting point.
`aster init` writes a smaller version of it wired to the provider you pick.

## Where the file lives

Aster reads two files and layers them:

1. **Global**: `~/.aster/aster.yaml`
2. **Project**: the first of `aster.yaml`, `aster.yml`, `.aster.yaml` that
   exists in the repo root

The project file is layered over the global one. How they combine differs by
section and is described in [Merging](#merging).

A file that fails to parse is an error, not a skipped file, so a typo never
changes behaviour silently. Unknown keys are rejected the same way: misspell
`min_confidence` and the run stops rather than ignoring the value you meant to
set.

## Precedence

CLI flags, then shell environment, then the project file, then the global file,
then built-in defaults. A non-empty environment variable beats the file; an
empty one counts as unset.

API keys are never read from `aster.yaml`. They come from `ASTER_API_KEY` or
`OPEN_ROUTER_API_KEY` in the environment, which `aster init` writes to `.env`
next to the config, or from `aster login` for GitHub.

## `review`

Despite the name, this section configures the model for everything: chat, `aster
review`, and `aster fix` all resolve their provider from here. Only the keys
marked "review only" are limited to the review pipeline.

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `model` | string | `openai/gpt-4o-mini` | Fallback for any stage without its own override. Env `ASTER_MODEL`, flag `--model`. |
| `base_url` | string | `https://openrouter.ai/api/v1` | Any OpenAI-compatible endpoint. Env `ASTER_BASE_URL`. |
| `effort` | `off` \| `low` \| `medium` \| `high` | `low` | Reasoning budget for thinking models. Env `ASTER_EFFORT` or `ASTER_REASONING_EFFORT`, flag `--effort`. |
| `web_search` | bool | `false` | OpenRouter web search: the agent gets a server tool it calls when it needs one, review stages get the `web` plugin, which searches on every request whether or not the diff calls for it. No effect on other endpoints. Env `ASTER_WEB_SEARCH` (`1`, `true`, `yes`, `on`). |
| `hypothesis_model` | string | same as `model` | Review only. Cheap, high-recall model for the first pass. Env `ASTER_HYPOTHESIS_MODEL`. |
| `verify_model` | string | same as `model` | Review only. Independent model for the adversarial verify pass. Env `ASTER_VERIFY_MODEL`. |
| `min_confidence` | float 0.0-1.0 | `0.5` | Review only. Findings below it are dropped. Flag `--min-confidence`. |
| `max_diff_bytes` | int | `200000` | Review only. Diffs longer than this are truncated before the model sees them. |
| `analyzers` | list of string | `[]` | Review only. Static backends whose findings also flow through verification: `semgrep`, `ast-grep`. Env `ASTER_ANALYZERS` (comma-separated) replaces the list. See [ANALYZERS.md](./ANALYZERS.md). |
| `astgrep_rules` | string | none | Review only. Repo-relative path to an ast-grep rule YAML. An unreadable path warns and runs without rules. Env `ASTER_ASTGREP_RULES` names the same file. |
| `focus_areas` | list of string | `[]` | Review only. Defect classes the hypothesis pass is biased toward, e.g. `correctness`, `security`. |
| `include` | list of glob | `[]` | Review only. Empty means everything. Flag `--include`/`-i`. |
| `exclude` | list of glob | `[]` | Review only. Added to the built-in list below. Flag `--exclude`/`-x`. |

`exclude` is always unioned with a built-in list, so lockfiles, minified assets,
source maps, snapshots, and `dist/`, `build/`, `out/`, `node_modules/`,
`vendor/`, `target/`, and VCS directories stay out of a review whether you list
them or not.

## `permissions`

Gates what the agent may write, read, and run. Applies to `aster chat
--allow-edits` and `aster fix --apply`.

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `mode` | `plan` \| `manual` \| `auto` \| `edit` \| `yolo` | `edit` | What happens to an action no rule matched. Flag `--permission-mode`. |
| `allow` | list of rule | `[]` | Runs without asking. |
| `ask` | list of rule | `[]` | Always confirmed first. |
| `deny` | list of rule | `[]` | Refused outright. |
| `use_default_rules` | bool | `true` | Whether the built-in rules below apply at all. |
| `additional_directories` | list of path | `[]` | Directories outside the repo the agent may read without asking. Absolute or `~`-relative. |

### Rules

`allow`, `ask`, and `deny` all hold rules in one language:

| Rule | Matches |
| --- | --- |
| `Edit(<glob>)` | writing a path |
| `Read(<glob>)` | reading a path |
| `Bash(<command>:*)` | a command line starting with `<command>` |
| `Bash(<command>)` | that command line exactly |
| `Edit`, `Read`, `Bash` | everything that tool does |

```yaml
permissions:
  allow: ["Bash(cargo test:*)", "Edit(src/**)"]
  ask:   ["Edit(migrations/**)", "Bash(git push:*)"]
  deny:  ["Bash(npm publish:*)", "Edit(infra/**)"]
```

A `Bash` rule is matched against the command line **and every command inside a
shell script it carries**, so `Bash(sudo:*)` still fires on `bash -lc "cargo
build && sudo make install"`. Leading environment assignments are ignored, and a
shell nested in a shell is followed a few levels down. A prefix ends at a word
boundary, so `Bash(rm:*)` does not also match `rmdir`.

### Rule precedence

1. `deny`
2. `ask`
3. `allow`
4. the built-in rules
5. `mode`

User rules come before the built-ins, so a single `allow` entry overrides one of
them without disabling the rest. To read `.env` files but keep every other
secret protected:

```yaml
permissions:
  allow: ["Read(**/.env)"]
```

### Built-in rules

Ask before writing, since anything here runs as code later: `.git/**`,
`**/.git/**`, `.github/workflows/**`, `.husky/**`.

Ask before running, **in every mode but `edit`**: privilege escalation (`sudo`,
`doas`, `su`), destructive filesystem operations (`rm`, `rmdir`, `dd`, `mkfs`,
`shred`), permission and process control (`chmod`, `chown`, `chgrp`, `kill`,
`killall`, `pkill`), system control (`shutdown`, `reboot`, `halt`, `systemctl`,
`launchctl`), and network egress (`curl`, `wget`, `nc`, `ssh`, `scp`, `rsync`).
Pausing on these is the whole difference between `auto` and `edit`; an `ask`
rule of your own fires in `edit` too.

Refuse to read, since a secret cannot be taken back out of the model's context:
`**/.env`, `**/.env.*`, `**/*.pem`, `**/*.key`, `**/id_rsa*`, `**/*.p12`,
`**/*.pfx`, `**/credentials.json`, `**/secrets.*`.

`use_default_rules: false` drops all three sets at once.

### The modes

- `plan` explores and proposes, never edits and never runs a command
- `manual` confirms every edit and command, so it needs the TUI or `--stream`
- `auto` edits and runs, pausing on the built-in risky-command list
- `edit` trusts commands: only a rule stops one
- `yolo` skips the rules and the sandbox entirely

No mode allows something a looser one refuses, so stepping up the ladder only
ever adds freedom.

A prompt needs a front-end that can answer. Headless runs (`-p`, `--json`) have
none, so anything that reaches `ask` is refused there; pre-approve it with an
`allow` rule.

## `agent`

Limits on one agent turn.

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `max_tool_rounds` | int | `60` | Tool rounds before the agent must answer with what it has. It says so when it hits the cap. Env `ASTER_MAX_TOOL_ROUNDS`. |
| `command_timeout_secs` | int | `300` | Seconds one `run_command` may take. Builds and test suites live here. Env `ASTER_COMMAND_TIMEOUT`. |
| `compact_budget_chars` | int | `192000` | History size above which older turns fold into a summary. Roughly 48k tokens; lower it for small-context models. Env `ASTER_COMPACT_BUDGET`. |

## `agents`

Fan-out limits for the `agent` tool's sub-agents. Every numeric value is clamped
to at least 1.

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `collector_model` | string | the session model | Cheap model for collector agents. Env `ASTER_COLLECTOR_MODEL`. |
| `max_concurrent` | int | `8` | Sub-agents running at once. Env `ASTER_AGENT_MAX_CONCURRENT`. |
| `max_per_turn` | int | `24` | `agent` tool tasks accepted in one turn. Env `ASTER_AGENT_MAX_PER_TURN`. |
| `agent_timeout_secs` | int | `300` | Seconds a single sub-agent may run. Env `ASTER_AGENT_TIMEOUT`. |

## `mcp`

MCP servers and the budget their tool inventory may spend. See
[MCP.md](./MCP.md) for how progressive injection uses these numbers.

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `servers` | map of name to server | `{}` | One entry per server. |
| `context_tokens` | int | `100000` | Context the inventory is measured against. |
| `inventory_percent` | float | `1.5` | Share of `context_tokens` the inventory may spend. Above it the prompt lists servers only and the model searches. Clamped to 0.01-100. |
| `search_limit` | int | `10` | Matches returned by one `search`. |

Each server entry:

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `command` | string | none | Executable to spawn for a local server, e.g. `npx`. |
| `args` | list of string | `[]` | Arguments to `command`. |
| `env` | map | `{}` | Extra environment, merged over the inherited one. |
| `cwd` | path | inherited | Working directory for the child. |
| `url` | string | none | Endpoint of a remote server. |
| `headers` | map | `{}` | Fixed headers sent when connecting to a remote server. |
| `type` | `stdio` \| `streamable-http` \| `sse` | inferred | Also spelled `transport`; `http` is accepted for `streamable-http`. |
| `disabled` | bool | `false` | Skip the server without deleting its config. `aster mcp enable\|disable` flips this. |

When `type` is absent it is inferred: a `url` means `streamable-http`, a
`command` means `stdio`. An entry with neither is ignored. `sse` is the
deprecated HTTP+SSE binding, kept for servers that only speak it.

Two other sources add servers after the YAML is read. `.mcp.json` in the repo
root and `~/.aster/mcp.json` are read natively, and installed plugins contribute
theirs as `<plugin>/<server>`. Both only fill names `aster.yaml` did not already
define, so the YAML always wins a collision.

## Merging

The project file layers over the global one, but not uniformly. The rules differ
because the sections mean different things.

**`review`, `agent`, `agents`** take the project's value for any key it sets.
Lists (`analyzers`, `focus_areas`, `include`, `exclude`) replace rather than
merge: an `include` of `["src/**"]` in a project means that and nothing else.

**`permissions`** unions every list, because both files are grants and dropping
the global one silently would widen or narrow access by accident. `mode` takes
the **stricter** of the two, so a project file cannot loosen a global `manual`
just by omitting the key. `use_default_rules` is true only if both files
leave it true.

**`mcp.servers`** unions by name, with the project's definition winning a
collision, so a repo can point a shared server name at its own binary.

## Settings that are environment-only

A few knobs have no `aster.yaml` key.

| Env var | What it does | Default |
| --- | --- | --- |
| `ASTER_API_KEY` | Provider key. `OPEN_ROUTER_API_KEY` is accepted too. | required |
| `ASTER_MAX_TOKENS` | Cap on generated tokens; `0`, `none`, or `off` removes the cap. | `8000` |
| `ASTER_SEED` | Fixed sampling seed; `none` or `off` disables it. | `0` |
| `ASTER_VERIFY_CONCURRENCY` | Verify passes run at once during review. | `8` |
| `ASTER_REPO` | Repository name recorded on a local review. | `local` |
| `ASTER_PRICE_PROMPT_PER_M` | Prompt price per million tokens, for cost reporting. | unset |
| `ASTER_PRICE_COMPLETION_PER_M` | Completion price per million tokens. | unset |

[`.env.example`](../.env.example) lists these with notes.

## What writes to this file

`aster init` scaffolds it. In the TUI, `/model` and switching endpoints persist
`review.model` and `review.base_url` back into whichever file is in play, the
project's when one exists and the global one otherwise. Those edits rewrite a
single line and leave the rest of the file, comments included, byte for byte.
