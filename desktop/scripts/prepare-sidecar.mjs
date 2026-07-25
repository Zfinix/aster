import { execSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// Stages the `aster` CLI as a Tauri sidecar under
// desktop/src-tauri/binaries/aster-cli-<target-triple>[.exe] so packaged builds
// ship a self-contained CLI and never depend on `aster` being on the user's
// PATH. Runs from beforeBuildCommand / beforeDevCommand.
//
// ASTER_CLI_BIN, when set, points at an already-built binary (CI reuses the
// artifact from the release `build` job instead of recompiling). Otherwise the
// CLI is built from the root workspace, which is what local dev wants.

const here = dirname(fileURLToPath(import.meta.url));
const desktop = dirname(here);
const root = dirname(desktop);

const triple = execSync("rustc -vV", { encoding: "utf8" }).match(
  /host:\s*(\S+)/,
)?.[1];
if (!triple) {
  console.error("prepare-sidecar: could not determine host target triple");
  process.exit(1);
}
const ext = triple.includes("windows") ? ".exe" : "";

let src = process.env.ASTER_CLI_BIN;
if (src) {
  if (!existsSync(src)) {
    console.error(`prepare-sidecar: ASTER_CLI_BIN not found: ${src}`);
    process.exit(1);
  }
  console.log(`prepare-sidecar: using prebuilt ${src}`);
} else {
  execSync("cargo build --release -p aster-cli --bin aster", {
    cwd: root,
    stdio: "inherit",
  });
  src = join(root, "target", "release", `aster${ext}`);
}

const outDir = join(desktop, "src-tauri", "binaries");
mkdirSync(outDir, { recursive: true });
const dst = join(outDir, `aster-cli-${triple}${ext}`);
copyFileSync(src, dst);
if (!ext) chmodSync(dst, 0o755);
console.log(`prepare-sidecar: ${dst}`);
