// Ori harness that runs aster instead of Ori's own agent loop, so an eval
// measures this harness rather than Ori's.
//
// `aster --stream` already emits NDJSON, one event per line, so this only has
// to translate that vocabulary into Ori's and forward it.
//
// The cases are generated from crates/aster-eval/src/live.rs; nothing here is
// edited by hand except the translation itself.

import { type AgentRuntimeEvent, defineHarness } from "ori";

interface HarnessOptions {
  readonly model?: string | null;
  readonly prompt: string;
  readonly systemPrompt?: string;
  readonly temperature?: number;
}

/** Checkout aster runs against; the driver passes the repo root. */
const REPO = process.env.ASTER_EVAL_REPO ?? process.cwd();
const BIN = process.env.ASTER_BIN ?? "aster";

async function* lines(stream: ReadableStream<Uint8Array>) {
  const decoder = new TextDecoder();
  let buffer = "";
  for await (const chunk of stream) {
    buffer += decoder.decode(chunk, { stream: true });
    let at: number;
    while ((at = buffer.indexOf("\n")) !== -1) {
      const line = buffer.slice(0, at).trim();
      buffer = buffer.slice(at + 1);
      if (line) yield line;
    }
  }
  if (buffer.trim()) yield buffer.trim();
}

/** aster's stream carries a message; Ori's failures are a classified record. */
const failure = (message: string) => ({
  code: "ORI_HARNESS_PROCESS_FAILED",
  kind: "unknown",
  message,
  stage: "harness",
}) as const;

/** aster sends tool arguments as a JSON string; Ori wants an object. */
const asObject = (raw: unknown) => {
  if (typeof raw !== "string") return raw ?? {};
  try {
    return JSON.parse(raw);
  } catch {
    return { raw };
  }
};

export const harness = defineHarness({
  name: "aster",
  init({ registerClose, registerPrompt }) {
    registerClose(() => Promise.resolve());
    registerPrompt(async function* (
      options: HarnessOptions,
    ): AsyncGenerator<AgentRuntimeEvent> {
      const args = ["--stream", "--permission-mode", "auto"];
      if (options.model) args.push("--model", options.model);
      args.push(options.prompt);

      const proc = Bun.spawn([BIN, ...args], {
        cwd: REPO,
        stdin: "ignore",
        stdout: "pipe",
        stderr: "pipe",
      });
      const sessionId = `aster-${proc.pid}`;

      yield { type: "session.started", payload: { sessionId } };
      yield { type: "turn.started", payload: { prompt: options.prompt } };

      // aster reports a tool's result separately from its call, so the name is
      // carried across by id to label the completion.
      const open = new Map<string, string>();
      let finished = false;

      for await (const line of lines(proc.stdout)) {
        let event: any;
        try {
          event = JSON.parse(line);
        } catch {
          continue;
        }
        switch (event.type) {
          case "token":
          case "text":
            if (event.content) {
              yield {
                type: "assistant.text.delta",
                payload: { delta: event.content },
              };
            }
            break;
          case "tool_call":
            open.set(event.id, event.name);
            yield {
              type: "tool.started",
              payload: {
                toolCallId: event.id,
                name: event.name,
                input: asObject(event.arguments),
              },
            };
            break;
          case "tool_result": {
            const name = open.get(event.id) ?? event.name ?? "unknown";
            open.delete(event.id);
            yield {
              type: event.error ? "tool.failed" : "tool.succeeded",
              payload: { toolCallId: event.id, name, result: event.result ?? "" },
            };
            break;
          }
          case "done": {
            // aster prices the turn itself; forwarding it is what lets an eval
            // compare models on cost rather than only on pass or fail.
            // Ori validates this payload and silently drops keys it does not
            // know, so the shape matches its own harnesses field for field.
            const u = event.usage ?? {};
            const usage = {
              cacheCreationTokens: 0,
              cacheReadTokens: 0,
              contextTokens: u.total_tokens ?? 0,
              costUsd: u.estimated_cost_usd ?? 0,
              inputTokens: u.prompt_tokens ?? 0,
              model: options.model ?? "",
              outputTokens: u.completion_tokens ?? 0,
            };
            // The reply already streamed as tokens; re-emitting would double it.
            yield { type: "turn.succeeded", payload: { usage } };
            yield { type: "session.succeeded", payload: { sessionId, usage } };
            finished = true;
            break;
          }
          case "error":
            yield {
              type: "turn.failed",
              payload: { failure: failure(event.message ?? "aster failed") },
            };
            yield {
              type: "session.failed",
              payload: { sessionId, failure: failure(event.message ?? "aster failed") },
            };
            finished = true;
            break;
        }
      }

      await proc.exited;
      if (!finished) {
        const stderr = (await new Response(proc.stderr).text()).trim();
        const message = stderr || `aster exited ${proc.exitCode} with no result`;
        yield { type: "turn.failed", payload: { failure: failure(message) } };
        yield {
          type: "session.failed",
          payload: { sessionId, failure: failure(message) },
        };
      }
    });
  },
});
