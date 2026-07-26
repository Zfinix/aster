#!/usr/bin/env bash
# Bump the version across the Rust workspace and the desktop app in lockstep.
# Usage: scripts/bump-version.sh <patch|minor|major|X.Y.Z>
set -euo pipefail

cd "$(dirname "$0")/.."

arg="${1:-patch}"

# Source of truth: the workspace package version in the root Cargo.toml.
current="$(awk '
  /^\[workspace\.package\]/ { f = 1 }
  f && /^version[[:space:]]*=/ { gsub(/[",]/, "", $3); print $3; exit }
' Cargo.toml)"

if [ -z "$current" ]; then
  echo "could not read the current version from Cargo.toml" >&2
  exit 1
fi

case "$arg" in
  major|minor|patch)
    IFS=. read -r major minor patch <<<"$current"
    case "$arg" in
      major) major=$((major + 1)); minor=0; patch=0 ;;
      minor) minor=$((minor + 1)); patch=0 ;;
      patch) patch=$((patch + 1)) ;;
    esac
    new="$major.$minor.$patch"
    ;;
  [0-9]*.[0-9]*.[0-9]*)
    new="$arg"
    ;;
  *)
    echo "invalid version bump: $arg (want patch|minor|major|X.Y.Z)" >&2
    exit 1
    ;;
esac

# Rewrite the version key only inside a [package] or [workspace.package] table,
# never a dependency's version.
bump_cargo() {
  local file="$1"
  awk -v new="$new" '
    /^\[/ { in_pkg = ($0 == "[package]" || $0 == "[workspace.package]") }
    in_pkg && /^version[[:space:]]*=/ { sub(/"[^"]*"/, "\"" new "\""); in_pkg = 0 }
    { print }
  ' "$file" > "$file.tmp" && mv "$file.tmp" "$file"
}

# Rewrite the first top-level "version" key in a JSON manifest.
bump_json() {
  local file="$1"
  awk -v new="$new" '
    !done && /"version"[[:space:]]*:/ {
      sub(/"version"[[:space:]]*:[[:space:]]*"[^"]*"/, "\"version\": \"" new "\"")
      done = 1
    }
    { print }
  ' "$file" > "$file.tmp" && mv "$file.tmp" "$file"
}

bump_cargo Cargo.toml
bump_cargo crates/aster-models/Cargo.toml
bump_cargo crates/aster-index/Cargo.toml
bump_cargo desktop/src-tauri/Cargo.toml
bump_json desktop/package.json
bump_json desktop/src-tauri/tauri.conf.json

# Keep Cargo.lock's workspace entries in step; ignore if offline.
cargo update --workspace --quiet 2>/dev/null || true

echo "bumped $current -> $new"
echo "next: git commit -am \"chore: release v$new\" && git tag cli-v$new"
