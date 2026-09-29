import { ChildProcess, spawn } from "child_process";
import { cliConfig, missingBinaryMessage } from "./asterCli";
import type { DictationEvent } from "./protocol";

const STOPPED = "Recording stopped unexpectedly. Try again.";

/** Runs `aster dictate` for one webview: start listens, stop has the
 *  recording transcribed, cancel throws it away. */
export class Dictation {
  private child: ChildProcess | undefined;

  constructor(private readonly onEvent: (event: DictationEvent) => void) {}

  start(cwd: string | undefined, env: NodeJS.ProcessEnv): void {
    this.cancel();
    const binary = cliConfig().binary;
    const child = spawn(binary, ["dictate"], { cwd, env, stdio: ["pipe", "pipe", "ignore"] });
    this.child = child;
    let buffer = "";
    let settled = false;
    child.stdout?.setEncoding("utf8");
    child.stdout?.on("data", (chunk: string) => {
      buffer += chunk;
      const lines = buffer.split("\n");
      buffer = lines.pop() ?? "";
      for (const line of lines) {
        let event: DictationEvent;
        try {
          event = JSON.parse(line) as DictationEvent;
        } catch {
          continue;
        }
        if (event.type === "transcript" || event.type === "error") settled = true;
        if (this.child === child) this.onEvent(event);
      }
    });
    child.on("error", (err: NodeJS.ErrnoException) => {
      if (this.child !== child) return;
      settled = true;
      this.child = undefined;
      this.onEvent({
        type: "error",
        message: "Couldn't start Aster to listen. Check that the Aster CLI is installed, then try again.",
        detail: err.code === "ENOENT" ? missingBinaryMessage(binary) : String(err),
      });
    });
    child.on("close", () => {
      if (this.child !== child) return;
      this.child = undefined;
      if (!settled) this.onEvent({ type: "error", message: STOPPED, detail: null });
    });
  }

  stop(): void {
    this.child?.stdin?.end("\n");
  }

  cancel(): void {
    const child = this.child;
    this.child = undefined;
    child?.kill();
  }
}
