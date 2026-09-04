# Configuration

Aster's configuration file is `aster.yaml`. This is the complete reference for
it. Every key is optional, and a file with no keys at all behaves exactly like
no file.

[`aster.yaml.example`](../aster.yaml.example) is a commented starting point.
`aster init` writes a smaller version of it wired to the provider you pick.

## `aster config`

Everything below is settable from the command line, so the file is something you
can keep working with rather than something you have to open.

`aster config` on its own opens a form, the same one `aster init` uses. It opens
on six groups, and each lists its settings under a plain name with what they
currently resolve to:

| Group | What is in it |
| --- | --- |
| Model and provider | the model, the endpoint, reasoning effort, web search |
| Permissions | the mode and the allow/ask/deny rules |
| Agent limits | how far one turn may go |
| Sub-agents | the fan-out the `agent` tool is allowed |
| Code review | the review pipeline only, not chat |
| MCP tools | how much of the tool catalogue the model sees |

The groups are what a setting **does**, which is not the block it sits in: the
model every surface uses lives under `review` for historical reasons, so the
form puts it with the provider and leaves the review pipeline's own knobs in
their own group. Each row shows the key it is spelled by, so a setting found in
the form is a setting you can pass to `get` and `set`.

Picking one prompts for a value: a setting with a fixed set of values offers
them, everything else is typed. `-` clears a setting back to its default,
`enter` on an untouched prompt keeps what is there, and a **Save to** row
switches between the repo's config and the global one without leaving the form.
A bad value is rejected in the prompt, with the parser's own message, rather
than after the fact.

Piped or scripted, the same command prints that as a grouped table instead, and
every step of the form has a flag:

```bash
aster config list                 # every key, its value, and where that came from
aster config get review.model     # one value, and nothing else
aster config set permissions.mode auto
aster config set review.exclude "docs/**, web/**"    # lists take commas
aster config unset agent.max_tool_rounds             # back to the default
aster config path                 # which files Aster reads here
aster config edit                 # open one in $EDITOR
```

A write goes into the repo's config when it has one, else the global one.
`--global` and `--local` say which outright, and `--local` creates
`aster.yaml` in the repo root when there is none.

`aster config list --json` describes each key well enough to build a form over
it: `kind` (`text`, `bool`, `number`, `list`, `choice`) with `choices` when it is
one, `unit` for what a number counts, and `scopes` holding what each file sets on
its own, so an editor can show a value's home and write back to the scope the
user picked rather than to whichever file won. The VS Code extension's settings
tab is built on exactly this.

Nothing is saved that the next run would refuse to read: the edited file is
parsed before it is written, so a misspelled key or a value of the wrong type is
an error rather than a config you find out about on the next turn. The write
itself rewrites a single line and leaves the rest of the file, comments
included, byte for byte.

`list` and `get` report the value the next turn resolves, not the line in the
file, and name where it came from: a file, a shell variable, or the default. A
shell variable that outranks what you just wrote is said out loud rather than
left to surprise you later.

`unset` clears the key from every file that sets it, since clearing one while
the other still pins it would look like the command did nothing. `--global` and
`--local` narrow that to one file.

Two things are not settable here: `mcp.servers` and `mcp.tools` are structures
rather than single values, and `aster mcp` owns them. API keys are never written
to `aster.yaml` at all; see [Precedence](#precedence).

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

API keys are never read from `aster.yaml`. They come from the environment, which
`aster key` and `aster init` write to `.env`, or from `aster login`: GitHub
by default, ChatGPT with `aster login codex`, and OpenRouter with
`aster login openrouter`, which runs the browser sign-in flow and stores the key
as `OPEN_ROUTER_API_KEY` in `~/.aster/.env`. Starting a chat with no OpenRouter
key on a terminal offers that sign-in before failing.

`aster login zai` signs in to a Z.ai account the same way and stores the GLM
Coding Plan token as `ZAI_API_KEY`. Z.ai only registers its own redirect, so the
browser lands back on `zcode.z.ai` with a message meant for the ZCode app; paste
that address back into Aster to finish. The plan token is served by
`https://api.z.ai/api/coding/paas/v4` alone, so the sign-in offers to point Aster
there (`aster provider use zai_coding`).

`aster key` owns those `.env` files without opening them:

```bash
aster key list                                # every key Aster reads, and where it came from
aster key set FIRECRAWL_API_KEY               # asks for the value without echoing it
aster key set EXA_API_KEY exa-… --local       # this repo's .env, git-ignored
aster key set EXA_API_KEY --stdin < key.txt   # reads the value from stdin, off the process list
aster key unset FIRECRAWL_API_KEY             # clears both files
aster key path                                # which files are read here
```

`aster init`'s web step asks from the same catalog, so the two never disagree
about which providers exist.

Writes land in `~/.aster/.env` unless `--local` names the repo's. Because the
shell outranks both files, `set` says so when an export would keep winning
rather than leaving you to find out as a 401. `list` prints where each key came
from and never prints the key itself; model endpoints holding no key are hidden
until `--all`.

One endpoint needs no key at all: `https://chatgpt.com/backend-api/codex` runs
on a ChatGPT subscription. Run `aster login codex` once; Aster stores the tokens
in `~/.local/share/aster/codex.json`, refreshes them before they expire, and imports an
existing `~/.codex/auth.json` from the Codex CLI if it finds one. That endpoint
speaks OpenAI's Responses API rather than `/chat/completions`; Aster translates
both directions, so models and tools work as they do anywhere else.

A key named for the endpoint in use wins over the shared one, so switching
provider picks up a key you already export instead of demanding you move it:

1. The endpoint's own var. Every provider Aster ships with names its own, so
   `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GROQ_API_KEY`, `BASETEN_API_KEY`,
   `TOGETHER_API_KEY`, `CEREBRAS_API_KEY`, `ZAI_API_KEY` and the rest each hold
   one key. `aster provider list --json` prints the var for every endpoint.
2. `ASTER_API_KEY`

Because each endpoint has its own var, a key is entered once and survives
switching away and back. Set `OPEN_ROUTER_API_KEY` and `BASETEN_API_KEY` and
`aster provider use` moves between the two without asking for either again.

`ASTER_API_KEY` is the only var that crosses endpoints, and it exists for
self-hosted servers and anything off the catalog. A var named for one vendor is
never offered to another, so `OPEN_ROUTER_API_KEY` is used for OpenRouter and
nowhere else: sending one vendor's key to another only ever produces a bare 401.

`aster provider use` reports which of the two it would use, and says so when it
falls back to the shared key: a key issued for the last endpoint is usually
rejected by the next one.

## `review`

Despite the name, this section configures the model for everything: chat, `aster
review`, and `aster fix` all resolve their provider from here. Only the keys
marked "review only" are limited to the review pipeline.

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `model` | string | `openai/gpt-4o-mini` | Fallback for any stage without its own override. Env `ASTER_MODEL`, flag `--model`. The value `auto` picks from OpenRouter's live benchmark rankings instead (OpenRouter endpoints only); the tier comes from `ASTER_ROUTER_TIER` (`cheap` \| `balanced` \| `strong`, default `balanced`), picks are cached for a day in `~/.aster/model-rankings.json`, and `aster model router` shows what each tier resolves to. |
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
| `allow_credentials` | list of `command:dir` | `[]` | Credential directories preauthorized for one command, e.g. `gh:~/.config/gh`. The sandbox denies `~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.config/gh`, and `~/.kube` by default and prompts when a command needs one; an entry here skips the prompt for that pair only. `~/Library/Keychains` is never grantable. The two config files union their lists. |

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
to at least 1. See [SWARM.md](./SWARM.md) for the roster, the run model, and
custom agent definitions.

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `collector_model` | string | the session model | Cheap model for collector agents. Env `ASTER_COLLECTOR_MODEL`. |
| `max_concurrent` | int | `8` | Sub-agents running at once. Env `ASTER_AGENT_MAX_CONCURRENT`. |
| `max_per_turn` | int | `24` | `agent` tool tasks accepted in one turn. Env `ASTER_AGENT_MAX_PER_TURN`. |
| `agent_timeout_secs` | int | `300` | Seconds a single sub-agent may run. Env `ASTER_AGENT_TIMEOUT`. |

## `ui`

What chat prints on its own.

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `welcome` | bool | `true` | Print the session header (model, provider, skills) when chat starts. `/welcome` in a chat toggles it and saves the choice. |

## `mcp`

MCP servers and the budget their tool inventory may spend. See
[MCP.md](./MCP.md) for how progressive injection uses these numbers.

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `servers` | map of name to server | `{}` | One entry per server. |
| `tools` | `allow`/`deny` glob lists | `{}` | Which tools reach the model, by `server/tool` id. See below. |
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

### `mcp.tools`

`disabled` turns off a whole server. `mcp.tools` turns off one tool, matching
globs against the `server/tool` id.

```yaml
mcp:
  tools:
    allow: []          # empty means every tool
    deny:
      - "web/crawl"
      - "browser/*"
```

`deny` wins over `allow`, so denying a tool an `allow` also names still turns it
off. The filter runs after every server has listed, so it covers the in-process
`web` server as well as third-party ones. A bad glob is reported and that one
rule is dropped, rather than costing the session its catalogue.

`aster mcp disable web/crawl` writes the id into `deny`, and `aster mcp enable
web/crawl` removes it. Without a `/` those commands still flip a server's
`disabled` line, as before. `aster mcp list` reports what the filter held back.

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
**`mcp.tools`** unions both lists, like `permissions`: a global `deny` is a
decision, and a project file omitting the key must not undo it.

## Settings that are environment-only

A few knobs have no `aster.yaml` key.

| Env var | What it does | Default |
| --- | --- | --- |
| `ASTER_API_KEY` | Provider key, used for any endpoint without a var of its own. A var named for the endpoint beats it; see [Precedence](#precedence). | required |
| `ASTER_MAX_TOKENS` | Cap on generated tokens; `0`, `none`, or `off` removes the cap. | `8000` |
| `ASTER_SEED` | Fixed sampling seed; `none` or `off` disables it. | `0` |
| `ASTER_VERIFY_CONCURRENCY` | Verify passes run at once during review. | `8` |
| `ASTER_REPO` | Repository name recorded on a local review. | `local` |
| `ASTER_PRICE_PROMPT_PER_M` | Prompt price per million tokens, for cost reporting. | unset |
| `ASTER_PRICE_COMPLETION_PER_M` | Completion price per million tokens. | unset |
| `ASTER_NO_BROWSER` | Set it and `open_preview` never launches a browser. The agent reports the URL instead, which is what you want over SSH or in a container. | unset |
| `ASTER_TIMEOUT_SECS` | Silence tolerance for the model client: how long a request may go without delivering any data before it is dropped. A reply that keeps streaming is never cut off by this. | `300` |
| `ASTER_MAX_RETRIES` | Retries on transient model errors. | `3` |
| `ASTER_DEADLINE_SECS` | Wall-clock cap across those retries. | `180` |
| `ASTER_EDITOR` | Editor for `aster config edit`, before `$VISUAL` and `$EDITOR`. | unset |
| `ASTER_UI_DIR` | Serve `aster serve`'s page from this directory instead of the embedded build, for working on the UI itself. | unset |
| `ASTER_MCP_EXTRA` | JSON of extra MCP servers a front-end injects for one session. | unset |
| `ASTER_NO_UPDATE_CHECK` | Set it to skip the update check. | unset |
| `ASTER_TELEGRAM_TOKEN` | Bot token for `aster remote`. | unset |
| `ASTER_REMOTE_USERS` | Telegram user ids allowed to drive `aster remote`. | unset |

[`.env.example`](../.env.example) lists these with notes.

## What writes to this file

`aster init` scaffolds it. In the TUI, `/model` and switching endpoints persist
`review.model` and `review.base_url` into the **global** file, because the model
you want to work with follows you between directories rather than belonging to
whichever repo you were standing in. If the project file pins one of those two
keys, it is updated as well: it outranks the global one, so leaving it alone
would make the switch look like it did nothing in that repo. Those edits rewrite
a single line and leave the rest of the file, comments included, byte for byte.

Re-running `aster init` in a repo that already has a config edits those same two
keys in place instead of leaving the file untouched, so it is the way to switch
provider as well as the way to start. `--force` rewrites the whole scaffold;
`-y` still never overwrites a config, since it picks defaults you were not
shown.

From outside the TUI, the same two keys are written by:

| Command | Writes |
| --- | --- |
| `aster provider use <id\|name\|url> [--model ID]` | `base_url` and `model` together, adopting the catalog's example model when `--model` is left off |
| `aster model use <ID>` | `model` |

Both then print what the next turn actually resolves to, which is not always
what was just saved: `ASTER_MODEL` and `ASTER_BASE_URL` outrank the file, so
either one being set in your shell is reported rather than left to surprise you
later. `--json` returns the same as a `{"model", "provider", "base_url",
"config", "also", "key_env", "has_key", "shadowed_by_env"}` object, where
`also` names the project file that was moved along, or `null`.

`aster config set` writes the same two keys, but into the file it was pointed
at rather than following the model between directories, since it is the command
for editing one config rather than for switching what you work with.

Reading the other way, `aster status --json` reports the resolved provider and
model without changing anything, `aster provider list` shows the catalog with
the endpoint in use marked, and `aster model list` asks that endpoint what it
serves. `aster model recommended` answers from the catalog instead, so a picker
has something to show before the endpoint has been asked. Every surface reads
these rather than keeping a list of its own.
