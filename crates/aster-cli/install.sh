#!/usr/bin/env bash
# Aster CLI installer.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/zfinix/aster/main/crates/aster-cli/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/zfinix/aster/main/crates/aster-cli/install.sh | bash -s -- --version v0.3.0
#   curl -fsSL https://raw.githubusercontent.com/zfinix/aster/main/crates/aster-cli/install.sh | bash -s -- --dir ~/.local/bin
#
# Environment:
#   ASTER_VERSION   pin to a specific version (e.g. v0.3.0)
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

# Termux reports `Linux` here like any other distro, but it is bionic and has no
# glibc loader, so the GNU build dies with a bare "not found". `uname -o` and
# $PREFIX are what tell the two apart.
is_android() {
  [ "$(uname -o 2>/dev/null)" = "Android" ] && return 0
  case "${PREFIX:-}" in */com.termux/*) return 0 ;; esac
  return 1
}

# The gnu build is compiled against the release runner's glibc and refuses to
# start on anything older, so a system below that floor takes the static musl
# build instead. Alpine and friends report no glibc at all and land there too.
GLIBC_FLOOR_MAJOR=2
GLIBC_FLOOR_MINOR=35

glibc_too_old() {
  local v
  # musl's ldd exits non-zero and prints nothing matchable; pipefail would take
  # the whole script down with it, so the failure has to be swallowed here.
  v="$(ldd --version 2>/dev/null | head -n1 | grep -oE '[0-9]+\.[0-9]+' | tail -n1 || true)"
  [ -z "$v" ] && return 0
  local major="${v%%.*}" minor="${v#*.}"
  [ "$major" -lt "$GLIBC_FLOOR_MAJOR" ] && return 0
  [ "$major" -eq "$GLIBC_FLOOR_MAJOR" ] && [ "$minor" -lt "$GLIBC_FLOOR_MINOR" ] && return 0
  return 1
}

case "$OS" in
  Darwin) os_id="apple-darwin" ;;
  Linux)
    if is_android; then
      os_id="linux-android"
    elif glibc_too_old; then
      os_id="unknown-linux-musl"
    else
      os_id="unknown-linux-gnu"
    fi
    ;;
  *) err "unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64) arch_id="x86_64" ;;
  arm64|aarch64) arch_id="aarch64" ;;
  *) err "unsupported arch: $ARCH"; exit 1 ;;
esac

if [ "$os_id" = "linux-android" ] && [ "$arch_id" != "aarch64" ]; then
  err "Aster ships an Android build for aarch64 only, and this device is $ARCH."
  err "Build it instead: pkg install rust clang binutils make pkg-config git"
  err "                 cargo install --git https://github.com/${REPO} aster-cli"
  exit 1
fi

TARGET="${arch_id}-${os_id}"
API="https://api.github.com/repos/${REPO}/releases"

if [ "$VERSION" = "latest" ]; then
  info "Resolving latest CLI release"
  TAG="$(curl -fsSL "${API}?per_page=100" | sed -n 's/.*"tag_name":[[:space:]]*"\(cli-v[^"]*\)".*/\1/p' | head -n1)"
  if [ -z "$TAG" ]; then
    err "could not resolve latest CLI release from ${API}"
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

asset_url() { printf 'https://github.com/%s/releases/download/%s/aster-%s-%s.tar.gz' "$REPO" "$TAG" "$PLAIN_VERSION" "$1"; }

# A release older than the musl builds has no such asset, and a glibc build can
# still be the only thing published. Fall back rather than fail on a 404.
if ! curl -fsIL "$(asset_url "$TARGET")" >/dev/null 2>&1; then
  case "$TARGET" in
    *-unknown-linux-musl) FALLBACK="${arch_id}-unknown-linux-gnu" ;;
    *-unknown-linux-gnu)  FALLBACK="${arch_id}-unknown-linux-musl" ;;
    *) FALLBACK="" ;;
  esac
  if [ -n "$FALLBACK" ] && curl -fsIL "$(asset_url "$FALLBACK")" >/dev/null 2>&1; then
    info "No ${TARGET} build in ${TAG}; using ${FALLBACK}"
    TARGET="$FALLBACK"
  fi
fi

ASSET="aster-${PLAIN_VERSION}-${TARGET}.tar.gz"
URL="$(asset_url "$TARGET")"
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

printf '\nGet started:\n  cd your-repo\n  aster\n'
printf 'The first run asks how you want to connect: sign in, paste a key, or a local model.\n' 
