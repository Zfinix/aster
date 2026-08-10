---
name: cli-toolbox
description: Getting exactly the output you need from CLI tools in one call: JSON flags, slicing, counting, non-interactive flags. Use when inspecting command output, working with JSON, probing HTTP, or when a command might prompt, page, or spew.
---

# CLI toolbox

1. **Read the tool before guessing at it, and never guess twice.** First
   contact with a CLI you have not used this session: `command -v <tool>` to
   confirm it is installed. When a call fails on usage rather than on the task
   ("unknown flag", "accepts at most 1 arg(s)", "unknown command"), the next
   call is `<tool> --help`, or `<tool> <subcommand> --help` for the exact
   subcommand that failed. Never re-send a shape that just failed with a small
   edit; that is how a turn burns twenty calls and lands nowhere.
   When the tool is missing, or its help does not settle the question, go and
   read: `aster_mcp` with `action: "execute"`, `name: "web/search"` for the
   flag, or `name: "web/extract"` on the tool's documentation URL. Going online
   once costs less than four blind retries, and unlike them it ends the
   question. Say so plainly if the tool simply is not installed, rather than
   quietly substituting a different one.

2. **Ask the tool for structured output.** Most modern CLIs have it:
   `gh pr view 12 --json title,files`, `cargo metadata --format-version 1`,
   `docker ps --format json`, `npm ls --json`. Parse with `jq -r '.field'`
   instead of eyeballing prose.
3. **Slice files without reading them whole.** `sed -n '120,160p' file` for a
   line range, `grep -n -m5 "pattern" file` for first hits with line numbers,
   `wc -l file` before deciding how to read it.
4. **Count and rank instead of scrolling.**
   `grep -c` for how many, `sort | uniq -c | sort -rn | head` for what
   dominates, `du -sh */ | sort -rh | head` for what is big.
5. **Probe HTTP minimally.** Status only:
   `curl -s -o /dev/null -w "%{http_code}" URL`. Content check:
   `curl -s URL | grep -c "expected"`. Never dump a whole page to look at
   one thing.
6. **Force non-interactive mode.** Anything that might prompt hangs the call:
   use `--yes`/`-y`, `--force` where safe, `GIT_TERMINAL_PROMPT=0`,
   `DEBIAN_FRONTEND=noninteractive`, `CI=1`, `</dev/null` as a last resort.
   Anything that might page gets `--no-pager` or `| cat`.
7. **Quiet flags cut noise at the source.** `npm install --silent`,
   `cargo build -q`, `git -q` variants. Prefer them over filtering noise
   afterwards.
8. **Batch transformations with xargs, not loops of calls.**
   `grep -rl "old_name" src | head -20` to see the blast radius, then one
   edit per file, not one search per file.
9. **Never `sed -i` a repository file.** Use `edit_file`: it is exact,
   previewed, and audited. `sed -i` also differs by platform (BSD needs
   `sed -i ''`, GNU takes `sed -i`), which makes it a portability bug on top
   of an audit hole. Stream edits (`sed -n '10,40p'`, `sed 's/x/y/'` in a
   pipe) are fine anywhere.
10. **Check the platform line before GNU-only flags.** The environment note
   says macos or linux. On macos the stock tools are BSD: no `grep -P` (use
   `grep -E`), different `date` arithmetic (`-v+1d`, not `-d "+1 day"`), no
   `xargs -d`. When a one-liner needs GNU behavior, reshape it around
   portable flags instead of assuming Linux.
11. **On Windows there is no sed or grep.** Stock tools are PowerShell:
    `Select-String` for grep, `Get-Content -Tail 20` for tail. Ask the
    environment note, not habit, which world you are in.
