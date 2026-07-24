# Aster Desktop

A small Tauri desktop shell around the `aster` CLI. Pick a repo, choose what to
review (working tree, a git range, a PR, or a diff file), and watch findings
stream in.

## How it works

The app shells out to `aster review --stream` and renders its NDJSON events
(diff, phases, findings, usage) live, tailing the CLI's log feed (stderr) into
the activity log. Ask messages shell out the same way, to
`aster chat --messages-json - --json`, with the repo as working directory. It
does not reimplement the review pipeline or the chat agent, so it stays in
lockstep with the CLI.

The binary is resolved in this order:

1. `ASTER_BIN` environment variable
2. `../target/{release,debug}/aster` (the workspace build)
3. `aster` on `PATH`

Model, base URL, and API key follow the same resolution as the CLI: values you
enter in the UI are passed as `ASTER_MODEL` / `ASTER_API_KEY`, otherwise the
CLI falls back to the repo's `.env` and `aster.yaml`.

## Develop

```sh
cargo build -p aster-cli   # from the repo root, so the CLI binary exists
cd desktop
npm install
npm run dev
```

## Build a bundle

```sh
npm run build
```

The app is a standalone crate (`desktop/src-tauri`), deliberately outside the
`crates/*` workspace so it never perturbs the library crates or their lockfile.
