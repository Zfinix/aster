---
name: supply-chain-safety
description: Installing and adding dependencies without executing an attacker's code. Use before any package install or add, when a lockfile changes, or when choosing a new dependency.
---

# Supply chain safety

1. **Install with scripts off.** Lifecycle hooks (preinstall, postinstall,
   prepare) run arbitrary code with your privileges and are the top npm
   attack vector: `npm ci --ignore-scripts`, `pnpm install --ignore-scripts`,
   `bun install` (scripts off by default for new deps). If the project truly
   needs a hook (native builds), run that one package's build explicitly
   afterward and say so.
2. **Reproduce, do not resolve.** In an existing repo use the lockfile
   verbatim: `npm ci`, `pnpm install --frozen-lockfile`,
   `bun install --frozen-lockfile`. Plain `npm install` may silently upgrade
   what the repo pinned.
3. **Verify a package exists before adding it.** Models hallucinate package
   names, and attackers register those names (slopsquatting). Check the
   registry first: `npm view <name> versions time downloads` — a package
   that appeared last week with three downloads is not the library you
   meant.
4. **Prefer boring versions.** The freshest release is the compromise
   window; recent campaigns shipped malware in brand-new patch versions of
   trusted packages. Pin exact versions and let a new release age unless it
   fixes something you need.
5. **Read the lockfile diff before committing it.** New transitive deps,
   changed registry URLs, or git/tarball sources appearing in a routine
   change are the dependency-confusion signature. `git diff --stat` the
   lockfile and question anything you did not intend to add.
6. **Never pipe the internet into a shell.** No `curl ... | bash`, no
   `wget ... | sh`. Download to a file, read it, then decide.
7. **Audit is a signal, not a chore.** `npm audit --omit=dev` after installs;
   report criticals to the user rather than auto-fixing with `--force` (that
   is a blind major bump).
