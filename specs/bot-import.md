# Aster Bot Import

Status: v0 implemented, stages 1 to 6
Target: `aster-cli` (`bots/`), `aster-agents`, `aster-skills`, `aster-cron`
Date: 2026-09-13

## Thesis

Grok Bot has a marketplace of specialist agents, and every one of them is a
prompt, a handful of skills, a few routines, and a list of connectors it
assumes exist. Aster already runs all four of those things. What is missing is
the adapter, and the honesty about what did not come across.

The failure mode to design against is not a parse error. It is a bot that
imports cleanly, reports success, and then does nothing useful because its
instructions assume a Grok CLI, an authenticated model, and a connector that
this machine has never heard of. An import is only finished when Aster has
said, in writing, which of the bot's assumptions hold here and which do not.

Second failure mode: a marketplace bot's "memories" are the publisher's
preferences and their machine's layout. They are not facts about the user.
Anything that writes them into `ASTER.md` has corrupted the user's memory with
a stranger's notes.

## What is actually out there

Checked 2026-09-13 against `RongleCat/awesome-grok-bot` (294 stars) and the
GitHub API. Two of the four leads resolve under the names given; the other two
resolve under different owners.

| Lead | Status | What it is |
|---|---|---|
| `colesmcintosh/botmigrate` | verified, 1 star | Python CLI, share JSON to Hermes profiles, with its own IR and a JSON schema |
| `jmporchet/grok-to-hermes` | verified via the list | TypeScript CLI, export data to Hermes distributions and `tar.gz` |
| `jaskirat1616/grok-skills` | verified via the list | 195 `SKILL.md` playbooks, installable through the Grok plugin marketplace |
| `adam91holt/grokbot-sdk` | verified, 12 stars | TypeScript gateway client plus disk readers for a running Bot host |

`botmigrate` is the useful one, because it ships fixtures. Its example share
JSON is the schema, in full:

```json
{
  "profile": { "name": "...", "description": "...", "title": "...",
               "avatarShape": "blob", "avatarColor": "cyan" },
  "memory":   [ { "kind": "profile" | "log", "createdAt": "...", "content": "..." } ],
  "skills":   [ { "name": "...", "description": "...", "content": "..." } ],
  "routines": [ { "slug": "...", "name": "...", "description": "...", "content": "..." } ],
  "plugins":  [ { "pluginId": "github", "name": "...", "description": "..." } ],
  "gettingStarted": { "skill": "..." },
  "visibility": "public"
}
```

Four things that schema does not tell you, all of which `botmigrate` handles
and this spec must too:

1. **A routine carries no schedule field.** The cron expression is prose inside
   `description` and `content`, recovered by regex (`extract_cron`).
2. **Not every routine is a cron.** Grok event triggers (`slack`, `github`,
   `webhook`, `linear`, `sentry`, `pagerduty`, and others) are listeners.
   `botmigrate` refuses to convert them into fake cron and records them in a
   sidecar instead. Aster copies that refusal.
3. **`plugins` are references, not configuration.** A `pluginId` and a display
   name. There is no command, no URL, no transport. Every connector is an
   unresolved dependency by construction.
4. **There is an on-disk form too**, for a Bot host you control: `profile.json`,
   `memory/`, `automations/<slug>/automation.json`.

Two reported constraints, both second-hand through the list's own summaries and
worth confirming against a real export before code depends on them:

- The official template export is reported to ship `skills: []` even when the
  preview lists skills. If that holds, the marketplace page is the only place a
  skill body exists, which inverts the usual "prefer the structured export"
  rule for that one field.
- There is no one-click memory export. Bot memory is plain text on the Bot's
  computer.

The official templates guide states the boundary directly: templates package
instructions, memories, skills, and plugin references. Custom scripts and
nonstandard MCP implementations do not travel and need separate setup.

## What exists already (do not rebuild)

- `aster-cli::import`: `run_mcp_import` and `run_sessions_import`, with a
  `Source` enum for Claude, Codex, Cursor, opencode, and Hermes. A bot importer
  is a third entry point in this file, not a new crate.
- `aster-agents`: an agent is a directory with `AGENT.md`, YAML frontmatter
  (`name`, `description`, `category`, `model`, `tools`, `max_rounds`, `verify`)
  then a markdown body that becomes the system prompt. Roots are
  `<repo>/.aster/agents` then `<home>/agents`, project shadowing global
  ([agents.rs:21](../crates/aster-cli/src/agents.rs#L21)).
- **Sub-agents are already isolated.** A dispatched agent gets
  `SkillSet::default()`, `Instructions::default()`, `store: None`,
  `recorder: None`, and `mcp: None`
  ([agents.rs:240](../crates/aster-cli/src/agents.rs#L240)). It cannot see the
  conversation, the user's memory, the project instructions, or the configured
  MCP servers. The isolation an imported bot needs is the default, not a
  feature to add.
- `aster-skills`: `SKILL.md` directories, frontmatter then body, discovered from
  `<repo>/.aster/skills` and `<home>/skills`.
- `aster-cron`: `Schedule { name, cron, agent, task, notify, notify_url }` under
  `schedules:` in `aster.yaml`, validated then installed into launchd or cron by
  an explicit `aster cron install`. Nothing runs until the user installs it.
- `aster-web`: `WebBackend::extract(url) -> ExtractedPage` returns Markdown, and
  `is_api_backed()` says whether a real extractor is configured.
- `aster-cli::skills`: GitHub search, sparse clone, tarball fetch, manifest-first
  listing, and a multi-select picker. The marketplace-page and skill-repo front
  ends reuse this, they do not reimplement fetching.
- `aster-plugins`: Agent Plugins v1.0.0. Relevant as the target for the
  *dependency* side (a connector that resolves to a real plugin), not as the
  container for an imported bot. A plugin contributes skills and MCP servers,
  never an agent, and Aster deliberately claims no extension namespace in
  `plugin.json`.

## Naming: a bot is a package, an agent is the primitive

Aster already has agents: `AGENT.md`, `AgentRegistry`, the `agent` tool, and
`schedules: { agent: ... }`. An imported bot runs on exactly that machinery, so
the temptation is to call it an agent and stop. The temptation to resist harder
is calling it a sub-agent. "Sub" names a dispatch relationship, the thing being
run inside a parent turn. Nobody installs a sub. It is the right word for the
mechanism and the wrong word for the noun.

The distinction that earns a second word is packaging, and this codebase already
draws it once:

> **bot : agent :: plugin : skill.**

A skill is a primitive you write. A plugin is a package you install that
contributes skills, carries provenance, and can be updated or removed as a unit.
`~/.aster/agents/scout/AGENT.md` is a prompt you wrote. A bot is a package
someone else published that contributes an agent, its skills, its routines, and
a list of things it assumes exist.

So:

- **bot**: the installed package. What `aster bots add` manages, what has an
  origin URL, a requirements list, and an update diff.
- **agent**: the runtime primitive a bot contributes, dispatched exactly like
  any built-in. `schedules: { agent: research-bot }` stays correct and needs no
  new key, the same way you reference a plugin's skill by the skill's name.
- **sub-agent**: how that agent executes, an implementation detail that appears
  in code and in `docs/`, not in the product surface.

The payoff is that `aster bots` reads like `aster plugins` and `aster skills`,
which is the shelf it belongs on. It also means `aster agents` stays free for
the management surface agents still lack, and the two never collide.

## Architecture

One IR, several front ends, one writer, one gate.

```
share JSON  ─┐
on-disk dir ─┼─> Reader ──> BotIR ──> Writer ──> agent package ──> Capability check
page extract ┤                                        │                    │
skill repo  ─┘                                        v                    v
                                              inert schedules        import report
                                                                           │
                                                          (optional) ──> smoke run
```

### 1. Readers

Order of implementation, which is also order of trust:

1. **Share JSON** (`aster bots add ./research-bot.json`). Fully specified, with
   fixtures available from `botmigrate`. Build this first.
2. **On-disk Bot directory.** Same IR, different layout. Cheap once 1 exists.
3. **Marketplace URL** (`aster bots add https://x.ai/bot/marketplace/bots/<slug>`).
   Extraction through `aster-web`. Lowest trust: it is a rendered page, and the
   pages are likely JS-driven, so `is_api_backed()` gating matters. If
   extraction yields a description where an instruction body belongs, the
   reader **fails that field** rather than promoting a summary into a skill.
4. **Standalone skill repos** (`jaskirat1616/grok-skills` and friends). These
   are already `SKILL.md`. They skip most of the pipeline and go straight to the
   capability check.

A reader's contract: produce a `BotIR`, or produce a list of fields it could not
recover. Never invent an instruction body.

### 2. `BotIR`

The normalized form, deliberately close to the share JSON so the mapping stays
auditable:

| IR field | From | To |
|---|---|---|
| `identity` (name, title, description) | `profile` | `AGENT.md` frontmatter |
| `instructions` | `profile.description` plus body text | `AGENT.md` body |
| `skills[]` (name, description, body) | `skills[]` | scoped `SKILL.md` dirs |
| `routines[]` (name, task, trigger) | `routines[]` plus `extract_cron` | `schedules:` entries, inert |
| `requirements[]` | `plugins[]`, plus anything the prose demands | dependency manifest |
| `notes[]` | `memory[]` | quarantined, never user memory |
| `origin` (url, fetched_at, raw) | the source itself | import snapshot |

`trigger` is an enum, not a string: `Cron(String)`, `Event { kind, detail }`, or
`Unknown`. Only `Cron` produces a schedule. `Event` is recorded and reported as
unsupported, because Aster has no event-listener substrate and a fabricated
`0 9 * * 1` would be a lie about when the bot runs.

### 3. Writer

Output is one bot package under `<data>/aster/bots/<name>/`, or
`.aster/bots/<name>/` with `-p`, mirroring where plugins install:

```text
<name>/
├── AGENT.md          # identity + instructions, tools narrowed to what it needs
├── skills/           # the bot's own skills, scoped to this agent
├── bot.json          # origin, fetched_at, per-field provenance, requirements
└── source/           # the untouched original (share JSON, or extracted markdown)
```

Keeping bots out of the agents roots is not bookkeeping. It is what makes
`remove` safe, `update` meaningful, and "which of these did I write" answerable.
Discovery gains a third root: hand-written agents still shadow a bot's agent on
a name collision, so installing a bot can never silently replace something you
wrote. `AgentRegistry::discover` takes the bots roots after the agents roots and
before the built-ins.

`skills/` is the one piece of new machinery. Today a sub-agent gets an empty
`SkillSet`, and the session's skills roots are global. An imported bot needs its
own skills without those skills appearing in the user's index or in any other
agent's. So: `AgentDef` carries an optional `skills_root`, set only for agents
found under a bots root, and the sub-agent spawn path builds its `SkillSet` from
that root instead of from `SkillSet::default()`. Same change unlocks scoped MCP
servers later; do not build that part until a requirement actually resolves to
one.

One thing this spec got wrong, found in the build: setting `ctx.skills` is not
enough. `system_prompt` deliberately skips the skill index for sub-agents, so a
bot's scoped skills were installed and invisible. The index now rides along in
the sub-agent branch, which is safe precisely because the set is empty for every
agent that is not a bot.

Routines do **not** write themselves into `aster.yaml`. `Settings` is
deserialize-only behind a line-based YAML editor, and contributing routines
implicitly would mean a later `aster cron install`, run for the user's own
schedules, quietly installing a stranger's too. Instead `aster bots show` prints
a paste-ready `schedules:` block and says that nothing runs until `aster cron
install`. `botmigrate` reaches the same conclusion from the other side: its
converted crons are written `enabled: false`.

### 4. Capability check

The part that makes the import honest. Every entry in `requirements[]` resolves
to exactly one of:

- **available**: the connector maps to a configured MCP server, a built-in tool,
  or a binary on `PATH`.
- **needs setup**: it maps to something Aster can install or authenticate, and
  the report says which command does it.
- **needs adaptation**: the requirement is real but the named implementation is
  not available here. A bot that requires the Grok CLI for its model calls is
  the canonical case. Substituting Aster's provider is a legitimate adaptation
  and must be disclosed as one.
- **unsupported**: nothing here provides it. Event triggers land here. So does a
  desktop-only command on a phone, unless a connected runner covers it.

Classification runs against the current machine, not against a static table.

### 5. Report

Printed on install, and re-printable with `aster bots show <name>`:

```console
$ aster bots add ./research-bot.json
added research-bot (grok share json)

  instructions   ok
  skills         1 of 1: arxiv-brief
  routines       1 cron (weekly-digest), inert until `aster cron install`
  requirements   1 needs setup: github

nothing scheduled, nothing authenticated.
  aster bots show research-bot      what is still missing
  aster run research-bot "..."      try it
```

The counts are the point. "1 of 1" and "0 of 3" are different imports, and the
user should not have to open a directory to tell which one they got.

### 6. Smoke run (optional, `--verify`)

One bounded task against the imported agent, checking a property the bot's own
description claims. A research bot should fetch a source and cite it. This is a
real model call, so it stays opt-in.

## Isolation and trust

An imported bot is third-party content that becomes a system prompt. Treat it
that way.

- **Memory never merges.** `memory[]` lands in `bot.json` as the publisher's
  notes. Nothing in the pipeline may write to `ASTER.md` or the memory store.
- **Instructions are scoped to the agent.** They are that agent's system prompt
  and nothing else. They do not touch the session prompt, and they cannot,
  because a sub-agent's `Instructions` are already default.
- **Tools are narrowed, not inherited.** An imported `AGENT.md` gets
  `DEFAULT_TOOLS` unless its requirements justify more. An imported bot asking
  for `edit_file` is a decision the user makes, not one the import makes.
- **The snapshot is evidence.** `source/` is never edited. It is what makes a
  later diff meaningful and what lets a suspicious import be read after the
  fact.

## Faithful and adapted

Two modes, named in the report, because silently doing the second while
claiming the first is the dishonest version of this feature.

- `--faithful` (default): preserve every stated requirement, classify, report
  what is missing. Substitute nothing.
- `--adapt`: rewrite the workflow against what this machine has, and print a
  line per substitution. Replacing a required Grok CLI call with Aster's own
  provider is an adaptation and appears in that list.

## Updates

`bot.json` holds `origin` and a content hash per field. `aster bots update
<name>` re-reads the source and shows a three-way view: what the publisher
changed, what the user changed locally, and where those collide. It never
overwrites a locally edited field without saying so. This is why `source/` is
kept verbatim.

## Asterdroid

Capability checking is per-device, and the phone is a different device. Its
roster is Android controls, the connectors configured there, local commands, and
any desktop runner it can reach. A desktop-only command is **unsupported** on
the phone unless such a runner exists.

Resist the temptation to classify every missing integration as "drive it by
tapping the screen". Screen automation is a fallback for things with no API, not
a universal shim for a missing connector.

The Telegram surface gets the same entry point: "import this bot: <url>" runs
the same pipeline and returns the same report.

## CLI surface

```bash
aster bots add ./research-bot.json                    # share JSON
aster bots add ~/grok/bots/research-bot               # on-disk bot directory
aster bots add https://x.ai/bot/marketplace/bots/...  # marketplace page
aster bots add jaskirat1616/grok-skills               # skills only
aster bots add ./research-bot.json -p                 # this project only
aster bots list                                       # installed, and what each contributes
aster bots show <name>                                # requirements still unmet
aster bots update <name>                              # re-read the origin, show the diff
aster bots remove <name>
```

`add` dispatches on what it is handed, the way `aster plugins add` and `aster
skills add` already accept a path, a repo, or a URL. `aster bots list` prints
what a bot contributes (agent, skills, routines) for the same reason `aster
plugins list` does.

`import` stays a verb under its existing nouns (`aster mcp import`, `aster
sessions import`). It is not promoted to a top-level command, because the thing
being named here is the package, not the act of reading one.

## Open questions

1. Does a real marketplace export actually ship `skills: []`? Every decision
   about trusting the page over the export depends on it. Get one export.
2. Are the marketplace pages server-rendered enough for `WebBackend::extract`,
   or is a headless browser required? If the latter, the URL front end is
   significantly more expensive than the JSON one and should stay stage 3.
3. Is there a public, supported export endpoint, or is "Add to Grok Bot" the
   only sanctioned path? A supported endpoint would retire the page reader.
4. Do `botmigrate` and `grok-to-hermes` have compatible licenses for borrowing
   the IR shape, and is it worth borrowing versus reading it and writing our own
   in Rust? The schema is small. Probably the latter.

## Build order

1. **Done.** `BotIr` plus the share JSON reader, against `botmigrate`'s fixture.
2. **Done.** Writer: the bots root, `AGENT.md`, `bot.json`, `source/`.
   `AgentRegistry::discover_all` takes the third root and its shadowing order.
3. **Done.** Per-agent skills root, the sub-agent spawn path that reads it, and
   the skill index in the sub-agent prompt.
4. **Done.** Cron routines recovered from prose, event triggers refused, both
   surfaced by `show` as a paste-ready block.
5. **Done.** Capability check and the report.
6. **Done.** `aster bots add` / `list` / `show` / `remove`.
7. On-disk reader, then the marketplace page reader, then skill repos.
8. `update` and the three-way diff.

Stages 1 through 6 are the MVP: a share JSON becomes a working, isolated,
honestly-reported bot you can list and remove. Everything after that is another
front end onto the same IR. `aster bots add <url>` currently fails with the
reason and points at the file form, rather than half-reading a page.
