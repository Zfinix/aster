import { describe, expect, it } from "vitest";
import {
  appendCall,
  appendReasoning,
  appendText,
  buildMessages,
  hydrate,
  newTurn,
  patchCall,
  restoreTurn,
  stopUnfinished,
  upsertAgentState,
  type AssistantTurn,
  type ToolCall,
  type Turn,
} from "./thread";

const call = (id: string, name = "read_file"): ToolCall => ({
  id,
  name,
  arguments: JSON.stringify({ path: `${id}.rs` }),
});

const shape = (turn: AssistantTurn) =>
  turn.blocks.map((block) => {
    if (block.kind === "text") return `text:${block.text}`;
    if (block.kind === "injected") return `injected:${block.text}`;
    if (block.kind === "agents") return `agents:${block.tasks.map((t) => t.agent).join(",")}`;
    if (block.kind === "reasoning") return `reasoning:${block.text}`;
    if (block.kind === "goal") return `goal:${block.condition}`;
    return `tools:${block.calls.map((c) => c.id).join(",")}`;
  });

describe("appendText", () => {
  it("merges consecutive chunks into one block", () => {
    let turn = appendText(newTurn("t1"), "Hello");
    turn = appendText(turn, " there");
    expect(shape(turn)).toEqual(["text:Hello there"]);
    expect(turn.text).toBe("Hello there");
  });

  it("ignores an empty chunk", () => {
    expect(appendText(newTurn("t1"), "").blocks).toEqual([]);
  });

  it("keeps the separator out of a block it opens", () => {
    let turn = appendText(newTurn("t1"), "First");
    turn = appendCall(turn, call("c1"));
    turn = appendText(turn, "Second", "\n\n");
    expect(shape(turn)).toEqual(["text:First", "tools:c1", "text:Second"]);
    expect(turn.text).toBe("First\n\nSecond");
  });
});

describe("appendCall", () => {
  it("groups calls that arrive back to back", () => {
    let turn = appendCall(newTurn("t1"), call("c1"));
    turn = appendCall(turn, call("c2"));
    expect(shape(turn)).toEqual(["tools:c1,c2"]);
  });

  it("opens a new group after text, so steps follow the thought", () => {
    let turn = appendCall(newTurn("t1"), call("c1"));
    turn = appendText(turn, "Now I have the picture.");
    turn = appendCall(turn, call("c2"));
    expect(shape(turn)).toEqual(["tools:c1", "text:Now I have the picture.", "tools:c2"]);
  });

  it("leaves the flattened text alone", () => {
    const turn = appendCall(appendText(newTurn("t1"), "Reply"), call("c1"));
    expect(turn.text).toBe("Reply");
  });
});

describe("patchCall", () => {
  it("attaches a result to the matching call only", () => {
    let turn = appendCall(newTurn("t1"), call("c1"));
    turn = appendCall(turn, call("c2"));
    turn = patchCall(turn, "c2", { result: "ok", error: false });

    const block = turn.blocks[0];
    if (block.kind !== "tools") throw new Error("expected a tool group");
    expect(block.calls[0].result).toBeUndefined();
    expect(block.calls[1].result).toBe("ok");
  });

  it("reaches a call in an earlier group", () => {
    let turn = appendCall(newTurn("t1"), call("c1"));
    turn = appendText(turn, "thinking");
    turn = appendCall(turn, call("c2"));
    turn = patchCall(turn, "c1", { result: "done" });

    const first = turn.blocks[0];
    if (first.kind !== "tools") throw new Error("expected a tool group");
    expect(first.calls[0].result).toBe("done");
  });

  it("is a no-op for an unknown id", () => {
    const turn = appendCall(newTurn("t1"), call("c1"));
    expect(patchCall(turn, "nope", { result: "x" })).toEqual(turn);
  });
});

describe("stopUnfinished", () => {
  it("marks an in-flight tool call stopped so it stops spinning", () => {
    let turn = appendCall(newTurn("t1"), call("c1"));
    turn = appendCall(turn, call("c2"));
    turn = patchCall(turn, "c1", { result: "done" });

    const stopped = stopUnfinished([turn])[0] as AssistantTurn;
    const block = stopped.blocks[0];
    if (block.kind !== "tools") throw new Error("expected a tool group");
    expect(block.calls[0].stopped).toBeUndefined();
    expect(block.calls[1].stopped).toBe(true);
  });

  it("leaves a finished turn's calls alone", () => {
    let turn = appendCall(newTurn("t1"), call("c1"));
    turn = patchCall(turn, "c1", { result: "done" });
    turn = { ...turn, pending: false };

    const stopped = stopUnfinished([turn])[0] as AssistantTurn;
    const block = stopped.blocks[0];
    if (block.kind !== "tools") throw new Error("expected a tool group");
    expect(block.calls[0].stopped).toBeUndefined();
  });
});

describe("upsertAgentState", () => {
  it("opens an agents block on the first event for a call", () => {
    const turn = upsertAgentState(newTurn("t1"), {
      callId: "c1",
      agent: "explorer",
      status: "running",
      done: 0,
      total: 1,
    });
    expect(shape(turn)).toEqual(["agents:explorer"]);
  });

  it("updates the matching row in place and adds a second agent", () => {
    let turn = upsertAgentState(newTurn("t1"), {
      callId: "c1",
      agent: "explorer",
      status: "running",
      done: 0,
      total: 2,
    });
    turn = upsertAgentState(turn, {
      callId: "c1",
      agent: "explorer",
      status: "done",
      report: "Found it",
      done: 1,
      total: 2,
    });
    turn = upsertAgentState(turn, {
      callId: "c1",
      agent: "reviewer",
      status: "running",
      done: 1,
      total: 2,
    });
    expect(shape(turn)).toEqual(["agents:explorer,reviewer"]);
    const block = turn.blocks[0];
    if (block.kind !== "agents") throw new Error("expected an agents block");
    expect(block.tasks[0].report).toBe("Found it");
  });

  it("keeps a second call's swarm in its own block", () => {
    let turn = upsertAgentState(newTurn("t1"), {
      callId: "c1",
      agent: "explorer",
      status: "done",
      done: 1,
      total: 1,
    });
    turn = upsertAgentState(turn, {
      callId: "c2",
      agent: "synthesizer",
      status: "running",
      done: 0,
      total: 1,
    });
    expect(shape(turn)).toEqual(["agents:explorer", "agents:synthesizer"]);
  });
});

describe("hydrate", () => {
  it("folds a pre-timeline turn into blocks, steps first", () => {
    const legacy = [
      { id: "t1", role: "user", text: "hi" },
      { id: "t2", role: "assistant", text: "Answer", tools: [call("c1")] },
    ] as unknown as Turn[];

    const turn = hydrate(legacy)[1] as AssistantTurn;
    expect(shape(turn)).toEqual(["tools:c1", "text:Answer"]);
  });

  it("drops empty halves rather than emitting blank blocks", () => {
    const legacy = [{ id: "t1", role: "assistant", text: "", tools: [] }] as unknown as Turn[];
    expect((hydrate(legacy)[0] as AssistantTurn).blocks).toEqual([]);
  });

  it("leaves an already-migrated turn untouched", () => {
    const turn = appendText(newTurn("t1"), "Answer");
    expect(hydrate([turn])[0]).toBe(turn);
  });

  it("handles no saved state", () => {
    expect(hydrate(undefined)).toEqual([]);
  });
});

describe("appendReasoning", () => {
  it("keeps thinking as its own block in arrival order", () => {
    let turn = appendText(newTurn("t1"), "before");
    turn = appendReasoning(turn, "weighing the options");
    turn = appendCall(turn, call("c1"));
    expect(shape(turn)).toEqual(["text:before", "reasoning:weighing the options", "tools:c1"]);
  });

  it("never opens a block for reasoning that is only whitespace", () => {
    const turn = appendReasoning(newTurn("t1"), "   \n  ");
    expect(turn.blocks).toEqual([]);
  });

  it("does not fold consecutive rounds into one block", () => {
    let turn = appendReasoning(newTurn("t1"), "first");
    turn = appendReasoning(turn, "second");
    expect(shape(turn)).toEqual(["reasoning:first", "reasoning:second"]);
  });
});

describe("restoreTurn", () => {
  it("rebuilds thinking, reply, and steps in the order they happened", () => {
    const turn = restoreTurn(
      "t1",
      "Renamed the label.",
      { text: "checking both call sites", tokens: 40, durationMs: 1200 },
      [call("c1"), call("c2")]
    );
    expect(shape(turn)).toEqual([
      "reasoning:checking both call sites",
      "text:Renamed the label.",
      "tools:c1,c2",
    ]);
    expect(turn.pending).toBe(false);
  });

  it("keeps the token count and duration so the block is not relabelled on reload", () => {
    const turn = restoreTurn("t1", "done", { text: "thought", tokens: 40, durationMs: 1200 });
    const block = turn.blocks.find((b) => b.kind === "reasoning");
    expect(block).toMatchObject({ tokens: 40, durationMs: 1200, done: true });
  });

  it("restores a round that was tool calls with no commentary", () => {
    const turn = restoreTurn("t1", "", undefined, [call("c1")]);
    expect(shape(turn)).toEqual(["tools:c1"]);
  });

  it("flattens the reply into text, which chat history and copy read", () => {
    const turn = restoreTurn("t1", "Renamed the label.", { text: "thinking" }, [call("c1")]);
    expect(turn.text).toBe("Renamed the label.");
  });

  it("leaves a turn with nothing to show empty rather than drawing a blank block", () => {
    expect(restoreTurn("t1", "").blocks).toEqual([]);
  });
});

describe("buildMessages across a compaction", () => {
  const said = (id: string, text: string): Turn => ({
    ...appendText(newTurn(id), text),
    pending: false,
  });
  const asked = (id: string, text: string): Turn => ({ id, role: "user", text });
  const seam: Turn = {
    id: "s1",
    role: "compaction",
    summary: "we renamed the label",
    folded: 4,
    messages: [
      { role: "assistant", content: "Summary of earlier conversation:\nwe renamed the label" },
      { role: "user", content: "kept verbatim" },
    ],
  };

  it("sends the folded history in place of everything above the seam", () => {
    expect(
      buildMessages([asked("u1", "dropped"), said("a1", "dropped too"), seam, asked("u2", "next")])
    ).toEqual([...seam.messages, { role: "user", content: "next" }]);
  });

  it("keeps the folded history whole when the limit trims the turns after it", () => {
    const after = Array.from({ length: 20 }, (_, i) => asked(`u${i}`, `m${i}`));
    const messages = buildMessages([asked("u0", "dropped"), seam, ...after], 3);
    expect(messages.slice(0, seam.messages.length)).toEqual(seam.messages);
    expect(messages.slice(seam.messages.length)).toHaveLength(3);
  });

  it("folds from the newest seam, so a second compaction supersedes the first", () => {
    const later: Turn = { ...seam, id: "s2", messages: [{ role: "assistant", content: "later" }] };
    expect(buildMessages([seam, asked("u1", "middle"), later, asked("u2", "after")])).toEqual([
      { role: "assistant", content: "later" },
      { role: "user", content: "after" },
    ]);
  });

  it("is unchanged when nothing has been compacted", () => {
    expect(buildMessages([asked("u1", "hi"), said("a1", "hello")])).toEqual([
      { role: "user", content: "hi" },
      { role: "assistant", content: "hello" },
    ]);
  });
});
