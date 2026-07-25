# Aster Desktop

A Tauri desktop shell around the [`aster`](../README.md) CLI. Pick a repo, choose
what to review (working tree, a git range, a PR, or a diff file), and watch
verified findings stream in. A chat panel lets you ask the same repo questions.

The app does not reimplement the review pipeline or the chat agent. It shells out
to the CLI and renders its output, so it stays in lockstep with `aster`.

## Getting started

You need [Bun](https://bun.sh), a Rust toolchain (the CLI builds from the same
workspace), and the [Tauri system prerequisites](https://tauri.app/start/prerequisites/)
for your platform.

```sh
cd desktop
bun install
bun run dev
```

`bun run dev` builds the `aster` CLI from the root workspace, stages it as a
sidecar, then launches the app. The first run compiles the CLI, so it takes a
while; later runs are fast.

When the window opens, pick a repo, choose a review scope, and run a review.
Findings, phases, and token usage stream in as the CLI emits them.

## How-to guides

### Run against a CLI you already built

Set `ASTER_CLI_BIN` to an existing binary to skip recompiling during the sidecar
step (CI does this with the release `build` job's artifact):

```sh
ASTER_CLI_BIN=/path/to/aster bun run dev
```

To point the running app at a specific binary at launch instead, set `ASTER_BIN`
(see [Binary resolution](#binary-resolution) for the full order).

### Choose a model, endpoint, or key

Values you enter in the UI are passed to the CLI as `ASTER_MODEL` and
`ASTER_API_KEY`. Leave them blank and the CLI falls back to the repo's `.env` and
`aster.yaml`, exactly as it does on the command line. See the CLI's
[Configuration](../README.md#configuration) for the resolution rules.

### Build a distributable bundle

```sh
bun run build
```

This stages the sidecar, builds the frontend, and produces a self-contained
Tauri bundle. End users do not need `aster` installed: the CLI ships inside the
bundle as an [`externalBin`](https://tauri.app/develop/sidecar/) sidecar.

### Regenerate app icons

```sh
bun run icon
```

Regenerates the icon set from `src-tauri/icons/source-icon.png`.

## Reference

### Scripts

| Script | Does |
| --- | --- |
| `bun run dev` | Build + stage the sidecar, then launch the app (Tauri dev) |
| `bun run build` | Build the sidecar and produce a distributable bundle |
| `bun run sidecar` | Build `aster` from the workspace and stage it only |
| `bun run icon` | Regenerate icons from `src-tauri/icons/source-icon.png` |

Always go through the `bun run` scripts. A bare `cargo build` inside `src-tauri`
fails until the sidecar has been staged.

### Binary resolution

At launch, the CLI binary is resolved in this order:

1. `ASTER_BIN` environment variable
2. the bundled sidecar (`aster-cli`, present in a packaged build)
3. `../target/{release,debug}/aster` (the workspace build, for dev)
4. `aster` on `PATH`

### Environment variables

| Variable | Purpose |
| --- | --- |
| `ASTER_BIN` | Force a specific CLI binary at launch (resolution step 1) |
| `ASTER_CLI_BIN` | Reuse an already-built binary during the sidecar step instead of compiling |
| `ASTER_MODEL` | Model passed through from the UI to the CLI |
| `ASTER_API_KEY` | Provider key passed through from the UI to the CLI |

### CLI commands the app invokes

| Action | Command |
| --- | --- |
| Review | `aster review --stream` (NDJSON events: diff, phases, findings, usage) |
| Chat | `aster chat --messages-json - --json` |

Both run with the selected repo as the working directory. Log output (stderr)
tails into the activity log.

## How it works

The shell is a thin renderer over the CLI's streaming output. A review invokes
`aster review --stream` and parses its NDJSON events (diff, phases, findings,
usage) as they arrive, so findings appear incrementally rather than all at once.
Chat messages go through `aster chat --messages-json - --json` the same way. In
both cases the CLI runs with the chosen repo as its working directory, and its
log feed (stderr) is tailed into the activity log.

Keeping the pipeline in the CLI is deliberate. The desktop app owns presentation
and the CLI owns behavior, so model resolution, the review algorithm, and the
chat agent have exactly one implementation. The app inherits CLI improvements
without changes.

The crate is standalone (`desktop/src-tauri`), deliberately outside the
`crates/*` workspace so it never perturbs the library crates or their lockfile.
A packaged build bundles the CLI as a sidecar, staged by
`scripts/prepare-sidecar.mjs` at `src-tauri/binaries/aster-cli-<target-triple>`,
which is why an end-user install needs nothing beyond the app itself.
