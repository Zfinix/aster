import { EventEmitter } from "events";
import { PassThrough } from "stream";
import { beforeEach, describe, expect, it, vi } from "vitest";

const spawn = vi.fn();
vi.mock("child_process", () => ({ spawn: (...args: unknown[]) => spawn(...args) }));
vi.mock("./asterCli", () => ({
  cliConfig: () => ({ binary: "aster", minConfidence: null, extraArgs: [] }),
  missingBinaryMessage: (binary: string) => `aster binary not found at "${binary}".`,
}));

import { ReviewRunner } from "./reviewRunner";

class FakeCli extends EventEmitter {
  stdout = new PassThrough();
  stderr = new PassThrough();
  kill = vi.fn();

  fail(said: string, code: number): void {
    this.stderr.write(`${said}\n`);
    setImmediate(() => this.emit("close", code, null));
  }
}

function options() {
  return {
    cwd: "/repo",
    source: { kind: "working" } as const,
    env: {},
    onEvent: () => undefined,
    onStderr: () => undefined,
  };
}

describe("ReviewRunner", () => {
  beforeEach(() => spawn.mockReset());

  it("quotes the CLI's last words when a review dies", async () => {
    const cli = new FakeCli();
    spawn.mockReturnValue(cli);
    const runner = new ReviewRunner();

    const done = runner.run(options());
    await vi.waitFor(() => expect(spawn).toHaveBeenCalled());
    cli.fail("error: model endpoint returned 429: rate limited", 1);

    expect(await done).toBe(1);
    expect(runner.crashMessage(1)).toContain("rate limited");
  });

  it("falls back to the output channel when the CLI says nothing", async () => {
    const cli = new FakeCli();
    spawn.mockReturnValue(cli);
    const runner = new ReviewRunner();

    const done = runner.run(options());
    await vi.waitFor(() => expect(spawn).toHaveBeenCalled());
    setImmediate(() => cli.emit("close", 1, null));

    expect(await done).toBe(1);
    expect(runner.crashMessage(1)).toContain("See the Aster output channel");
  });
});