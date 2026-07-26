---
name: aster-fix-workflow
description: Pipe aster review findings into aster fix to generate and apply patches safely, using review --json, fix --findings-json, dry-run inspection, --apply, and permission gating. Use when asked to auto-fix review findings, apply aster's suggested edits, or build a review-then-fix pipeline.
---

# Review → fix workflow

`aster fix` takes findings produced by `aster review --json` and asks the model for concrete edits. It is dry-run by default and only writes with `--apply`, subject to the `permissions` block in aster.yaml.

## The pipeline

```sh
aster review --json > findings.json        # 1. review, capture findings
aster fix --findings-json findings.json    # 2. dry-run: preview proposed edits
aster fix --findings-json findings.json --apply   # 3. write to the working tree
```

Or as one stream:

```sh
aster review --json | aster fix --findings-json - --apply
```

## Rules

- ALWAYS run the dry-run first and read the preview before `--apply`. Never jump straight to `--apply` on findings you have not seen.
- `--apply` writes to the working tree only; it never commits. Review the diff with `git diff` afterward and commit yourself.
- Writes are gated by `permissions` in aster.yaml (`mode`, `allow`/`deny` globs, protected paths). In headless runs `mode: ask` denies, so automation needs `mode: auto` with explicit `allow` globs.
- `--json` emits one JSON array of per-finding results (useful to report which fixes applied, failed, or were skipped).
- `--model` overrides the fix model (else `ASTER_MODEL`, else aster.yaml); `--repo-root <DIR>` sets the root the finding paths are relative to.

## Curating findings

`aster fix` fixes whatever is in the array, so filter first. Findings are JSON; drop low-confidence or out-of-scope entries before fixing, e.g.:

```sh
aster review --json | jq '[.[] | select(.confidence >= 0.8)]' | aster fix --findings-json - 
```

## After applying

Run the project's tests/build to confirm the fixes hold, then commit. Treat model edits like any other patch: verified, not trusted.
