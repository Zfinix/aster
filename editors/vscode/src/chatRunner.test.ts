import { EventEmitter } from "events";
import { PassThrough } from "stream";
import { beforeEach, describe, expect, it, vi } from "vitest";

const spawn = vi.fn();
vi.mock("child_process", () => ({ spawn: (...args: unknown[]) => spawn(...args) }));
vi.mock("./asterCli", () => ({
  cliConfig: () => ({ binary: "aster" }),
  missingBinaryMessage: (binary: string) => `aster binary not found at "${binary}".`,
}));

import { ChatRunner } from "./chatRunner";
import { ChatStreamEvent } from "./protocol";

class FakeAgent extends EventEmitter {
  exitCode: number | null = null;
  stdout = new PassThrough();
  stderr = new PassThrough();
  stdin = new PassThrough();
  kill = vi.fn();
  promptError: string | null = null;
  /** A crashing agent never answers the prompt it died on. */
  silentPrompt = false;
  prompts: string[][] = [];
  methods: string[] = [];

  constructor() {
    super();
    this.stdin.on("data", (chunk: Buffer) => {
      for (const line of chunk.toString().split("\n").filter(Boolean)) {
        const message = JSON.parse(line);
        if (message.id === undefined) continue;
        this.methods.push(message.method);
        if (message.method === "session/prompt") {
          this.prompts.push(
            (message.params.prompt as { text: string }[]).map((part) => part.text)
          );
        }
        if (message.method === "session/prompt" && this.silentPrompt) continue;
        const failing = message.method === "session/prompt" && this.promptError;
        const body = failing
          ? { error: { code: -32603, message: this.promptError } }
          : { result: { sessionId: "s1" } };
        this.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", id: message.id, ...body })}\n`);
      }
    });
  }

  crash(said: string, code: number): void {
    this.stderr.write(`${said}\n`);
    setImmediate(() => {
      this.exitCode = code;
      this.emit("close", code, null);
    });
  }
}

function options(
  onEvent: (event: ChatStreamEvent) => void,
  messages: { role: "user" | "assistant"; content: string }[] = [
    { role: "user", content: "hi" },
  ],
  session: string | null = null
) {
  return {
    messages,
    cwd: "/repo",
    model: null,
    permissionMode: "ask" as const,
    effort: null,
    env: {},
    session,
    onEvent,
    onStderr: () => undefined,
  };
}

describe("ChatRunner", () => {
  beforeEach(() => spawn.mockReset());

  it("shows the reason the agent gave for failing a turn", async () => {
    const agent = new FakeAgent();
    agent.promptError = "model endpoint returned 429: rate limited";
    spawn.mockReturnValue(agent);
    const events: ChatStreamEvent[] = [];
    const runner = new ChatRunner();

    expect(await runner.run(options((event) => events.push(event)))).toBe(1);
    const error = events.find((e) => e.type === "error");
    expect((error as { message: string }).message).toBe(
      "model endpoint returned 429: rate limited"
    );
  });

  it("reports the agent's last words when it dies mid-turn", async () => {
    const agent = new FakeAgent();
    agent.silentPrompt = true;
    spawn.mockReturnValue(agent);
    const events: ChatStreamEvent[] = [];
    const runner = new ChatRunner();

    const done = runner.run(options((event) => events.push(event)));
    await vi.waitFor(() => expect(spawn).toHaveBeenCalled());
    agent.crash("aster: failed to open the session store", 101);

    expect(await done).toBe(1);
    const error = events.find((e) => e.type === "error");
    expect(error).toBeDefined();
    expect((error as { message: string }).message).toContain(
      "failed to open the session store"
    );
    expect((error as { message: string }).message).toContain("code 101");
  });

  it("still sends a message once the capped history stops growing", async () => {
    const agent = new FakeAgent();
    spawn.mockReturnValue(agent);
    const runner = new ChatRunner();
    // The webview sends a fixed-length window of the transcript, so past the
    // cap every turn arrives the same length as the one before it, sliding
    // forward by the reply and the new message.
    const transcript: { role: "user" | "assistant"; content: string }[] = [];
    const window = (text: string) => {
      transcript.push({ role: "user", content: text });
      const sent = transcript.slice(-12);
      transcript.push({ role: "assistant", content: `re: ${text}` });
      return sent;
    };
    for (let i = 0; i < 8; i++) {
      await runner.run(options(() => undefined, window(`turn ${i}`), "s1"));
    }

    expect(agent.prompts).toEqual(
      Array.from({ length: 8 }, (_, i) => [`turn ${i}`])
    );
    // One bind at the start; a sliding window is not a reason to rebuild.
    expect(agent.methods.filter((m) => m === "session/load")).toHaveLength(1);
  });

  it("resyncs the session when the transcript no longer lines up", async () => {
    const agent = new FakeAgent();
    spawn.mockReturnValue(agent);
    const runner = new ChatRunner();

    await runner.run(
      options(
        () => undefined,
        [
          { role: "user", content: "one" },
          { role: "assistant", content: "reply" },
          { role: "user", content: "two" },
        ],
        "s1"
      )
    );
    // An edited message rewrites the history the agent already has.
    await runner.run(
      options(
        () => undefined,
        [
          { role: "user", content: "one, but different" },
          { role: "assistant", content: "reply" },
          { role: "user", content: "three" },
        ],
        "s1"
      )
    );

    expect(agent.methods.filter((m) => m === "session/load")).toHaveLength(2);
    expect(agent.prompts.at(-1)).toEqual(["three"]);
  });
});
