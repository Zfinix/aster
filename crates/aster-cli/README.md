# aster

The `aster` command-line tool: run Aster's verification-first code review on a
diff, locally or against a GitHub PR.

## Install

```bash
cargo install --path crates/aster-cli
```

This puts `aster` on your PATH (via `~/.cargo/bin`).

## Configure

Aster talks to any OpenAI-compatible provider. Set a key (env or `aster login`),
and optionally pin models:

```bash
export ASTER_API_KEY=...                              # or OPEN_ROUTER_API_KEY
export ASTER_BASE_URL=https://openrouter.ai/api/v1    # default
export ASTER_HYPOTHESIS_MODEL=google/gemini-3.1-flash-lite
export ASTER_VERIFY_MODEL=anthropic/claude-sonnet-5
```

A local `.env`, the shell env, and an `aster.yaml` (repo root, or
`~/.config/aster/aster.yaml`) all feed config. Precedence:
CLI flags > env > `aster.yaml` > defaults. API keys never live in `aster.yaml`.
See [aster.yaml.example](../../aster.yaml.example) for the full set of knobs
(models, `min_confidence`, `focus_areas`, `include`/`exclude`).

## Usage

```bash
aster review                       # review uncommitted changes (working tree)
aster review --tui                 # watch the review happen live
aster review --range main..HEAD    # a specific git range
aster review --diff change.diff    # a diff file, or - for stdin
aster review --json                # machine-readable findings
aster review -i "crates/**" -x "**/tests/**"   # scope by glob
```

### GitHub PRs

```bash
aster login                        # link a GitHub account (device flow, no secret)
aster review --pr 123              # fetch and review a PR's diff
aster review --pr 123 --comment    # post findings as inline PR comments
aster logout
```

Repo is auto-detected from the `origin` remote; override with `--repo owner/repo`.
Token resolution: `--token` > `GITHUB_TOKEN` > the token stored by `aster login`.

## How it works

`aster review` runs the harness pipeline: **hypothesize** (a cheap model
over-produces candidate defects) → **retrieve** (pull only the evidence each
candidate needs from a local symbol index) → **verify** (a second, adversarial
model call refutes the weak ones and gates on confidence; set
`ASTER_VERIFY_MODEL` to make it a genuinely independent, stronger model rather
than the hypothesis model) → **shape** (dedup and rank). See
[docs/ALGORITHM.md](../../docs/ALGORITHM.md) for the full design and
[docs/BENCHMARKS.md](../../docs/BENCHMARKS.md) for how the models were chosen.
