---
name: package-managers
description: Choosing the right JavaScript package manager and diagnosing install failures. Use before any install, add, or script command in a JS/TS repo, and when an install fails.
---

# Package managers

1. **The lockfile picks the manager.** `bun.lock`/`bun.lockb` means bun,
   `pnpm-lock.yaml` means pnpm, `yarn.lock` means yarn, `package-lock.json`
   means npm. Never mix: running npm in a bun repo creates a second lockfile
   and a second dependency tree.
2. **Run from the directory that owns the lockfile**, not the repo root of a
   monorepo.
3. **Scripts go through the owner.** `bun run build`, `pnpm run test`;
   one-offs with `bunx tool` or `pnpm dlx tool`, never bare `npx` in a
   non-npm repo.
4. **A failed install is not a reason to switch managers.** If `bun install`
   fails, `npm install` in the same repo fails for the same underlying reason
   and leaves a mess behind. Diagnose the actual error instead.
5. **Permission errors during installs are usually the sandbox.** Writes are
   allowed in the repository, temp directories, and package caches
   (`~/.npm`, `~/.bun`, `~/.cargo`). An error naming another path means the
   sandbox blocked it; report the path instead of retrying.
6. **Offline or turbo mode blocks the network.** Installs need the network;
   say that plainly instead of retrying the install.
