---
name: git-worktrees
description: Isolating work in a linked worktree so another branch can be built, tested, or fixed without touching the current checkout. Use when the working tree is dirty or busy and the task needs a different branch checked out.
---

# Git worktrees

1. **Reach for a worktree only when the current checkout must stay put.** The
   user is mid-edit, the tree is dirty, or a long build is running and the task
   needs a different branch. A clean idle tree does not need one: `git stash`
   or a plain `git checkout` is simpler.
2. **List before creating.** `git worktree list` first; pick a path and branch
   that are not already taken.
3. **Create with an explicit branch.** New branch:
   `git worktree add <path> -b <branch>`. Existing branch:
   `git worktree add <path> <branch>`. A branch checked out in one worktree
   cannot be checked out in another: `fatal: '<branch>' is already used by
   worktree at '<path>'`. Pick a new branch name instead of forcing it.
4. **Put the worktree where the sandbox can write and git will not sweep it.**
   Inside the repo under a gitignored path (`<repo>/target/wt/<name>`) or in
   `/tmp`. A sibling directory outside the repo fails the filesystem write
   policy, and an untracked directory inside the repo pollutes `git status`
   and `git add` sweeps. Verify the path is actually ignored with
   `git check-ignore -q <path>`; if it is not, add it to `.git/info/exclude`
   rather than editing the user's `.gitignore`.
5. **Know what is shared.** Commits and branches are shared through the one
   object store: a commit made in the worktree is immediately visible from the
   main checkout. HEAD, the index, and untracked files are per-worktree.
   Build artifacts (`target/`, `node_modules/`) are not shared either: the
   first build in a fresh worktree is a full cold build. Budget for it and
   say so if it will be slow.
6. **Run checks inside the worktree.** `cd <path>` first, then the project's
   own verbs (`make test`, `cargo test -p <crate>`). Running them from the
   main repo tests the wrong tree.
7. **Remove with git, not with `rm`.** `git worktree remove <path>` when the
   work is done. It refuses a dirty tree:
   `contains modified or untracked files, use --force to delete it`. Reset or
   clean the tree first, or use `--force` only when losing the changes is the
   agreed outcome. `rm -rf` by hand leaves a `prunable` entry; repair with
   `git worktree prune -v`.
8. **The branch outlives the worktree.** Removing the worktree keeps its
   branch. Delete it too when the work was abandoned (`git branch -D` counts
   as destructive: say what is being deleted and why).
9. **Never `mv` a worktree directory.** It breaks the gitdir link. Use
   `git worktree move <path> <new-path>`.
10. **Leave no worktrees behind.** A worktree created for one task is removed
    in the same session once its check ran, unless the user asked to keep it.
    `git worktree list` at the end of the task proves the cleanup.