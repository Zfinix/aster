import { ChildProcess, spawn } from "child_process";
import { cliConfig, missingBinaryMessage } from "./asterCli";
import { ChatMessage, ChatStreamEvent, Effort, PermissionMode } from "./protocol";

export interface ChatOptions {
  messages: ChatMessage[];
  cwd: string;
  model: string | null;
  permissionMode: PermissionMode;
  effort: Effort | null;
  env: NodeJS.ProcessEnv;
  session: string | null;
  onEvent: (event: ChatStreamEvent) => void;
  onStderr: (line: string) => void;
}

/** Owns at most one `aster chat --stream` child. The messages go in as a single
 *  stdin line and stdin then stays open, because that is the channel the CLI
 *  reads approval replies from. */
export class ChatRunner {
  private child: ChildProcess | undefined;

  get running(): boolean {
    return this.child !== undefined;
  }

  run(options: ChatOptions): Promise<number> {
    if (this.child) {
      throw new Error("a chat turn is already running");
    }

    const { binary } = cliConfig();
    const args = [
      "chat",
      "--messages-json",
      "-",
      "--stream",
      "--permission-mode",
      options.permissionMode,
    ];
    // Only when the user picked one: the flag outranks ASTER_EFFORT and
    // `aster.yaml`, so sending a default would silently override the repo's.
    if (options.effort) {
      args.push("--effort", options.effort);
    }
    if (options.model) {
      args.push("--model", options.model);
    }
    if (options.session) {
      args.push("--session", options.session);
    }

    const child = spawn(binary, args, {
      cwd: options.cwd,
      env: options.env,
      stdio: ["pipe", "pipe", "pipe"],
    });
    this.child = child;

    child.stdin.write(`${JSON.stringify(options.messages)}\n`);

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
          options.onEvent(JSON.parse(line) as ChatStreamEvent);
        } catch {
          options.onStderr(line);
        }
      }
    });

    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk: string) => {
      for (const line of chunk.split("\n")) {
        if (line.trim()) {
          options.onStderr(line);
        }
      }
    });

    // Only clear the slot if it still holds *this* child: a cancel may already
    // have released it and started a new turn.
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

  approve(allow: boolean): void {
    this.child?.stdin?.write(`${JSON.stringify({ allow })}\n`);
  }

  answer(choice: string | null): void {
    this.child?.stdin?.write(`${JSON.stringify({ choice })}\n`);
  }

  inject(text: string): void {
    this.child?.stdin?.write(`${JSON.stringify({ message: text })}\n`);
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
