# Agent Plugins

Aster is a client for the [Agent Plugins
specification](https://github.com/agentplugins/agent-plugins-spec) v1.0.0: an
open, vendor-neutral package format for the two things agents already share,
Agent Skills and MCP servers. A plugin published for any conformant client
installs into Aster unchanged.

## The package

A plugin is a directory with a manifest at its root. Component locations are
fixed, so the manifest never redirects discovery:

```text
my-plugin/
├── plugin.json          # required manifest
├── skills/              # each immediate child with a SKILL.md is one skill
│   └── summarize/
│       └── SKILL.md
├── mcp.json             # MCP servers, portable transport config
└── com.example.client/  # another client's private files, ignored here
```

The manifest is closed: `$schema` and `name` are required, and `version`,
`description`, `author`, `homepage`, `repository`, `license`, `keywords`, and
`extensions` are the only other fields allowed.

```json
{
  "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
  "name": "my-plugin",
  "version": "1.0.0",
  "description": "Summarizing, plus a local MCP server"
}
```

## Using plugins

```bash
aster plugins add owner/repo        # install from a repo
aster plugins add ./my-plugin -p    # install into this project only
aster plugins list                  # what is installed and what it contributes
aster plugins remove my-plugin      # uninstall, keeping its data directory
aster plugins validate ./my-plugin  # check a package you are authoring
```

Plugins install user-global under `<data>/aster/plugins`, or into
`.aster/plugins` with `-p`, where a project plugin shadows a global one of the
same name. A repository holding several plugins is offered as a list: one at
the repo root, or one per directory under it or under `plugins/`.

A plugin's skills join the session skill index alongside installed and built-in
ones. Precedence runs skills roots, then plugins, then built-ins, so a skill you
installed yourself always wins.

Its MCP servers join the configured ones as `<plugin>/<server>`, which keeps two
plugins from colliding on a common name like `search`. They are injected through
the same progressive bridge, and reached over the same transports, as any other
server ([docs/MCP.md](./MCP.md)).

## What Aster supports

Aster implements the whole v1 format:

- The closed `plugin.json` schema, including the non-fatal cases. An unknown
  top-level field or a non-object `extensions` is reported and ignored; any
  other violation rejects the plugin.
- Path containment. A package path that resolves outside the plugin root is
  refused, at the narrowest boundary the spec defines: one bad skill is skipped,
  one bad server entry is skipped, and only a bad manifest rejects the package.
- All three MCP transports. `stdio` servers expand `${PLUGIN_ROOT}` and
  `${PLUGIN_DATA}` in `args`, `env`, and `cwd`, default their working directory
  to the plugin root, and resolve `command` as a single token: a bare name goes
  to the platform's executable search, a `./` path resolves against the plugin
  root. `streamable-http` and `sse` servers connect to their `url` with the
  configured `headers`.
- `PLUGIN_ROOT` and `PLUGIN_DATA` in every plugin subprocess. `PLUGIN_DATA` is
  `<scope>/plugin-data/<name>`, created before the server starts and left alone
  by updates and by `aster plugins remove` unless you pass `--purge`. Put
  installed dependencies, generated code, and caches there; use `PLUGIN_ROOT`
  for what ships in the package.

Component types outside v1, such as commands and hooks, are ignored.

Aster claims no extension namespace of its own. Its own configuration stays in
`aster.yaml` and its own skills in `.aster/skills`, so a plugin never needs
Aster-specific files to work here.

## Authoring

`aster plugins validate` runs the same loader a session does and prints
everything it would otherwise log: the skills it found, the servers it found
with their transports, and every warning. It exits non-zero when the package
would be rejected outright.

```console
$ aster plugins validate .
my-plugin conforms to Agent Plugins 1.0.0
  skills:  summarize
  servers: everything (stdio), remote (streamable-http)
```
