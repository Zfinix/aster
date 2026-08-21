.DEFAULT_GOAL := check
.PHONY: fmt fmt-check lint test check desktop-check pre-push hooks \
	review review-tui review-stream chat chat-print fix init desktop web serve \
	release install bump

# Extra flags for the run targets, e.g. `make review ARGS="--pr 42"`.
ARGS ?=

# Format the whole workspace in place.
fmt:
	cargo fmt --all

# CI parity: the exact checks .github/workflows/ci.yml runs.
fmt-check:
	cargo fmt --all --check

lint:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace --all-targets

# Everything CI gates on. Run before pushing.
check: fmt-check lint test

# Typecheck the Tauri desktop frontend (not covered by the Rust CI job).
desktop-check:
	cd desktop && bun run tsc --noEmit

# Format first, then run the full gate. Handy as a pre-push step.
pre-push: fmt check

# Install a git pre-push hook that runs `make check`.
hooks:
	printf '#!/bin/sh\nexec make check\n' > .git/hooks/pre-push
	chmod +x .git/hooks/pre-push
	@echo "installed .git/hooks/pre-push -> make check"

# ---- Run the CLI in its various modes ----
# Review the current branch's diff. Add ARGS for a range/PR/file, e.g.
# `make review ARGS="--pr 42"` or `make review ARGS="--range main..HEAD"`.
review:
	cargo run -p aster-cli -- review $(ARGS)

# Same review, in the live full-screen TUI.
review-tui:
	cargo run -p aster-cli -- review --tui $(ARGS)

# Same review, streaming NDJSON events to stdout (what the desktop app consumes).
review-stream:
	cargo run -p aster-cli -- review --stream $(ARGS)

# Interactive chat agent (full-screen TUI).
chat:
	cargo run -p aster-cli -- chat $(ARGS)

# One-shot chat: `make chat-print ARGS="how do i fix finding 2"`.
chat-print:
	cargo run -p aster-cli -- chat --print $(ARGS)

# Apply model-generated fixes (dry-run unless ARGS includes --apply).
fix:
	cargo run -p aster-cli -- fix $(ARGS)

# First-time setup: pick a provider, write aster.yaml, store a key.
init:
	cargo run -p aster-cli -- init $(ARGS)

# Run the Tauri desktop app in dev (builds the CLI sidecar first).
desktop:
	cd desktop && bun run dev

# Build the browser UI into crates/aster-serve/ui, where the CLI embeds it.
# Needed once before `make release`/`make install` for `aster serve` to have a
# page to hand a browser.
web:
	cd editors/vscode && bun install --silent && bun run build:web

# Serve that UI from this repo on http://localhost:4187.
serve: web
	cargo run -p aster-cli -- serve $(ARGS)

# ---- Build / install ----
release:
	cargo build --release -p aster-cli

# --force so a rebuild at the same version still overwrites: the VS Code
# extension and the desktop app both shell out to the `aster` on PATH, so a
# stale binary makes source changes invisible to them.
install:
	cargo install --path crates/aster-cli --force

# Bump the version across the workspace and desktop app. Defaults to a patch bump;
# `make bump LEVEL=minor`, `make bump LEVEL=major`, or `make bump VERSION=0.2.0`.
LEVEL ?= patch
bump:
	./scripts/bump-version.sh $(if $(VERSION),$(VERSION),$(LEVEL))
