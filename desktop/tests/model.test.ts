import { describe, expect, test } from "bun:test";
import { modelChip, modelProvider, modelShort } from "../src/lib/model";

describe("modelShort", () => {
  test("drops the provider and title-cases the slug", () => {
    expect(modelShort("google/gemini-3.1-flash-lite")).toBe("Gemini 3.1 Flash Lite");
  });

  test("a missing id reads as the default", () => {
    expect(modelShort(null)).toBe("Default");
  });

  test("an all-consonant word is an acronym", () => {
    expect(modelShort("gpt-5")).toBe("GPT 5");
  });

  test("a short token with a digit is upper-cased", () => {
    expect(modelShort("o3-mini")).toBe("O3 Mini");
  });

  test("a bare version keeps its lowercase v", () => {
    expect(modelShort("qwen-V3")).toBe("Qwen v3");
  });

  test("a fireworks p version reads as a point release", () => {
    expect(modelShort("fireworks/glm-5p3-flash-low")).toBe("GLM 5.3 Flash Low");
  });
});

describe("modelChip", () => {
  test("drops the claude family prefix and joins split version digits", () => {
    expect(modelChip("claude-fable-5-1")).toBe("Fable 5.1");
  });

  test("drops a trailing date stamp", () => {
    expect(modelChip("claude-haiku-4-5-20251001")).toBe("Haiku 4.5");
  });

  test("claude elsewhere in the id is kept", () => {
    expect(modelChip("anthropic/claude-opus-5")).toBe("Opus 5");
  });

  test("a non-claude id is left alone", () => {
    expect(modelChip("google/gemini-3.1-flash")).toBe("Gemini 3.1 Flash");
  });

  test("a missing id reads as the default", () => {
    expect(modelChip(null)).toBe("Default");
  });
});

describe("modelProvider", () => {
  test("returns the segment before the slash", () => {
    expect(modelProvider("google/gemini-3.1")).toBe("google");
  });

  test("an unqualified id has no provider", () => {
    expect(modelProvider("gpt-5")).toBe("");
  });
});
