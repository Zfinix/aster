import { describe, expect, it } from "vitest";
import type { MemoryBlock } from "../../src/protocol";
import { blockWhen, countFacts, filterBlocks, plural } from "./memory";

const block = (over: Partial<MemoryBlock> = {}): MemoryBlock => ({
  name: "release-process",
  description: "tag cli-vX.Y.Z to ship",
  path: "/m/release-process.md",
  source_session: null,
  created_at: null,
  updated_at: null,
  ...over,
});

describe("filterBlocks", () => {
  it("matches a name or its description, case-insensitively", () => {
    const blocks = [block(), block({ name: "tone", description: "prefers terse replies" })];
    expect(filterBlocks(blocks, "TERSE").map((b) => b.name)).toEqual(["tone"]);
    expect(filterBlocks(blocks, "release").map((b) => b.name)).toEqual(["release-process"]);
    expect(filterBlocks(blocks, "  ")).toHaveLength(2);
  });
});

describe("blockWhen", () => {
  it("dates a block by its last write, falling back to when it was saved", () => {
    const created = new Date(Date.now() - 3 * 86400_000).toISOString();
    const updated = new Date(Date.now() - 60_000).toISOString();
    expect(blockWhen(block({ created_at: created, updated_at: updated }))).toBe("1m");
    expect(blockWhen(block({ created_at: created }))).toBe("3d");
    expect(blockWhen(block())).toBe("");
  });
});

describe("countFacts", () => {
  it("counts the bullets in project memory and nothing else", () => {
    const text = "# Project memory\n\n- one\n- two\nnot a fact\n";
    expect(countFacts({ path: "/m/ASTER.md", text })).toBe(2);
    expect(countFacts(null)).toBe(0);
  });
});

describe("plural", () => {
  it("keeps the noun singular for one", () => {
    expect(plural(1, "block")).toBe("1 block");
    expect(plural(0, "fact")).toBe("0 facts");
  });
});
