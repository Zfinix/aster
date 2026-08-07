---
name: batched-bash
description: Getting everything one shell call can give: chaining, labeled stages, self-verifying pipelines. Use whenever more than one shell step is needed for one goal.
---

# Batched bash

1. **The project's own verbs beat hand-rolled pipelines.** If the environment
   note or repo shows a Justfile, Makefile, Taskfile, or package.json
   scripts, use those (`just build`, `make test`, `bun run check`): they
   encode the flags and env the project actually uses. Hand-roll only what
   the project has not named.
2. **One call per goal, not per command.** Each tool round costs a full model
   round-trip; the commands themselves are nearly free. Write
   `bash -lc "mkdir -p public && mv dashboard.html public/index.html && ls public"`.
   Never three separate calls for mkdir, mv, ls.
3. **Label the stages** so the output reads like a report and a failure names
   its stage:
   `echo "=== install ===" && bun install 2>&1 | tail -2 && echo "=== typecheck ===" && bunx tsc --noEmit 2>&1 | head -20`
4. **Make pipelines self-verifying.** Append the check to the action:
   `grep -q "export default" index.ts && echo OK || echo MISSING`.
5. **Bound every noisy step** inside the chain: `| head -20`, `| tail -5`,
   `--stat`, `-n 5`. The context you save is your own.
6. **Batch independent probes with separators:**
   `ls src; echo ---; ls tests; echo ---; head -5 Cargo.toml`.
7. **Repo content lookups do not go through the shell.** `search_files` and
   `find_files` already run ripgrep without the overhead; never shell out to
   `rg`, `grep -r`, `find`, or `fd` for repository files.
