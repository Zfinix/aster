
# ✳ Aster

**An open-source AI code review harness.**

Self-hostable, bring-your-own model, verification-first reviews that are precise and cheap.

[![CI](https://github.com/zfinix/aster/actions/workflows/ci.yml/badge.svg)](https://github.com/zfinix/aster/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](./LICENSE)

Built by [Instalog](https://instalog.dev).

---

> **Status: early, building in the open.** The core review algorithm and the
> `aster` CLI have landed. Deeper GitHub and Linear integrations are next. See [docs/](./docs).

## What is Aster?

![Aster demo animation](./aster.gif)

Aster reviews a code diff and emits verified, actionable findings. It is
model-agnostic (point it at any OpenAI-compatible provider) and self-hostable
(no vector DB, no external services in the core).

The insight: **code review is an adversarial verification task, not a generation
task.** So Aster separates *hypothesis* from *verification* and retrieves context
precisely instead of stuffing whole files into a prompt. That makes it both more
precise and cheaper, which turn out to be the same lever.

## Why Aster

- **Precision over volume.** A false positive costs more trust than a missed
  finding costs coverage. Every candidate is put on trial before it reaches you.
- **BYO model, no lock-in.** Any OpenAI-compatible endpoint: OpenRouter, Groq,
  OpenAI, or a local llama.cpp server.
- **Self-host first.** No vector DB and no external services in the review core.
  A diff and a local SQLite index are all it needs.
- **Cheap by design.** The expensive model tier is spent only on verifying what
  survives, never on the whole diff.

Compared to a plain "ask GPT to review this diff" prompt, Aster refutes its own
findings before showing them, retrieves the exact evidence each finding needs,
and gives you a confidence score you can gate on.

## Install

Aster is pre-release, so build it from source (Rust >= 1.85, edition 2024):

```bash
git clone https://github.com/zfinix/aster
cd aster
cargo install --path crates/aster-cli    # installs the `aster` binary
```

Or build without installing: `cargo build --release -p aster-cli` puts the
binary at `target/release/aster`.

## Quick start

Point Aster at a model provider, then review your current branch:

```bash
export ASTER_API_KEY=sk-...                          # your provider key
export ASTER_BASE_URL=https://openrouter.ai/api/v1   # or Groq / OpenAI / local
export ASTER_MODEL=openai/gpt-4o-mini

cd your-repo
aster review                     # reviews the current branch vs its base
```

More ways to scope a review:

```bash
aster review --range main..HEAD           # an explicit git range
git diff HEAD~1 | aster review --diff -    # a diff from stdin
aster review --pr 42                       # a GitHub PR (needs `aster login`)
aster review --pr 42 --comment             # post findings as inline PR comments

aster review --json                        # machine-readable output
aster review --tui                         # browse findings interactively
aster review -i "src/**/*.rs" -x "**/*.gen.rs"   # include / exclude globs
```

Run `aster review --help` for every flag.

## What a finding looks like

Text output (the default) leads with a summary, then each finding with its
severity, category, confidence, and location:

```
  ● 1 finding worth your attention.

  HIGH  correctness  1/1  74%
  Unchecked index can panic on an empty slice
  crates/aster-index/src/grep.rs:58
```

With `--json`, the same finding is emitted as structured data ready for CI or a
fix-agent:

```json
[
  {
    "file_path": "crates/aster-index/src/grep.rs",
    "line": 58,
    "severity": "high",
    "category": "correctness",
    "title": "Unchecked index can panic on an empty slice",
    "description": "into_inner().unwrap() assumes the mutex is uncontended; a poisoned lock panics the worker.",
    "suggestion": "Handle the PoisonError instead of unwrapping.",
    "confidence": 0.74
  }
]
```

## How it works

A cost-staged, verification-first pipeline:

```mermaid
flowchart LR
    A[HYPOTHESIZE] --> B[RETRIEVE] --> C[VERIFY] --> D[SHAPE]
```

1. **Hypothesize.** A *cheap* model over-produces candidate defects from the
   diff. Every candidate must carry a concrete `failure_scenario` or it is dropped.
2. **Retrieve.** Pull only the evidence that candidate's scenario needs: the
   changed hunk, a source window, the enclosing symbol and its definition, and
   references drawn from a local SQLite/FTS5 symbol index (no repo-wide walk).
3. **Verify.** A second model call, prompted to **refute**, kills
   plausible-but-wrong findings. A candidate survives only if the verifier
   reports confidence above a configurable threshold (`--min-confidence`, default 0.5).
   The verify pass uses the same model as hypothesis by default. Point it at a
   separate, stronger model with `ASTER_VERIFY_MODEL` for true independence.
4. **Shape.** Dedup (collapsing the same defect surfaced by multiple sources)
   and rank by `severity × confidence`, then emit canonical findings ready for
   inline comments, Linear issues, or a future fix-agent.

The expensive tier is spent only on adversarial verification of what survives,
never on the whole diff. Full write-up: [docs/ALGORITHM.md](./docs/ALGORITHM.md).

> **On precision.** The confidence gate filters the verifier's *self-reported*
> confidence, which is a useful heuristic, not a calibrated probability. Aster's
> end-to-end precision and false-positive rate are measured by the eval in
> [docs/BENCHMARKS.md](./docs/BENCHMARKS.md). Treat headline precision claims as
> backed by those numbers, not by the gate alone.

## Configuration

Precedence: **CLI flags > shell env > `aster.yaml` > built-in defaults.** API
keys are read only from the environment (or `aster login`), never from the file.

| Env var | Purpose | Default |
| --- | --- | --- |
| `ASTER_API_KEY` | Provider API key (required) | none |
| `ASTER_BASE_URL` | OpenAI-compatible endpoint | `https://openrouter.ai/api/v1` |
| `ASTER_MODEL` | Default model for any stage without an override | `openai/gpt-4o-mini` |
| `ASTER_HYPOTHESIS_MODEL` | Cheap, high-recall model for the hypothesis pass | falls back to `ASTER_MODEL` |
| `ASTER_VERIFY_MODEL` | Independent model for the adversarial verify pass | falls back to `ASTER_MODEL` |
| `ASTER_REASONING_EFFORT` | Thinking budget: `low` / `medium` / `high` / `off` | `low` |
| `ASTER_MAX_TOKENS` | Cap generated tokens (`0` or `off` disables) | `8000` |
| `ASTER_SEED` | Fixed sampling seed for reproducibility (`off` disables) | `0` |

For repo-level defaults (models, `min_confidence`, analyzers, include/exclude
globs), copy [`aster.yaml.example`](./aster.yaml.example) to `aster.yaml` in your
repo root. See [`.env.example`](./.env.example) for the full annotated env list.

## Repository layout

```
crates/
  aster-models/      domain types: findings, symbols, Candidate/Verdict
  aster-ai/          provider-agnostic OpenAI-compatible chat client
  aster-index/       zero-dep code index: SQLite + FTS5 + ripgrep
  aster-analyzers/   static-analysis engine (semgrep / ast-grep)
  symbol-extractor/  tree-sitter-tags symbol extraction (14 languages)
  aster-harness/     the review core (hypothesize -> verify -> shape)
  aster-cli/         the `aster` command-line interface
docs/
  ARCHITECTURE.md    crates + runtime diagrams
  ALGORITHM.md       the algorithm + cost model (paper notes)
  ANALYZERS.md       static-analysis integration
  BENCHMARKS.md      how the hypothesis model was chosen (reproducible)
```

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](./CONTRIBUTING.md) for the dev
setup and the three checks CI runs (`fmt`, `clippy`, `test`). For security
issues, see [SECURITY.md](./SECURITY.md).

## License

[Apache-2.0](./LICENSE). See [NOTICE](./NOTICE).
