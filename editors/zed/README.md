# Aster Zed extension

A Zed extension that runs Aster from the assistant panel.

## What it does

Registers one slash command:

- `/aster-review` runs `aster review` in the worktree and inserts the findings
  as text into the assistant panel.

## Install as a dev extension

1. Install the wasm target: `rustup target add wasm32-wasip1`
2. Build: `cargo build --target wasm32-wasip1` (from this directory)
3. In Zed: command palette, then `install dev extension`, and point it at this
   directory. Zed builds and loads it.

The `aster` binary must be on your PATH. From the aster repo, `make install`.
