#!/usr/bin/env node
// Puts the CLI for one platform in `bin/`, so `vsce package --target` ships an
// extension that needs no download. Takes the binary from a local build, from
// $ASTER_CLI_BIN, or from the matching GitHub release.
//
//   node scripts/bundle-cli.mjs <vsce-target> [--release <tag>]
//
// Targets follow vsce: darwin-arm64, darwin-x64, linux-x64, linux-arm64,
// win32-x64.

import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const RUST_TARGET = {
  "darwin-arm64": "aarch64-apple-darwin",
  "darwin-x64": "x86_64-apple-darwin",
  "linux-x64": "x86_64-unknown-linux-gnu",
  "linux-arm64": "aarch64-unknown-linux-gnu",
  "win32-x64": "x86_64-pc-windows-msvc",
};

const REPO = process.env.ASTER_REPO ?? "zfinix/aster";
const here = dirname(fileURLToPath(import.meta.url));
const extensionDir = dirname(here);
const repoRoot = join(extensionDir, "..", "..");

const target = process.argv[2];
const rust = RUST_TARGET[target];
if (!rust) {
  console.error(`usage: bundle-cli.mjs <${Object.keys(RUST_TARGET).join("|")}>`);
  process.exit(1);
}

const at = process.argv.indexOf("--release");
const tag = at === -1 ? undefined : process.argv[at + 1];
const exe = target.startsWith("win32") ? "aster.exe" : "aster";
const binDir = join(extensionDir, "bin");
const dest = join(binDir, exe);

rmSync(binDir, { recursive: true, force: true });
mkdirSync(binDir, { recursive: true });

const source = tag ? fromRelease(tag) : fromLocalBuild();
copyFileSync(source, dest);
if (!target.startsWith("win32")) chmodSync(dest, 0o755);
console.log(`bundled ${rust} -> ${dest}`);

function fromLocalBuild() {
  const candidates = [
    process.env.ASTER_CLI_BIN,
    join(repoRoot, "target", rust, "release", exe),
    join(repoRoot, "target", "release", exe),
  ].filter(Boolean);
  const found = candidates.find((path) => existsSync(path));
  if (!found) {
    console.error(
      `no CLI found for ${rust}. Build it with\n` +
        `  cargo build --release -p aster-cli --bin aster --target ${rust}\n` +
        `or pass --release <tag> to take it from GitHub, or set ASTER_CLI_BIN.`
    );
    process.exit(1);
  }
  return found;
}

function fromRelease(tag) {
  const version = tag.replace(/^cli-v/, "");
  const archive = target.startsWith("win32")
    ? `aster-${version}-${rust}.zip`
    : `aster-${version}-${rust}.tar.gz`;
  const url = `https://github.com/${REPO}/releases/download/${tag}/${archive}`;
  const work = mkdtempSync(join(tmpdir(), "aster-cli-"));
  const local = join(work, archive);
  // curl and tar rather than a bundled HTTP client: they honour the proxy and
  // certificate settings of whoever is running the build.
  execFileSync("curl", ["-fsSL", "--retry", "3", "-o", local, url], { stdio: "inherit" });
  if (archive.endsWith(".zip")) {
    execFileSync("unzip", ["-q", "-o", local, "-d", work], { stdio: "inherit" });
  } else {
    execFileSync("tar", ["-xzf", local, "-C", work], { stdio: "inherit" });
  }
  return join(work, `aster-${version}-${rust}`, exe);
}
