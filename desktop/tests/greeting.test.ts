import { describe, expect, test } from "bun:test";
import { itemAt, parts, TIPS } from "../src/lib/greeting";

describe("parts", () => {
  test("splits a tip into prose and chips", () => {
    expect(parts("[@] picks a file from the repo")).toEqual([
      { text: "@", chip: true },
      { text: " picks a file from the repo", chip: false },
    ]);
  });

  test("handles several chips in one tip", () => {
    expect(parts("[shift] [enter] adds a newline").filter((p) => p.chip)).toEqual([
      { text: "shift", chip: true },
      { text: "enter", chip: true },
    ]);
  });

  test("a tip with no brackets is one prose part", () => {
    expect(parts("Light and dark live in Settings")).toEqual([
      { text: "Light and dark live in Settings", chip: false },
    ]);
  });

  test("every shipped tip round-trips to its original text", () => {
    for (const tip of TIPS) {
      const rebuilt = parts(tip)
        .map((p) => (p.chip ? `[${p.text}]` : p.text))
        .join("");
      expect(rebuilt).toBe(tip);
    }
  });
});

describe("itemAt", () => {
  const items = ["a", "b", "c"];

  test("indexes within range", () => {
    expect(itemAt(items, 1)).toBe("b");
  });

  test("wraps past the end", () => {
    expect(itemAt(items, 4)).toBe("b");
  });

  test("wraps below zero rather than returning undefined", () => {
    expect(itemAt(items, -1)).toBe("c");
    expect(itemAt(items, -4)).toBe("c");
  });
});
