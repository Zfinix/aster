---
name: dependency-upgrade
description: Upgrading dependencies without breaking the build or drowning in a mega-diff. Use when bumping package versions, resolving audit warnings, or when the user asks to update dependencies.
---

# Dependency upgrade

1. **See the landscape before touching anything.** `cargo outdated`,
   `npm outdated`, `bun outdated`: know which bumps are patch, minor, major.
2. **One major at a time.** Batch patch and minor bumps freely; each major
   version gets its own upgrade, its own check run, and its own commit. A
   ten-major mega-bump that fails tells you nothing about which one broke.
3. **Read the changelog for majors.** Search for "breaking", "migration",
   "removed". Five minutes of changelog beats an hour of compile-error
   archaeology.
4. **Let the lockfile do its job.** Regenerate it with the repo's own
   manager, commit it with the manifest, and never hand-edit it.
5. **Full check between steps.** Build plus tests after each major, quick
   check after the minor batch. The first red stops the line; fix or revert
   that bump before continuing.
6. **Audit fixes are upgrades too.** `npm audit fix --force` is a blind major
   bump wearing a security costume; treat the flagged package to the same
   one-at-a-time process.
7. **Report version deltas, not vibes.** "tokio 1.38 to 1.41, serde pinned,
   axum held back: 0.8 needs the router rewrite." The user should know what
   moved and what deliberately did not.
