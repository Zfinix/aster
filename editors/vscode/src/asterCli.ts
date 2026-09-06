import { chmodSync, createWriteStream, existsSync } from "fs";
import { mkdir, rename, rm as rmDir, unlink } from "fs/promises";
import { get } from "https";
import type { IncomingMessage } from "http";
import { join } from "path";
import { spawn, execFile } from "child_process";
import * as vscode from "vscode";

export interface CliConfig {
  binary: string;
  minConfidence: number | null;
  extraArgs: string[];
}

/** The CLI this build ships with, when it ships one. */
let bundled: string | undefined;

/** A platform build carries the CLI beside the extension, so there is nothing
 *  to download and no network to get through. A generic build carries none,
 *  and falls back to the PATH and the install card. */
export function useBundledCli(extensionPath: string): string | undefined {
  const name = process.platform === "win32" ? "aster.exe" : "aster";
  const candidate = join(extensionPath, "bin", name);
  if (!existsSync(candidate)) return undefined;
  // A VSIX is a zip, and the executable bit does not survive every unpacking.
  if (process.platform !== "win32") {
    try {
      chmodSync(candidate, 0o755);
    } catch {
      // A read-only install is still worth trying to run.
    }
  }
  bundled = candidate;
  return candidate;
}

export function cliConfig(): CliConfig {
  const config = vscode.workspace.getConfiguration("aster");
  return {
    binary: config.get<string>("binaryPath")?.trim() || bundled || "aster",
    minConfidence: config.get<number | null>("minConfidence") ?? null,
    extraArgs: config.get<string[]>("extraArgs") ?? [],
  };
}

export function missingBinaryMessage(binary: string): string {
  return `aster binary not found at "${binary}". Install it with \`curl -fsSL https://withaster.dev/install | sh\` or set aster.binaryPath.`;
}

/** True when the configured binary can be executed. */
export async function checkBinary(binary: string): Promise<boolean> {
  return new Promise((resolve) => {
    const child = spawn(binary, ["--version"], { stdio: "ignore" });
    child.on("error", () => resolve(false));
    child.on("close", (code) => resolve(code === 0));
  });
}

export interface RunResult {
  stdout: string;
  stderr: string;
  code: number;
}

/** The environment every CLI child gets. A provider chosen in the panel travels
 *  as `ASTER_BASE_URL` rather than being written into the user's aster.yaml,
 *  so the override belongs to this panel the way its model and effort do. */
export function cliEnv(override?: ProviderOverride): NodeJS.ProcessEnv {
  if (!override) {
    return process.env;
  }
  const key = override.keyEnv.map((name) => process.env[name]).find((v) => v?.trim());
  return {
    ...process.env,
    ASTER_BASE_URL: override.baseUrl,
    ...(key ? { ASTER_API_KEY: key } : {}),
  };
}

export interface ProviderOverride {
  baseUrl: string;
  keyEnv: string[];
}

export interface LoginRun {
  done: Promise<number>;
  cancel: () => void;
}

/** Run `aster login <target>`, relaying every line it prints as it goes so
 *  the panel can show the browser flow's progress. */
export function runLogin(
  target: string,
  cwd: string,
  env: NodeJS.ProcessEnv,
  onLine: (line: string) => void
): LoginRun {
  const { binary } = cliConfig();
  const child = spawn(binary, ["login", target], { cwd, env, stdio: ["ignore", "pipe", "pipe"] });
  const relay = (chunk: string) => {
    for (const line of chunk.split("\n")) {
      if (line.trim()) onLine(line);
    }
  };
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", relay);
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", relay);
  const done = new Promise<number>((resolve, reject) => {
    child.on("error", (err: NodeJS.ErrnoException) => {
      reject(new Error(err.code === "ENOENT" ? missingBinaryMessage(binary) : String(err)));
    });
    child.on("close", (code) => resolve(code ?? 1));
  });
  return { done, cancel: () => child.kill() };
}

/** Run the CLI to completion, optionally writing `stdin` first. */
export function runCli(
  args: string[],
  cwd: string,
  stdin?: string,
  env: NodeJS.ProcessEnv = process.env
): Promise<RunResult> {
  const { binary } = cliConfig();
  return new Promise((resolve, reject) => {
    const child = spawn(binary, args, {
      cwd,
      env,
      stdio: ["pipe", "pipe", "pipe"],
    });

    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => (stdout += chunk));
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk: string) => (stderr += chunk));

    child.on("error", (err: NodeJS.ErrnoException) => {
      reject(new Error(err.code === "ENOENT" ? missingBinaryMessage(binary) : String(err)));
    });
    child.on("close", (code) => resolve({ stdout, stderr, code: code ?? 0 }));

    child.stdin.end(stdin ?? "");
  });
}

/** Determine the Rust target triple for the current platform. */
function detectTarget(): string {
  const arch = process.arch === "arm64" ? "aarch64" : "x86_64";
  const os =
    process.platform === "darwin" ? "apple-darwin" : process.platform === "linux" ? "unknown-linux-gnu" : "unknown-linux-gnu";
  return `${arch}-${os}`;
}

/** Resolve the latest CLI release tag from GitHub. */
const REPO = "zfinix/aster";

/** The official installer, the same line the docs give. */
export const INSTALL_CMD = "curl -fsSL https://withaster.dev/install | sh";

/** Long enough for a slow link, short enough that a blocked one says so rather
 *  than sitting on the OS default of over a minute. */
const NET_TIMEOUT_MS = 20000;

/** A request that gives up rather than hanging, and follows the redirect a
 *  release asset always answers with. */
function fetch(url: string, hops = 5): Promise<IncomingMessage> {
  return new Promise((resolve, reject) => {
    const request = get(url, { headers: { "User-Agent": "aster-vscode" } }, (res) => {
      const location = res.headers.location;
      if (res.statusCode && res.statusCode >= 300 && res.statusCode < 400 && location) {
        res.resume();
        if (hops === 0) {
          reject(new Error("too many redirects"));
          return;
        }
        fetch(new URL(location, url).toString(), hops - 1).then(resolve, reject);
        return;
      }
      if (res.statusCode !== 200) {
        res.resume();
        reject(new Error(`GitHub answered ${res.statusCode}`));
        return;
      }
      resolve(res);
    });
    request.setTimeout(NET_TIMEOUT_MS, () => {
      request.destroy(new Error("the connection timed out"));
    });
    request.on("error", (err: NodeJS.ErrnoException) => reject(new Error(network(err))));
  });
}

/** Timeouts and refusals are the common case behind a proxy or a firewall, and
 *  the raw address in them tells the reader nothing they can act on. */
function network(err: NodeJS.ErrnoException): string {
  switch (err.code) {
    case "ETIMEDOUT":
    case "ECONNRESET":
    case "EHOSTUNREACH":
    case "ENETUNREACH":
      return "could not reach github.com. Check your connection, VPN or proxy.";
    case "ENOTFOUND":
      return "could not look up github.com. Check your DNS or proxy.";
    case "ECONNREFUSED":
      return "github.com refused the connection. Check your proxy.";
    default:
      return err.message;
  }
}

async function resolveTag(repo: string): Promise<string> {
  const res = await fetch(`https://api.github.com/repos/${repo}/releases?per_page=100`);
  const body = await new Promise<string>((resolve, reject) => {
    let data = "";
    res.setEncoding("utf8");
    res.on("data", (chunk: string) => (data += chunk));
    res.on("end", () => resolve(data));
    res.on("error", reject);
  });
  const releases = JSON.parse(body) as { tag_name?: string }[];
  const tag = Array.isArray(releases)
    ? releases.find((r) => r.tag_name?.startsWith("cli-v"))?.tag_name
    : undefined;
  if (!tag) throw new Error("no CLI release found");
  return tag;
}

/** Download to a local path, reporting progress when the server says how big
 *  the file is. */
async function download(
  url: string,
  dest: string,
  onProgress: (msg: string) => void
): Promise<void> {
  const res = await fetch(url);
  const total = Number(res.headers["content-length"] ?? 0);
  let seen = 0;
  let shown = 0;
  return new Promise<void>((resolve, reject) => {
    const file = createWriteStream(dest);
    res.on("data", (chunk: Buffer) => {
      seen += chunk.length;
      const done = total ? Math.floor((seen / total) * 100) : 0;
      if (total && done >= shown + 20) {
        shown = done;
        onProgress(`Downloaded ${done}%`);
      }
    });
    res.on("error", (err: Error) => reject(err));
    file.on("error", (err: Error) => reject(err));
    file.on("finish", () => resolve());
    res.pipe(file);
  });
}

export async function installCli(
  storagePath: string,
  onProgress: (msg: string) => void
): Promise<string> {
  const target = detectTarget();
  onProgress(`Detected platform: ${target}`);

  onProgress("Looking up the latest release...");
  const tag = await resolveTag(REPO);
  const plain = tag.replace("cli-v", "");
  const asset = `aster-${plain}-${target}.tar.gz`;
  const url = `https://github.com/${REPO}/releases/download/${tag}/${asset}`;
  onProgress(`Downloading ${asset}...`);

  const tmpDir = join(storagePath, ".download");
  await mkdir(tmpDir, { recursive: true });
  const archive = join(tmpDir, asset);
  await download(url, archive, onProgress);

  onProgress("Extracting...");
  const extractDir = join(tmpDir, "extracted");
  await mkdir(extractDir, { recursive: true });
  await new Promise<void>((resolve, reject) => {
    execFile("tar", ["-xzf", archive, "-C", extractDir], (err) => {
      if (err) reject(new Error(`extraction failed: ${err.message}`));
      else resolve();
    });
  });

  // The archive unpacks to aster-{version}-{target}/aster
  const binaryName = process.platform === "win32" ? "aster.exe" : "aster";
  const staged = join(extractDir, `aster-${plain}-${target}`, binaryName);
  const dest = join(storagePath, binaryName);
  await rename(staged, dest);

  // Clean up temp files
  await unlink(archive).catch(() => {});
  await rm(extractDir, { recursive: true, force: true }).catch(() => {});
  await rm(tmpDir, { recursive: true, force: true }).catch(() => {});

  // Verify the binary works
  await new Promise<void>((resolve, reject) => {
    const child = spawn(dest, ["--version"], { stdio: "ignore" });
    child.on("error", () => reject(new Error("Installed binary failed to start")));
    child.on("close", (code) => code === 0 ? resolve() : reject(new Error(`Binary exit code ${code}`)));
  });

  // Update VS Code settings so checkBinary finds it
  const config = vscode.workspace.getConfiguration("aster");
  await config.update("binaryPath", dest, vscode.ConfigurationTarget.Global);

  onProgress(`Installed to ${dest}`);
  return dest;
}

function rm(path: string, opts: { recursive?: boolean; force?: boolean }): Promise<void> {
  return rmDir(path, opts).catch(() => {});
}
