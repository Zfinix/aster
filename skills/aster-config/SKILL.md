---
name: aster-config
description: Reference for aster.yaml, covering review models, analyzers, focus areas, include/exclude globs, min_confidence, and the permissions block that gates edits. Use when creating or editing aster.yaml, choosing hypothesis/verify models, enabling semgrep or ast-grep, or configuring edit permissions.
---

# Configuring Aster (aster.yaml)

`aster init` writes `aster.yaml` at the repo root (or `~/.config/aster/aster.yaml` with `-g`). Every field is optional. Precedence: CLI flags > shell env > aster.yaml > built-in defaults. API keys are NEVER read from this file; they come from `ASTER_API_KEY` or the key stored by `aster init` / `aster login`, so aster.yaml is safe to commit.

## Review block

```yaml
review:
  model: openai/gpt-4o-mini                      # fallback for stages with no override
  base_url: https://openrouter.ai/api/v1         # any OpenAI-compatible provider
  hypothesis_model: deepseek/deepseek-v4-flash   # cheap, high-recall pass
  verify_model: anthropic/claude-sonnet-5        # independent adversarial verify
  min_confidence: 0.6                            # drop findings below this (0.0-1.0)
  max_diff_bytes: 200000                         # cap the diff sent to the model
  analyzers: []                                  # [semgrep], [ast-grep], or both
  focus_areas: [correctness, security]           # bias the hypothesis pass
  include: []                                    # empty = everything except exclude
  exclude: ["target/**", "node_modules/**", "**/*.lock", "**/*.min.js"]
```

- Split models by stage: a cheap `hypothesis_model` for recall, a strong `verify_model` for precision. `model` fills any stage without an override. `ASTER_MODEL` env overrides all of it for a run.
- `analyzers` adds static-analysis backends whose findings also flow through verification: `semgrep` needs `semgrep`/`opengrep` on PATH; `ast-grep` needs `ast-grep`/`sg` on PATH plus `ASTER_ASTGREP_RULES` pointing at a rule file.

## Permissions block

Gates what the agent edits, reads, and runs:

```yaml
permissions:
  mode: edit              # plan | manual | auto | edit | yolo (prompts need the TUI; headless denies)
  allow: []               # e.g. ["Bash(cargo test:*)", "Edit(src/**)"]
  ask: []                 # e.g. ["Edit(migrations/**)"]
  deny: []                # e.g. ["Bash(npm publish:*)", "Edit(infra/**)"]
  use_default_rules: true # ask on .git/**, workflows; ask on sudo/rm/curl in `auto`; refuse secret reads
```

One rule language for all three tools: `Edit(<glob>)`, `Read(<glob>)`, `Bash(<command>:*)` for a prefix or `Bash(<command>)` for an exact line. A bare `Edit`, `Read`, or `Bash` covers everything that tool does. Precedence is `deny`, `ask`, `allow`, built-ins, then `mode`, so one `allow` entry overrides a single built-in without dropping the rest.

A `Bash` rule matches inside `bash -lc "…"`, so chaining does not slip a command past it.

Keep `use_default_rules: true` unless you have a specific reason; it is what makes the agent confirm before touching CI workflows and git internals, and what stops it reading env and key files.

## Environment variables

Env beats aster.yaml, and CLI flags beat env. Everything is optional except the API key.

Provider and models:

- `ASTER_API_KEY` - the provider API key (required; never read from aster.yaml)
- `ASTER_BASE_URL` - OpenAI-compatible endpoint
- `ASTER_MODEL` - fallback model for every stage
- `ASTER_HYPOTHESIS_MODEL` / `ASTER_VERIFY_MODEL` - per-stage overrides

Request tuning:

- `ASTER_TIMEOUT_SECS` / `ASTER_MAX_RETRIES` / `ASTER_DEADLINE_SECS` - HTTP timeout, retry count, overall deadline
- `ASTER_MAX_TOKENS` / `ASTER_SEED` / `ASTER_REASONING_EFFORT` - completion caps, deterministic seed, reasoning effort
- `ASTER_PRICE_PROMPT_PER_M` / `ASTER_PRICE_COMPLETION_PER_M` - $ per million tokens, for cost reporting

Review:

- `ASTER_ANALYZERS` - comma-separated analyzer list, e.g. `semgrep,ast-grep` (empty = LLM only)
- `ASTER_ASTGREP_RULES` - path to the ast-grep rule file
- `ASTER_VERIFY_CONCURRENCY` - parallel verification workers
- `ASTER_REPO` - repo name used in the summary (defaults to `local`)

## Conventions

- `aster init -y` writes the defaults with no wizard; `--force` overwrites an existing file.
- Deny-first: `deny` beats `ask`, which beats `allow`, which beats the built-ins.
- A headless run cannot answer a prompt, so anything reaching `ask` is refused there. Automation wants `mode: edit` with tight `allow` rules.
