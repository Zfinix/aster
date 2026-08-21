# Changelog

All notable changes to Aster are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **Aster's editor commands are findable again in the command palette.** Ten of
  them set the `Aster` prefix as a `category` rather than baking it into the
  title, and VS Code and Cursor drop the category when they render the palette:
  `Aster: Open` showed up as a bare `Open`, indistinguishable from every other
  extension's. The prefix now lives in the title, matching the review commands
  that always had it.

- **The panel's keyboard tips read the way a Mac keyboard is labelled.** The
  empty state spelled its chords `cmd alt k`, and `alt` is the one modifier a
  Mac keycap does not name: that key says `option`, and the convention is the
  glyph. The tips now draw `⌘ ⌥ ⇧` on macOS and keep `ctrl alt shift` elsewhere,
  which is the branch the panel already made for `cmd` against `ctrl` and never
  extended to the other two.

## [0.4.0] - 2026-08-21

### Added

- **Aster in your browser.** `aster serve` opens the agent at
  `http://localhost:4187/`: the editor extension's panel, served as a page.
  Streaming chat, approvals, questions, `@` mentions, slash commands, saved
  sessions, review, compaction, and the model, provider, and MCP pickers all
  work, because the page speaks the same protocol the extension's webview does
  and every turn runs as an `aster` child in the repo the server was started
  from. The UI ships inside the released binary, so `curl … | sh` installs it
  with everything else; the port is guarded so no other page in the browser can
  drive the agent, and `--host` past loopback requires the token the banner
  prints. `--port` moves it, `--no-open` leaves the browser alone, and
  `ASTER_UI_DIR` serves a UI build from disk while working on the page itself.

- **Work you can look at, in your browser.** A turn that builds a page used to
  end with a description of it. The agent now has `open_preview`: it points at
  a running dev server or a file it built, and the page opens in your browser.
  Loopback URLs and files in the repo open on their own; anything else asks
  first, so a link off the machine is still your call. A port nothing is
  listening on is refused rather than shown to you as a connection error, a
  directory opens its `index.html`, and a page already opened this session
  points back at the tab instead of stacking a second one. `ASTER_NO_BROWSER`
  turns the launch off and leaves the URL in the reply, for SSH and containers.

- **Sessions get their names back.** Naming ran as a background task the turn
  never waited on, which works in the TUI and nowhere else: `--stream` and
  one-shot runs are a process per turn, so the runtime was dropped and the
  naming request killed before it left the machine. Every session started from
  VS Code, the desktop app, or `aster -p` went unnamed, however many turns it
  ran. The turn now waits for its own name, bounded at ten seconds so a slow
  endpoint cannot hold a finished turn behind it, and only on the one turn per
  session that earns the name. A miss leaves the session unnamed and the next
  turn tries again.

- **A session gets its name from the first prompt.** Naming waited for two user
  turns, so a session that opened with the whole task still sat in the picker
  under its raw opening line while you worked in it. An opening message that
  already states the task now names the session at the end of that first turn.
  Only a thin opener still waits: a bare greeting, a prod like "continue", or
  something too short to carry a topic. Messages in languages that do not space
  their words are measured by length rather than word count, so they name on the
  first turn too.

- **Links in a reply are clickable everywhere.** A bare URL in the VS Code
  panel is now a link, not text you retype, and clicking one in either the
  panel or the desktop app hands it to your browser. The desktop app used to
  navigate itself away from the app when you clicked a link, with no way back.

- **Every provider keeps its own API key.** Only seven endpoints named a key
  variable of their own; the other thirty-one shared `ASTER_API_KEY`, so setting
  up a second provider overwrote the first and switching back meant pasting the
  key again. The var each endpoint reads now comes from the shipped catalog
  alongside its base URL and models, and every provider that takes a key names
  one: `BASETEN_API_KEY`, `TOGETHER_API_KEY`, `CEREBRAS_API_KEY`, and so on for
  all of them. Set two and `aster provider use` moves between them without
  asking for either again. `ASTER_API_KEY` still works and is still the fallback
  for self-hosted servers and endpoints off the catalog. The desktop shell reads
  the same catalog, so a key stored on one surface is found by the other.

- **The agent stops gathering instead of wandering.** The loop's runaway
  guards all watched for repetition, which a model that varies its query never
  trips: a turn could spend forty rounds searching without editing a thing and
  every guard would call it progress. Rounds where nothing but a lookup ran are
  now counted too. Ten in a row and the loop says so; ten more and the turn ends
  by answering with what it found, which is not a failed turn and no longer
  reads as one. Any edit or command resets the streak, so a long investigation
  before a change is untouched. The model is also told its round budget once, at
  the halfway mark, since it could not see the counter it was running against.
- **Reopening a session brings back the whole thread.** A turn's reasoning is
  recorded in the transcript rather than streamed and forgotten, and both the
  VS Code panel and the desktop app rebuild a loaded session's blocks in the
  order they happened: the thinking, the reply, then the steps it ran. The
  desktop sidebar now fills itself from sessions on disk at startup, where it
  used to open empty every launch. Sessions saved before this reload without
  reasoning, because it was never written down.

- **The config file is editable from the command line.** `aster config` reads
  and writes `aster.yaml`, so a setting no longer means finding the file and
  remembering the key's spelling. On its own it opens a form, the same one
  `aster init` uses. Settings are grouped by what they do rather than by the
  block they sit in, since that is the part a key name gives away least: the
  model every surface uses sits under `review` for historical reasons, and the
  form files it with the provider instead. Each carries a plain name, the value
  it currently resolves to, and the key it is spelled by, so a setting found in
  the form is one you can pass to `get` and `set`. Numbers read as quantities:
  a timeout is `300s`, a compaction budget `192k chars`, an empty `include`
  says "everything". Picking one prompts for a value, `-` clears it back to the
  default, and a **Save to** row switches between the repo's config and the
  global one without leaving the form. Piped or scripted it prints that as a table instead, and `list` asks for
  it outright. `get` prints one value and nothing else, so it pipes. `set` and `unset` write and clear one key, `path` says
  which files Aster reads here, and `edit` opens one in `$EDITOR` and tells you
  whether what you saved still parses. Writes land in the repo's config when it
  has one and the global one otherwise, with `--global` and `--local` to say
  outright. Nothing is saved that the next run would refuse to read: the edited
  file is parsed first, so a misspelled key or a value of the wrong type is an
  error naming the keys that do exist, rather than a config that fails on the
  next turn. Comments and layout survive an edit, `unset` clears the key from
  every file that pins it, and a shell variable that outranks what was just
  written is said out loud. `mcp.servers` and `mcp.tools` stay with `aster mcp`,
  which is where their structure belongs.

- **One place to switch provider and model.** `aster provider use <id>` points
  Aster at an endpoint and adopts a model it serves in the same write, and
  `aster model use <id>` changes the model alone. Both save to `aster.yaml`,
  which every surface already resolves from, so a switch made in one is the
  switch everywhere rather than a preference belonging to whichever app made it.
  Both then report what the next turn actually resolves to, and say so out loud
  when `ASTER_MODEL` or `ASTER_BASE_URL` in your shell outranks what was just
  saved. `aster provider list` shows the catalog with the endpoint in use
  marked, `aster model list` asks that endpoint what it serves, and `aster model
  recommended` answers from the catalog, so a picker no longer has to ship a
  hardcoded list that goes wrong the moment the endpoint changes. `aster models`
  keeps working as the older spelling of `aster model list`, and listing the
  provider catalog no longer needs a key, since the catalog ships in the binary.
  `aster init` offers the same shortlist when picking a model, and points at
  `aster provider use` when it finds a config it will not overwrite.

- **Web search works with no API key and no server.** `web/search` falls
  through to DuckDuckGo when nothing else is configured, so a fresh install can
  search the web and read a page without signing up for anything. A configured
  provider still takes over: `EXA_API_KEY` is new and leads search, joining
  Context.dev, Firecrawl, Browserbase, and Cloudflare Browser Rendering. The
  provider serving a tool is named in the description the model reads, and the
  keys are documented in [docs/MCP.md](docs/MCP.md#web-tools) and
  `.env.example`, where they were not written down at all before.
- **Any single tool can be turned off.** `mcp.tools` filters the catalogue by
  `server/tool` id, the same id the policy engine authorizes against. Globs
  work, `deny` beats `allow`, and an empty `allow` means everything. The filter
  runs after every server has listed, so it covers the in-process `web` server
  and third-party ones alike. `aster mcp disable web/crawl` writes the rule for
  you and `aster mcp enable web/crawl` takes it back out; without a `/` both
  still flip a whole server, as before. `aster mcp list` reports what the filter
  held back, so a missing tool is never a mystery. Global and project configs
  union their lists, so a project file cannot silently undo a global `deny`.
- **A real browser, off by default.** `aster init` scaffolds a `browser` server
  running [browser-use](https://github.com/browser-use/browser-use) over stdio;
  `aster mcp enable browser` turns it on. The agent can then navigate, click,
  type, scroll, read page state, and screenshot. It needs `uv` and Python 3.11+
  plus a one-time `uvx browser-use install`; without them the server is reported
  and skipped like any other, and the session is unaffected. The scaffolded
  entry disables browser-use's vendor telemetry and runs headless, and its two
  LLM-dependent tools are denied by default, since both want a second API key
  and one of them runs a whole agent loop inside a single tool call.
- **A tool can return an image.** An MCP `image` content part now reaches the
  model as an image instead of `[image content omitted]`, which is what makes a
  browser screenshot worth taking. The bytes go through the same encoder as
  `@path` mentions, so the 2048px and 10MB caps apply to a full-page screenshot
  too, and a model without image input still gets the placeholder it always did.

- **The first turn already knows what the repository is.** One walk at session
  start builds a short profile — the project's name, what it says it does, how
  many files it has in each language and where those packages live, the
  top-level layout, and the pages in its docs directory — and that profile
  leads the note the model starts every turn with. Before this the model knew
  the platform, the date, and the git state but nothing about the project
  itself unless the repo happened to keep an `AGENTS.md`, so an opening
  question got an answer written for no codebase in particular. On Aster's own
  repo the profile is six lines and 24ms. The description comes from the
  README's opening prose, skipping headings, badges, and HTML; a repo with no
  README, or one whose README opens with a logo, falls back to the
  `description` in `Cargo.toml`, `package.json`, `pyproject.toml`,
  `composer.json`, or `pubspec.yaml`. A workspace collapses to the directory
  holding most of it, dependency and build directories are left out, a language
  with a handful of stray files is not reported as a stack, and the walk stops
  at 20,000 entries so a huge monorepo cannot hold the UI up.
- `aster status` reports what the next turn would run with: model, provider,
  effort, permission mode, the turn limits, and how many MCP servers, skills,
  memory blocks, and sessions are wired in. `--json` for front-ends.
- `aster mcp list --no-connect` lists the configured servers and their on/off
  state without spawning any of them, so a UI can draw a control panel without
  paying for a connect.
- `aster models --providers` lists the endpoint catalog, marking the one this
  repo points at and naming the env vars that may hold each one's key.
- `aster chat --compact` folds a `--messages-json` history into a summary and
  prints the shorter history back, so a front-end that owns its own transcript
  can compact it the way the TUI's `/compact` does.

- Sandbox credential access now asks instead of failing. A command that needs a
  credential directory the sandbox denies (`gh` and `~/.config/gh`, `aws` and
  `~/.aws`, `kubectl` and `~/.kube`, `ssh`/`git` and `~/.ssh`, `gpg` and
  `~/.gnupg`) prompts for approval, with yes / always / no. An approval covers
  one command and one directory, so approving `gh` never lets the next `cat`
  read the token, and it is stored apart from the file-read grants so it cannot
  widen `read_file`. `~/Library/Keychains` stays denied and is not grantable.
  Preauthorize headless runs with `permissions.allow_credentials`
  (`["gh:~/.config/gh"]`).

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
- [docs/CONFIG.md](docs/CONFIG.md), a full `aster.yaml` reference: every key
  with its type, default, and environment equivalent, the built-in
  protected/secret/exclude lists, and the per-section rules for how the project
  file merges over the global one. `aster.yaml.example` gains the keys it was
  missing (`agents`, `review.astgrep_rules`, the `mcp` inventory budget, and
  per-server `env`).

### Changed

- **`ASTER_API_KEY` is the only key that crosses endpoints.** `OPEN_ROUTER_API_KEY`
  used to stand in as a general fallback, a leftover from when OpenRouter was the
  only backend, so pointing Aster at another provider with no key of its own sent
  an OpenRouter key to it and got back a bare 401 that read as "check your API
  key" when the key was fine and simply belonged elsewhere. It is now what its
  name says: OpenRouter's var, used for OpenRouter. Endpoints without a var of
  their own fall back to `ASTER_API_KEY` and nothing else, and a missing key is
  named before the request instead of after it. Nothing changes for OpenRouter
  users, who still resolve through `OPEN_ROUTER_API_KEY`. Resolution also moved
  into `aster_ai::keys`, so the CLI, the Telegram bridge, the eval harness, and
  `AiClient::from_env` all read a key the same way; three of them used to ignore
  the endpoint entirely and take whichever of the two vars was set.
- **A key named for the endpoint now wins over the shared one.** Point Aster at
  Anthropic and `ANTHROPIC_API_KEY` is used; the same holds for `OPENAI_API_KEY`,
  `GROQ_API_KEY`, `MISTRAL_API_KEY`, `DEEPSEEK_API_KEY`, `GEMINI_API_KEY`, and
  `OPENROUTER_API_KEY`. `ASTER_API_KEY` is still the fallback and still works on
  its own. The TUI and the editor panel already resolved keys this way; the
  command line did not, so a saved provider switch could fail on the next turn
  with a key already exported and ignored. `aster provider use` names which key
  it would use, and says so when it falls back to the shared one, since a key
  issued for the last endpoint is usually rejected by the next.
- **The VS Code panel gets a command menu**, holding everything it can do:
  conversation actions, model, provider, effort, mode, the review commands,
  status, diff, memory, MCP servers, and every skill the session can see. Open
  it from the `/` button, by pressing `/` in an empty composer, with
  `cmd+alt+k`, or from the editor palette as **Aster: Show Command Menu**. Rows
  show the setting's current value; effort is set inline on its row; the rest
  either run, open a picker, or answer with a card in the thread. Commands are
  no longer typed at the composer as `/name`, which was a terminal habit in a
  window that can just show the list.
- The VS Code panel lists skills again. It parsed `aster skills list --json` as
  a flat array when the command emits scopes and plugins, so the menu had
  silently shown only its built-ins, and plugin-contributed skills never
  appeared at all.
- The VS Code panel renders `explore`, `run_tests`, `update_plan`, `ask_user`,
  and `exit_plan_mode` steps by name and icon instead of printing the raw tool
  id.
- The VS Code approval prompt takes numbered answers: `1` allows, `2` allows
  and remembers when the ask carries a scope to remember it against, `3` and
  `Esc` reject. A box under them rejects and tells the agent what to do
  instead, in one step.
- The VS Code composer shows how much of the history budget is left before the
  CLI auto-compacts, as a ring that fills and warns under 25%. Clicking it
  compacts now. It measures what the next turn would actually send, not the
  whole thread, since older turns are dropped before they reach the CLI.
- The VS Code toolbar's actions moved to the trailing edge with larger glyphs,
  and "new conversation" is a speech bubble with a plus rather than a pencil,
  which read as editing the conversation already open.
- **The three overlapping web surfaces are one.** Aster used to ship a
  `websearch` plugin that made it spawn a copy of itself as a subprocess for
  tools it could call in-process, so the model saw `websearch/search` beside
  `web/search` and `websearch/fetch_content` beside `web/extract`, one pair
  needing an API key and the other not. DuckDuckGo is now a provider inside
  `aster-web`, the `aster-websearch` crate and the bundled plugin are gone, and
  the plugin's directory is removed from existing installs on the next start. A
  package the user installed under that name is left alone. One dispatch table
  now serves both the in-process server and `aster mcp serve web`, which
  replaces `aster mcp serve websearch` and exposes the whole catalogue rather
  than two tools; the old name still starts it.
- `aster init` scaffolds a `browser` server in place of the `chrome` and
  `playwright` stubs, which were disabled placeholders nothing ever wired up.
  Existing configs naming either still work and are still described to the
  model.

- **Permissions collapse into one rule language.** `allow`, `ask`, and `deny`
  now hold rules of the form `Edit(<glob>)`, `Read(<glob>)`, and
  `Bash(<command>:*)` (or `Bash(<command>)` for an exact line); a bare `Edit`,
  `Read`, or `Bash` covers everything that tool does. That replaces six keys
  and three separate matcher vocabularies. Precedence is `deny`, `ask`,
  `allow`, the built-in rules, then `mode`, so one `allow` entry overrides a
  single built-in without dropping the rest:
  `allow: ["Read(**/.env)"]` reads env files and leaves every other secret
  protected.
- **A `Bash` rule matches inside a shell invocation.** The agent is told to
  chain work through `bash -lc "one && two"`, which made every command look
  like `bash` to a matcher that only saw the binary name: `deny_exec: ["rm"]`
  never stopped `bash -lc "rm -rf x"`, and `sudo` and `curl` never triggered
  the risky-command pause. Rules are now matched against the command line and
  every command inside a script it carries, through quotes, past leading
  environment assignments, and a few levels of nesting.
- **The mode ladder no longer inverts.** Writing to `.git/**`,
  `**/.git/**`, `.github/workflows/**`, and `.husky/**` now asks in every mode
  instead of being refused outright, which is what `auto` already did while the
  looser `edit` refused. `plan < manual < auto < edit < yolo` now holds for
  every action: no mode denies something a looser one allows. Reading `.env`,
  `*.pem`, `*.key` and friends is still refused, since a secret cannot be taken
  back out of the model's context.
- `auto` and `edit` still differ in exactly one way, now stated as such: `auto`
  pauses on the built-in risky-command list (`sudo`, `rm`, `curl`, `ssh` …) and
  `edit` trusts commands, leaving only your own rules to stop one.
- `edit_file` writes outside the repository ask for approval instead of
  failing. `read_file` and `list_files` already did; `edit_file` refused, and
  the agent routed around it with a shell script, which put the write through
  the path with no prompt and no diff. Approval covers a directory for the
  session, `always` persists it, and the grants are stored apart from the read
  grants so approving a read never hands out a write. Yolo skips the prompt.
- `--effort` is no longer a global flag. It now belongs to the commands that
  run a model (chat, `aster review`, and `aster fix`), so it stops appearing in
  the help of commands that never reach a provider, and must be written after
  the subcommand: `aster review --effort high`.

### Removed

- `permissions.protected`, `permissions.secret_read`, `permissions.allow_exec`,
  `permissions.deny_exec`, and `permissions.use_default_protected`, all folded
  into the rule buckets. Bare globs in `allow` and `deny` are no longer
  accepted; write `Edit(<glob>)`.
- The `ask` and `deny` mode aliases retired in 0.3.0.

### Migration notes

- **Rewrite `permissions:` in `aster.yaml`.** A retired key now stops the run
  naming its replacements rather than silently dropping the protection it asked
  for. The mapping is direct:

  | Before | After |
  | --- | --- |
  | `allow: ["src/**"]` | `allow: ["Edit(src/**)"]` |
  | `deny: ["infra/**"]` | `deny: ["Edit(infra/**)"]` |
  | `protected: ["infra/**"]` | `ask: ["Edit(infra/**)"]` or `deny:` |
  | `secret_read: ["**/*.token"]` | `deny: ["Read(**/*.token)"]` |
  | `allow_exec: ["cargo"]` | `allow: ["Bash(cargo:*)"]` |
  | `deny_exec: ["curl"]` | `deny: ["Bash(curl:*)"]` |
  | `use_default_protected: false` | `use_default_rules: false` |

- **Headless runs cannot answer a prompt**, so anything reaching `ask` is
  refused there. A script that ran `curl` or `rm` under `mode: edit` now needs
  an explicit `allow: ["Bash(curl:*)"]`.

### Fixed

- **Backgrounding a dev server no longer costs its output, or the server.**
  A shell that started something long-running and printed before exiting hit a
  grandchild still holding the pipes: after a five second grace the captured
  output was thrown away and the whole process group was killed, so the command
  came back empty and the server it had just started was gone. What the child
  wrote is now kept, and a process deliberately left running is left running.
  A command that hangs is still killed on the timeout, group and all. The agent
  is also told how to start one: redirect it to a log, background it, and do
  not follow it with a `sleep`/`tail`/`curl` poll to watch it boot.

- **A `run_command` call that put the binary somewhere else lost the round.**
  The tool takes the binary in `command` and its arguments in `args`, and a
  model that sent the whole argv as a list in `command`, or left the binary at
  the front of `args`, got `run_command needs a \`command\`` back and spent the
  round on the error instead of the work. Both shapes now run. A call with
  nothing runnable in it still fails, but the message names the shape to send
  instead of repeating the complaint, since the bare complaint tended to get
  the same argument-less call again.

- **An MCP step is named, not recited.** A step's header took whichever string
  argument came first when it recognised none of them, so a tool called with a
  script or a blob of prose wore the whole thing as its name, wrapped over two
  lines, and buried the tool that actually ran. A label the tool wrote for
  itself now wins (`title`, `label`, `description`, `summary`), headers are held
  to one capped line, and an argument long enough to be the call's payload is
  left to the body, where the step's name carries the row instead.

- **`aster provider use baseten` adopted a model Baseten does not serve.** The
  catalog's example was `meta-llama/Llama-3.1-8B-Instruct`, retired since it was
  written, so switching to Baseten without `--model` wrote a dead ID and the next
  turn failed on the model rather than the switch. The entry now points at
  `deepseek-ai/DeepSeek-V4-Pro` and carries a recommended list, all of it checked
  against `GET /v1/models`. Baseten also gained `BASETEN_API_KEY`, so it no longer
  has to borrow the shared key.
- **Chat turns no longer carry a forced web search.** OpenRouter's `web` plugin
  searches on every request it rides along with and pastes the hits into the
  prompt, and it was attached to the tool-calling requests too, so an unrelated
  page could turn up in the `Sources` footer of a turn that never asked to
  search. Chat now leans on the `openrouter:web_search` server tool the model
  calls when it wants one; the plugin stays on the tool-less review stages.
- **`review.web_search` defaults to off**, since a search the turn did not ask
  for spends money per result and drags outside pages into the context. Set
  `web_search: true` in `aster.yaml` or `ASTER_WEB_SEARCH=1` to get it back.
- **`explore` runs outside-repo lookups in yolo instead of refusing them.** Yolo
  drops the sandbox and the write gate, but the read gate still bounced any path
  that left the repository out of the batch, so a lookup the mode had already
  allowed came back as "call this tool on its own". It now says what a step
  actually got wrong (no tool named, not a lookup, or bad arguments) rather than
  blaming the path, and a step is read whether the model labels it
  `tool`/`args`, `name`/`arguments`, or sends its arguments as a JSON string.
- **The VS Code panel no longer dies on a streamed table.** A table row is a
  block start, but the table branch only claims one when the separator row is
  already there, so mid-stream — header in, `|---|` not yet — no branch
  consumed the line and the renderer spun on it, pushing an empty paragraph per
  pass until the webview ran out of memory. It looked like a crash on tables
  that "went away" on reopening, because a replayed transcript arrives whole.
- **New conversation no longer wipes the one already open.** `reveal()` created
  a fresh editor tab every time instead of revealing the tab already there, and
  `attach` only claimed the active surface when none was set, so the new
  conversation went to a new tab while the "start fresh" message landed on the
  old one. Both ended up empty. The open tab is now reused wherever a command
  needs a surface, and a new conversation opens beside the old one, which keeps
  its thread. In the sidebar, which is a single surface, it still starts over
  in place.
- **Auto-scroll stops switching itself off.** A hidden panel measures zero
  height, which the follow logic read as "scrolled away" and left following off
  after the panel came back. Zero-height metrics are now ignored, and sending a
  message always returns you to the bottom wherever you had scrolled to.
- **Approving a plan now lets the turn act on it.** `exit_plan_mode` opened the
  edit tool but left the policy in `plan`, so every edit and command that
  followed was refused with "permissions mode is `plan`" and the agent looped
  against a plan it had just been told to carry out. Approval promotes the
  policy too.
- **Leaving plan mode sticks in VS Code.** The panel spawns one `aster chat`
  per turn with `--permission-mode`, so a promotion inside the turn was lost at
  the next one. `approval_request` now carries `kind` (`plan` or `action`), and
  approving a plan moves the panel to `edit` the way the TUI already did.
- `aster chat --permission-mode auto` no longer exits with a usage error. The
  mode existed in `aster.yaml` and in the VS Code picker but not in the flag,
  so choosing **Auto** in the panel killed every turn with exit code 2 until
  another mode was picked.

- The `cli-toolbox` skill now tells the agent to read a tool before guessing at
  it: confirm the binary exists, run `--help` after a usage error rather than
  re-sending a near-identical shape, and go read the docs online when help does
  not settle it. Blind retries were burning a turn's budget without ever
  resolving the question.
- Sandbox denials are recognized on macOS again. The coaching that explains a
  blocked path matched `permission denied` and `eperm`, but macOS prints
  `operation not permitted`, so the note never fired on the platform it was
  written for.

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
[0.3.0]: https://github.com/Zfinix/aster/compare/v0.2.0...cli-v0.3.0
[0.4.0]: https://github.com/Zfinix/aster/compare/cli-v0.3.0...cli-v0.4.0
[Unreleased]: https://github.com/Zfinix/aster/compare/cli-v0.4.0...HEAD
