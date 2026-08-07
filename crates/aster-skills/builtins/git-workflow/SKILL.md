---
name: git-workflow
description: Using git without hanging, clobbering, or over-committing. Use before any git command that changes state (commit, push, checkout, rebase, reset) and when inspecting repo history or status.
---

# Git workflow

1. **Never use interactive flags.** There is no terminal to answer them, so the
   command hangs until it is killed. Never: `git rebase -i`, `git add -i`,
   `git add -p`, `git commit` without `-m`.
2. **Disarm the pager.** `git --no-pager log`, `git --no-pager diff`, or bound
   with `| head -40`. A paged command hangs exactly like an interactive one.
3. **Commit only when the user asked for a commit.** Finishing an edit is not
   permission to commit it.
4. **Stage named files.** `git add src/lib.rs src/tests/lib_test.rs`.
   Never `git add -A` or `git add .`: they sweep in unrelated changes and
   scratch files.
5. **Conventional commit message, one line.** `type(scope): summary`,
   imperative and lowercase, no trailing period. Write:
   `fix(sandbox): keep partial output on timeout`. Never a multi-paragraph
   body unless the user asked for one.
6. **No attribution trailers.** Never add `Co-Authored-By` or tool-generated
   footers unless the user asks.
7. **Branch before touching the default branch.** Asked to push while on
   main/master: create a branch and push that, or ask. Never push to the
   default branch as a side effect.
8. **Batch state checks into one call.**
   `bash -lc "git status --short; git log --oneline -5; git diff --stat"`.
9. **Destructive commands need an explicit ask.** `reset --hard`,
   `checkout -- .`, `clean -f`, `push --force`: only when the user asked for
   that outcome, and state first what will be lost. Prefer
   `push --force-with-lease` over `--force`.
