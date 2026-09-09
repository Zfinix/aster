import { describe, expect, test } from "bun:test";
import { findingKey, matchFile } from "../src/lib/match";
import { effortShort, EFFORT_OPTIONS } from "../src/lib/effort";
import { severityOf } from "../src/lib/severity";
import type { Finding } from "../src/lib/types";

const at = (file_path: string): Finding => ({ file_path, line: 7, title: "Leak" }) as Finding;

describe("matchFile", () => {
  test("an exact path matches", () => {
    expect(matchFile(at("src/lib/diff.ts"), "src/lib/diff.ts")).toBe(true);
  });

  test("a finding reported relative to a subdirectory still matches", () => {
    expect(matchFile(at("lib/diff.ts"), "src/lib/diff.ts")).toBe(true);
  });

  test("a finding with the longer path matches the shorter diff key", () => {
    expect(matchFile(at("desktop/src/lib/diff.ts"), "src/lib/diff.ts")).toBe(true);
  });

  test("an unrelated file does not match", () => {
    expect(matchFile(at("src/lib/diff.ts"), "src/lib/model.ts")).toBe(false);
  });
});

describe("findingKey", () => {
  test("combines path, line and title so two findings on a line stay distinct", () => {
    expect(findingKey(at("src/lib/diff.ts"))).toBe("src/lib/diff.ts:7:Leak");
  });
});

describe("severityOf", () => {
  test("passes a known severity through", () => {
    expect(severityOf("critical")).toBe("critical");
  });

  test("falls back to info rather than trusting the string", () => {
    expect(severityOf("catastrophic")).toBe("info");
    expect(severityOf("")).toBe("info");
  });
});

describe("effortShort", () => {
  test("medium is shortened so the chip stays one word", () => {
    expect(effortShort("medium")).toBe("Med");
  });

  test("an unset effort reads as the default", () => {
    expect(effortShort(null)).toBe("Default");
    expect(effortShort("")).toBe("Default");
  });

  test("an unknown effort is capitalised rather than dropped", () => {
    expect(effortShort("blistering")).toBe("Blistering");
  });

  test("the menu offers the default first, then the whole ladder", () => {
    expect(EFFORT_OPTIONS[0]).toEqual({ value: "", label: "Default" });
    expect(EFFORT_OPTIONS.map((o) => o.label)).toEqual([
      "Default",
      "Off",
      "Low",
      "Med",
      "High",
      "XHigh",
      "Max",
      "Ultra",
    ]);
  });
});
