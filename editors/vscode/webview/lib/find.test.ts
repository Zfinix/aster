import { describe, expect, it } from "vitest";
import { matchOffsets } from "./find";

describe("matchOffsets", () => {
  it("finds every case-insensitive hit", () => {
    expect(matchOffsets("Panel panel PANEL", "panel")).toEqual([0, 6, 12]);
  });

  it("does not overlap hits", () => {
    expect(matchOffsets("aaaa", "aa")).toEqual([0, 2]);
  });

  it("returns nothing for an empty query", () => {
    expect(matchOffsets("anything", "")).toEqual([]);
  });

  it("falls back to an exact match when folding case would shift offsets", () => {
    expect(matchOffsets("İstanbul istanbul", "istanbul")).toEqual([9]);
  });
});
