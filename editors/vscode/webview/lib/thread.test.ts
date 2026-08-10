import { describe, expect, it } from "vitest";
import {
  appendCall,
  appendText,
  hydrate,
  newTurn,
  patchCall,
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
