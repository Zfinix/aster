#!/usr/bin/env bash
# Aster CLI installer.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/zfinix/aster/main/crates/aster-cli/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/zfinix/aster/main/crates/aster-cli/install.sh | bash -s -- --version v0.1.0
#   curl -fsSL https://raw.githubusercontent.com/zfinix/aster/main/crates/aster-cli/install.sh | bash -s -- --dir ~/.local/bin
#
# Environment:
#   ASTER_VERSION   pin to a specific version (e.g. v0.1.0)
#   ASTER_PREFIX    install prefix (default: /usr/local, falls back to $HOME/.local)
#   ASTER_REPO      override repo (default: zfinix/aster)

set -euo pipefail

REPO="${ASTER_REPO:-zfinix/aster}"
VERSION="${ASTER_VERSION:-latest}"
PREFIX="${ASTER_PREFIX:-}"

err() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; }
info() { printf '\033[36m==>\033[0m %s\n' "$*"; }
ok() { printf '\033[32m✓\033[0m %s\n' "$*"; }

while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --version=*) VERSION="${1#*=}"; shift ;;
    --dir) PREFIX_BIN="$2"; shift 2 ;;
    --dir=*) PREFIX_BIN="${1#*=}"; shift ;;
    --repo) REPO="$2"; shift 2 ;;
    -h|--help)
      sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) err "unknown flag: $1"; exit 1 ;;
  esac
done

need() { command -v "$1" >/dev/null 2>&1 || { err "missing required tool: $1"; exit 1; }; }
need curl
need tar
need uname

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin) os_id="apple-darwin" ;;
  Linux)  os_id="unknown-linux-gnu" ;;
  *) err "unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64) arch_id="x86_64" ;;
  arm64|aarch64) arch_id="aarch64" ;;
  *) err "unsupported arch: $ARCH"; exit 1 ;;
esac

TARGET="${arch_id}-${os_id}"
API="https://api.github.com/repos/${REPO}/releases"

if [ "$VERSION" = "latest" ]; then
  info "Resolving latest CLI release"
  TAG="$(curl -fsSL "${API}/latest" | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)"
  if [ -z "$TAG" ]; then
    err "could not resolve latest release from ${API}/latest"
    exit 1
  fi
else
  case "$VERSION" in
    cli-v*) TAG="$VERSION" ;;
    v*) TAG="cli-${VERSION}" ;;
    *) TAG="cli-v${VERSION}" ;;
  esac
fi

PLAIN_VERSION="${TAG#cli-v}"
ASSET="aster-${PLAIN_VERSION}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET}"
SHA_URL="${URL}.sha256"

info "Installing aster ${PLAIN_VERSION} (${TARGET})"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

info "Downloading ${ASSET}"
curl -fsSL "$URL" -o "$TMP/$ASSET" || { err "download failed: $URL"; exit 1; }

if curl -fsSL "$SHA_URL" -o "$TMP/$ASSET.sha256" 2>/dev/null; then
  EXPECTED="$(cat "$TMP/$ASSET.sha256")"
  if command -v shasum >/dev/null 2>&1; then
    ACTUAL="$(shasum -a 256 "$TMP/$ASSET" | awk '{print $1}')"
  else
    ACTUAL="$(sha256sum "$TMP/$ASSET" | awk '{print $1}')"
  fi
  if [ "$EXPECTED" != "$ACTUAL" ]; then
    err "checksum mismatch (expected $EXPECTED, got $ACTUAL)"
    exit 1
  fi
  ok "checksum verified"
else
  info "no checksum published; skipping verification"
fi

tar -xzf "$TMP/$ASSET" -C "$TMP"
STAGED="$TMP/aster-${PLAIN_VERSION}-${TARGET}/aster"
if [ ! -x "$STAGED" ]; then
  err "binary not found in archive at expected path"
  exit 1
fi

if [ -z "${PREFIX_BIN:-}" ]; then
  if [ -n "$PREFIX" ]; then
    PREFIX_BIN="${PREFIX%/}/bin"
  elif [ -w /usr/local/bin ] 2>/dev/null; then
    PREFIX_BIN="/usr/local/bin"
  elif [ "$(id -u)" = "0" ]; then
    PREFIX_BIN="/usr/local/bin"
  else
    PREFIX_BIN="$HOME/.local/bin"
  fi
fi

mkdir -p "$PREFIX_BIN"

DEST="$PREFIX_BIN/aster"
SUDO=""
if [ ! -w "$PREFIX_BIN" ] && [ "$(id -u)" != "0" ]; then
  if command -v sudo >/dev/null 2>&1; then
    SUDO="sudo"
    info "Using sudo to write to $PREFIX_BIN"
  else
    err "$PREFIX_BIN is not writable and sudo is unavailable"
    exit 1
  fi
fi

$SUDO install -m 0755 "$STAGED" "$DEST"

ok "Installed $DEST"

case ":$PATH:" in
  *":$PREFIX_BIN:"*) ;;
  *)
    printf '\n'
    info "Add this to your shell profile so 'aster' is on PATH:"
    printf '  export PATH="%s:$PATH"\n\n' "$PREFIX_BIN"
    ;;
esac

if [ -x "$DEST" ]; then
  "$DEST" --version || true
fi
