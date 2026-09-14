#!/bin/sh
# Aster installer — https://withaster.dev
# Usage: curl -fsSL https://withaster.dev/install | sh
#
# Installs a prebuilt `aster` binary from GitHub Releases — no Rust required.
# If no prebuilt binary matches this platform, falls back to building from
# source with cargo (when available).
#
# Environment:
#   ASTER_VERSION   pin to a specific version (e.g. v0.1.0)
#   ASTER_REPO      override repo (default: zfinix/aster)
set -eu

REPO="${ASTER_REPO:-zfinix/aster}"
VERSION="${ASTER_VERSION:-latest}"
BIN="aster"
CRATE="aster-cli"
API="https://api.github.com/repos/${REPO}/releases"

c_info='\033[1;36m'
c_ok='\033[1;32m'
c_err='\033[1;31m'
c_dim='\033[2m'
c_off='\033[0m'

info() { printf "${c_info}::${c_off} %s\n" "$1"; }
ok()   { printf "${c_ok}✓${c_off} %s\n" "$1"; }
die()  { printf "${c_err}error:${c_off} %s\n" "$1" >&2; exit 1; }

info "Installing ${BIN} from ${REPO}"

# The file a PATH line has to go in for the next shell to see it.
profile_file() {
  case "${SHELL##*/}" in
    zsh) printf '%s' "${ZDOTDIR:-$HOME}/.zshrc" ;;
    bash) if [ -f "$HOME/.bash_profile" ]; then printf '%s' "$HOME/.bash_profile"; else printf '%s' "$HOME/.bashrc"; fi ;;
    fish) printf '%s' "$HOME/.config/fish/config.fish" ;;
    *) printf '%s' "$HOME/.profile" ;;
  esac
}

build_from_source() {
  if ! command -v cargo >/dev/null 2>&1; then
    printf "${c_err}error:${c_off} no prebuilt binary for this platform and cargo (Rust) is not installed.\n" >&2
    printf "\nInstall Rust, then re-run this command:\n" >&2
    printf "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh\n" >&2
    printf "  curl -fsSL https://withaster.dev/install | sh\n" >&2
    exit 1
  fi
  info "Building ${BIN} from source (this can take a few minutes)…"
  git_ref="https://github.com/${REPO}"
  cargo install --git "$git_ref" "$CRATE" --locked --force
}

# Resolve the platform triple used in release asset names.
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin) os_id="apple-darwin" ;;
  Linux)  os_id="unknown-linux-gnu" ;;
  *) info "Unsupported OS ($OS) for prebuilt binaries."; build_from_source; os_id="" ;;
esac

if [ -n "${os_id:-}" ]; then
  case "$ARCH" in
    x86_64|amd64) arch_id="x86_64" ;;
    arm64|aarch64) arch_id="aarch64" ;;
    *) info "Unsupported arch ($ARCH) for prebuilt binaries."; build_from_source; os_id="" ;;
  esac
fi

# If we already fell back to source, os_id is empty; skip the download path.
if [ -n "${os_id:-}" ]; then
  TARGET="${arch_id}-${os_id}"

  if [ "$VERSION" = "latest" ]; then
    info "Resolving latest release"
    TAG="$(curl -fsSL --connect-timeout 10 --retry 3 "${API}?per_page=100" | sed -n 's/.*"tag_name":[[:space:]]*"\(cli-v[^"]*\)".*/\1/p' | head -n1)"
  else
    case "$VERSION" in
      cli-v*) TAG="$VERSION" ;;
      v*) TAG="cli-${VERSION}" ;;
      *) TAG="cli-v${VERSION}" ;;
    esac
  fi

  if [ -z "${TAG:-}" ]; then
    info "No published release found; building from source instead."
    build_from_source
  else
    PLAIN_VERSION="${TAG#cli-v}"
    ASSET="aster-${PLAIN_VERSION}-${TARGET}.tar.gz"
    URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET}"

    TMP="$(mktemp -d)"
    trap 'rm -rf "$TMP"' EXIT

    info "Downloading ${ASSET}"
    if ! curl -fL --progress-bar --connect-timeout 10 --speed-limit 1024 --speed-time 30 --retry 3 "$URL" -o "$TMP/$ASSET"; then
      info "No prebuilt binary at ${URL}; building from source instead."
      build_from_source
    else
      if curl -fsSL "${URL}.sha256" -o "$TMP/$ASSET.sha256" 2>/dev/null; then
        EXPECTED="$(cat "$TMP/$ASSET.sha256")"
        if command -v shasum >/dev/null 2>&1; then
          ACTUAL="$(shasum -a 256 "$TMP/$ASSET" | awk '{print $1}')"
        else
          ACTUAL="$(sha256sum "$TMP/$ASSET" | awk '{print $1}')"
        fi
        [ "$EXPECTED" = "$ACTUAL" ] || die "checksum mismatch (expected $EXPECTED, got $ACTUAL)"
        ok "checksum verified"
      else
        info "no checksum published; skipping verification"
      fi

      tar -xzf "$TMP/$ASSET" -C "$TMP"
      STAGED="$TMP/aster-${PLAIN_VERSION}-${TARGET}/aster"
      [ -x "$STAGED" ] || die "binary not found in archive at expected path"

      if [ -w /usr/local/bin ] 2>/dev/null; then
        DEST_DIR="/usr/local/bin"
      else
        DEST_DIR="$HOME/.local/bin"
      fi
      mkdir -p "$DEST_DIR"

      SUDO=""
      if [ ! -w "$DEST_DIR" ] && [ "$(id -u)" != "0" ] && command -v sudo >/dev/null 2>&1; then
        SUDO="sudo"
      fi

      $SUDO install -m 0755 "$STAGED" "$DEST_DIR/aster"
      ok "Installed $DEST_DIR/aster"

      case ":$PATH:" in
        *":$DEST_DIR:"*) ;;
        *) printf "\n"; info "Add this to $(profile_file) so '${BIN}' is on PATH:"; printf '  export PATH="%s:$PATH"\n' "$DEST_DIR" ;;
      esac
    fi
  fi
fi

if command -v "$BIN" >/dev/null 2>&1; then
  ok "$("$BIN" --version 2>/dev/null || echo "${BIN} installed")"
else
  ok "${BIN} installed"
fi

printf "\nGet started:\n"
printf "  cd your-repo\n"
printf "  ${c_info}%s${c_off}\n" "$BIN"
printf "${c_dim}The first run asks how you want to connect: sign in with a browser,\n"
printf "paste an API key, or point at a model running on this machine.${c_off}\n"

printf "\n${c_dim}I build Aster alone, and I am job hunting. Hiring for systems\n"
printf "engineering or applied AI? chiziaruhoma@gmail.com${c_off}\n"
