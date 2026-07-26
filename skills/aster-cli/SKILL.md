---
name: aster-cli
description: Guidance for using the `aster` CLI to run AI code reviews, chat with the review agent, apply fixes, and manage sessions, memory, and skills. Use when running or designing aster commands, reviewing a diff or PR with aster, or when the user mentions `aster review`, `aster chat`, `aster fix`, or aster.yaml.
---

# Aster CLI

Aster is an AI code review agent: it reads a diff, forms hypotheses about defects, verifies them against the codebase, and reports shaped findings. It runs locally against the current repo and needs an `aster.yaml` (created by `aster init`).

## Setup

```sh
aster init          # pick a provider, write aster.yaml, store the API key
aster login         # optional: link GitHub via device flow (needed for PR reviews)
```

`aster init` is interactive. Run it once per repo; keys are stored outside the repo so `aster.yaml` is safe to commit.

## Reviewing code

```sh
aster review                    # review the current branch against its base
aster review main..HEAD         # review an explicit range
aster review path/to/file.rs    # review a single file
aster review --pr 123           # review a GitHub PR (requires aster login)
```

Review output is a list of findings. Prefer reviewing the smallest meaningful diff: a branch or range, not the whole repo.

## Chat and fixes

```sh
aster chat                      # interactive TUI with the review agent
aster chat --print "question"   # one-shot answer to stdout (use this from scripts and agents)
aster fix                       # apply model-generated fixes for findings (dry-run by default)
```

As an agent, always use `aster chat --print` rather than the TUI. `aster fix` never writes without an explicit apply flag; run it plain first and inspect the dry-run output.

## Sessions and memory

```sh
aster sessions                  # list saved chat sessions for this repo
aster sessions show <id>        # print a session's full transcript
aster memory                    # list stored durable memory
aster memory add "fact"         # append a fact to project memory
aster memory add --title t "…"  # save a titled memory block
```

Both `sessions` and `memory` accept `--json` for machine-readable output; prefer it when parsing.

## Skills

Aster loads agent skills from `.aster/skills` (project) and `<config>/aster/skills` (user-global).

```sh
aster skills add owner/repo     # install skills from a GitHub repo (also: git URL or local path)
aster skills add -g owner/repo  # install user-global instead of project
aster skills add owner/repo -l  # list what a source offers without installing
aster skills list               # list installed skills (--json for machine output)
aster skills use owner/repo@x   # print a skill's instructions without installing
aster skills find <query>       # search GitHub for skills interactively
aster skills update             # update all installed skills
aster skills remove <name>      # remove a skill
aster skills init <name>        # scaffold <name>/SKILL.md
```

`add` flags: `-s/--skill` to pick specific skills (`*` for all), `--all` for everything without prompts, `-y` to skip confirmation, `--force` to overwrite, `--full-depth` to keep searching inside a directory that already has a `SKILL.md`.

## Conventions

- Non-interactive contexts (CI, agents): use `--print`, `--json`, and `-y` variants; never invoke the TUI.
- Every command supports `--help`; check it before guessing flags.
