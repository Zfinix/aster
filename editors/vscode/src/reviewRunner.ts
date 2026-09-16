import { ChildProcess, spawn } from "child_process";
import { cliConfig, missingBinaryMessage } from "./asterCli";
import { ReviewSource } from "./protocol";
import { StreamEvent } from "./types";

export interface ReviewOptions {
  cwd: string;
  source: ReviewSource;
  env: NodeJS.ProcessEnv;
  onEvent: (event: StreamEvent) => void;
  onStderr: (line: string) => void;
}

function sourceArgs(source: ReviewSource): string[] {
  switch (source.kind) {
    case "range":
      return ["--range", source.value];
    case "pr":
      return ["--pr", source.value];
    case "working":
      return [];
  }
}

/** Owns at most one `aster review --stream` child and splits its NDJSON stdout. */
export class ReviewRunner {
  private child: ChildProcess | undefined;

  /** The CLI's last words. A failure reports its reason through a dead pipe,
   *  so the text only exists on stderr. */
  private lastStderr: string[] = [];

  get running(): boolean {
    return this.child !== undefined;
  }

  run(options: ReviewOptions): Promise<number> {
    if (this.child) {
      throw new Error("a review is already running");
    }

    const { binary, minConfidence, extraArgs } = cliConfig();
    const args = ["review", "--stream", ...sourceArgs(options.source), ...extraArgs];
    if (minConfidence !== null) {
      args.push("--min-confidence", String(minConfidence));
    }

    const child = spawn(binary, args, {
      cwd: options.cwd,
      env: options.env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    this.child = child;

    let buffer = "";
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => {
      buffer += chunk;
      let newline;
      while ((newline = buffer.indexOf("\n")) !== -1) {
        const line = buffer.slice(0, newline).trim();
        buffer = buffer.slice(newline + 1);
        if (!line) {
          continue;
        }
        try {
          options.onEvent(JSON.parse(line) as StreamEvent);
        } catch {
          options.onStderr(line);
        }
      }
    });

    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk: string) => {
      for (const line of chunk.split("\n")) {
        if (line.trim()) {
          this.rememberStderr(line);
          options.onStderr(line);
        }
      }
    });

    // Only clear the slot if it still holds *this* child: a cancel may already
    // have released it and started a new run.
    const release = () => {
      if (this.child === child) {
        this.child = undefined;
      }
    };

    return new Promise((resolve, reject) => {
      child.on("error", (err: NodeJS.ErrnoException) => {
        release();
        reject(new Error(err.code === "ENOENT" ? missingBinaryMessage(binary) : String(err)));
      });
      child.on("close", (code) => {
        release();
        resolve(code ?? 0);
      });
    });
  }

  private rememberStderr(line: string): void {
    this.lastStderr.push(line);
    if (this.lastStderr.length > 8) {
      this.lastStderr.shift();
    }
  }

  /** Quotes what the CLI printed before it died, so the panel explains the
   *  failure instead of pointing at the output channel. */
  crashMessage(code: number | null): string {
    const said = this.lastStderr.join("\n").trim();
    this.lastStderr = [];
    return said
      ? `aster exited with code ${code ?? 1}: ${said}`
      : `aster exited with code ${code ?? 1}. See the Aster output channel.`;
  }

  cancel(): void {
    const child = this.child;
    if (!child) {
      return;
    }
    this.child = undefined;
    child.kill("SIGTERM");
    const forceKill = setTimeout(() => child.kill("SIGKILL"), 2000);
    child.on("close", () => clearTimeout(forceKill));
  }
}
