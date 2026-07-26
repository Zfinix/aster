---
name: aster-review-ci
description: Run aster code reviews non-interactively in CI, GitHub Actions, or from another agent. Covers `aster review --pr`, `--json`, `--stream`, `--comment`, diff-from-stdin, token handling, and filtering findings. Use when wiring aster into a pipeline, posting PR comments, or parsing review output programmatically.
---

# Aster review in CI and automation

`aster review` reads a diff, hypothesizes defects, verifies them against the indexed repo, and reports findings. Everything is flag-driven; nothing requires a terminal.

## Picking the diff

```sh
aster review                          # current branch vs its base
aster review --range main..HEAD       # explicit git range
aster review --diff changes.patch     # a saved diff file
git diff main... | aster review --diff -   # diff from stdin
aster review --pr 123                 # fetch a GitHub PR's diff
```

`--pr` needs a token: `--token`, else `GITHUB_TOKEN`, else the one stored by `aster login`. The repo defaults to the `origin` remote; override with `--repo owner/repo`.

## Machine-readable output

```sh
aster review --json                   # findings as one JSON array
aster review --stream                 # NDJSON progress events, one per line
```

Use `--json` in CI and pipe it to `aster fix` or your own tooling. `--stream` is for editors and UIs that want live progress. `--tui` is the interactive browser; never use it headless.

## Posting to the PR

```sh
aster review --pr 123 --comment       # post findings as inline PR comments
```

`--comment` implies `--pr`. Run it only when you intend to publish; the plain run is read-only.

## Scoping and thresholds

```sh
aster review -i "src/**/*.rs" -x "**/generated/**" --min-confidence 0.7
```

- `-i/--include` overrides `include` from aster.yaml; `-x/--exclude` adds to it. Both repeatable.
- `--min-confidence <0.0-1.0>` drops weak findings; falls back to aster.yaml.
- `--no-index` skips the symbol index for speed, at the cost of weaker verification. Prefer keeping the index in CI.
- `--repo-root <DIR>` points evidence retrieval at a checkout other than the cwd.

## Minimal GitHub Actions shape

```yaml
- run: aster review --pr ${{ github.event.pull_request.number }} --comment --json > findings.json
  env:
    GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
    ASTER_API_KEY: ${{ secrets.ASTER_API_KEY }}
```

The API key comes from `ASTER_API_KEY` (never from aster.yaml). An `aster.yaml` committed to the repo supplies models, analyzers, and filters; `aster init -y` writes a default one.
