import { describe, expect, it } from "vitest";
import { modelChip, modelShort } from "./model";

describe("modelChip", () => {
  it("drops the family prefix and joins split version digits", () => {
    expect(modelChip("claude-fable-5-1")).toBe("Fable 5.1");
    expect(modelChip("anthropic/claude-opus-5")).toBe("Opus 5");
  });

  it("drops a trailing date stamp", () => {
    expect(modelChip("claude-haiku-4-5-20251001")).toBe("Haiku 4.5");
  });

  it("leaves other vendors' names alone", () => {
    expect(modelChip("google/gemini-3.1-flash-lite")).toBe("Gemini 3.1 Flash Lite");
    expect(modelChip("gpt-5-mini")).toBe("GPT 5 Mini");
  });

  it("falls back like modelShort when there is nothing to pick", () => {
    expect(modelChip(null)).toBe("Default");
    expect(modelShort("claude-fable-5-1")).toBe("Claude Fable 5 1");
  });
});
