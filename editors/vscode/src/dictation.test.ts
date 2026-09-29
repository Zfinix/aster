import { EventEmitter } from "events";
import { PassThrough } from "stream";
import { beforeEach, describe, expect, it, vi } from "vitest";

const spawn = vi.fn();
vi.mock("child_process", () => ({ spawn: (...args: unknown[]) => spawn(...args) }));
vi.mock("./asterCli", () => ({
  cliConfig: () => ({ binary: "aster" }),
  missingBinaryMessage: (binary: string) => `aster binary not found at "${binary}".`,
}));

import { Dictation } from "./dictation";
import { DictationEvent } from "./protocol";

class FakeDictate extends EventEmitter {
  stdout = new PassThrough();
  stdin = new PassThrough();
  kill = vi.fn(() => this.emit("close", null, "SIGTERM"));
  written = "";

  constructor() {
    super();
    this.stdin.on("data", (chunk: Buffer) => (this.written += chunk.toString()));
  }
}

const flush = () => new Promise((resolve) => setImmediate(resolve));

describe("Dictation", () => {
  let child: FakeDictate;
  let events: DictationEvent[];
  let dictation: Dictation;

  beforeEach(() => {
    child = new FakeDictate();
    spawn.mockReset().mockReturnValue(child);
    events = [];
    dictation = new Dictation((event) => events.push(event));
  });

  it("relays lines split across chunks and asks the CLI to stop with a newline", async () => {
    dictation.start("/repo", {});
    child.stdout.write('{"type":"listen');
    child.stdout.write('ing"}\n');
    dictation.stop();
    child.stdout.write('{"type":"transcribing"}\n{"type":"transcript","text":"hello there"}\n');
    child.emit("close", 0, null);
    await flush();

    expect(spawn).toHaveBeenCalledWith("aster", ["dictate"], {
      cwd: "/repo",
      env: {},
      stdio: ["pipe", "pipe", "ignore"],
    });
    expect(child.written).toBe("\n");
    expect(events).toEqual([
      { type: "listening" },
      { type: "transcribing" },
      { type: "transcript", text: "hello there" },
    ]);
  });

  it("reports a CLI that exits without a result", async () => {
    dictation.start(undefined, {});
    child.stdout.write('{"type":"listening"}\n');
    await flush();
    child.emit("close", 101, null);

    expect(events).toEqual([
      { type: "listening" },
      { type: "error", message: "Recording stopped unexpectedly. Try again.", detail: null },
    ]);
  });

  it("stays quiet after a cancel", async () => {
    dictation.start(undefined, {});
    child.stdout.write('{"type":"listening"}\n');
    await flush();
    dictation.cancel();
    child.stdout.write('{"type":"transcript","text":"late"}\n');
    await flush();

    expect(child.kill).toHaveBeenCalled();
    expect(events).toEqual([{ type: "listening" }]);
  });
});
