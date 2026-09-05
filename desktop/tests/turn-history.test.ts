import { describe, expect, test } from "bun:test";
import { branchAtTurn } from "../src/lib/turn-history";
import type { Conversation } from "../src/lib/session";

const original: Conversation = {
  id: "original", title: "Thread", repoPath: "/repo", repoName: "repo",
  whenLabel: "earlier", sessionId: "saved",
  turns: [
    { id: "u1", role: "user", text: "First" },
    { id: "a1", role: "assistant", text: "Answer" },
    { id: "u2", role: "user", text: "Second" },
    { id: "a2", role: "assistant", text: "Later" },
  ],
};

describe("branchAtTurn", () => {
  test("edit supplies only prior history and clears the resumable session", () => {
    const edited = branchAtTurn(original, "u2", "edit", "unused")!;
    expect(edited.id).toBe(original.id);
    expect(edited.turns.map((t) => t.id)).toEqual(["u1", "a1"]);
    expect(edited.sessionId ?? null).toBeNull();
    expect(edited.repoPath).toBe("/repo");
  });
  test("editing the first message starts with empty history", () => {
    expect(branchAtTurn(original, "u1", "edit", "unused")!.turns).toEqual([]);
  });
  test("fork keeps the selected message and preserves the original", () => {
    const snapshot = structuredClone(original);
    const fork = branchAtTurn(original, "u2", "fork", "fork")!;
    expect(fork.id).toBe("fork");
    expect(fork.turns.map((t) => t.id)).toEqual(["u1", "a1", "u2"]);
    expect(fork.sessionId ?? null).toBeNull();
    expect(original).toEqual(snapshot);
    expect(fork.turns).not.toBe(original.turns);
  });
  test("rewind discards subsequent context without resuming it", () => {
    const rewound = branchAtTurn(original, "u1", "rewind", "unused")!;
    expect(rewound.id).toBe(original.id);
    expect(rewound.turns.map((t) => t.id)).toEqual(["u1"]);
    expect(rewound.sessionId ?? null).toBeNull();
    expect(original.turns).toHaveLength(4);
    expect(original.sessionId).toBe("saved");
  });
  test("unknown and non-user targets do nothing", () => {
    expect(branchAtTurn(original, "missing", "edit", "fork")).toBeNull();
    expect(branchAtTurn(original, "a1", "rewind", "fork")).toBeNull();
  });
});
