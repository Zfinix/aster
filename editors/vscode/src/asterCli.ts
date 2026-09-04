import { spawn } from "child_process";
import * as vscode from "vscode";

export interface CliConfig {
  binary: string;
  minConfidence: number | null;
  extraArgs: string[];
}

export function cliConfig(): CliConfig {
  const config = vscode.workspace.getConfiguration("aster");
  return {
    binary: config.get<string>("binaryPath") || "aster",
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

/**
 * The environment every CLI child gets. A provider chosen in the panel travels
 * as `ASTER_BASE_URL` rather than being written into the user's aster.yaml,
 * so the override belongs to this panel the way its model and effort do.
 */
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
  /** Env vars holding this endpoint's own key, from `aster models --providers`. */
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
