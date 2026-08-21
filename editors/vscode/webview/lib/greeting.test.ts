import { afterEach, describe, expect, it, vi } from "vitest";
import { itemAt, OPENERS, parts, TIPS } from "./greeting";

const prose = (tip: string) => tip.replace(/[[\]]/g, "");

describe("itemAt", () => {
  it("returns the plain index inside the list", () => {
    expect(itemAt(["a", "b", "c"], 1)).toBe("b");
  });

  it("wraps past the end, so a rotation runs forever", () => {
    expect(itemAt(["a", "b", "c"], 4)).toBe("b");
  });

  it("wraps below zero rather than reading off the front", () => {
    expect(itemAt(["a", "b", "c"], -1)).toBe("c");
  });
});

describe("parts", () => {
  it("leaves a tip without brackets as one run of prose", () => {
    expect(parts("drop a file on the composer")).toEqual([
      { text: "drop a file on the composer", chip: false },
    ]);
  });

  it("chips a bracketed run and keeps the prose around it", () => {
    expect(parts("[/diff] shows changes")).toEqual([
      { text: "/diff", chip: true },
      { text: " shows changes", chip: false },
    ]);
  });

  it("chips each key of a chord, keeping the space that separates them", () => {
    expect(parts("[shift] [enter]")).toEqual([
      { text: "shift", chip: true },
      { text: " ", chip: false },
      { text: "enter", chip: true },
    ]);
  });

  it("rebuilds the tip, so nothing is dropped on the way through", () => {
    for (const tip of TIPS) {
      expect(parts(tip).map((p) => p.text).join("")).toBe(prose(tip));
    }
  });
});

describe("chords", () => {
  const tipsFor = async (userAgent: string) => {
    vi.resetModules();
    vi.stubGlobal("navigator", { userAgent });
    return (await import("./greeting")).TIPS.join("\n");
  };

  afterEach(() => vi.unstubAllGlobals());

  it("draws Mac modifiers as the glyphs printed on the keycaps", async () => {
    const tips = await tipsFor("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)");
    expect(tips).toContain("[⌥] [a]");
    expect(tips).toContain("[⌘] [⌥] [k]");
    expect(tips).toContain("[⇧] [enter]");
    expect(tips).not.toMatch(/\[(alt|cmd|shift)\]/);
  });

  it("spells them out where the keycaps carry words", async () => {
    const tips = await tipsFor("Mozilla/5.0 (Windows NT 10.0; Win64; x64)");
    expect(tips).toContain("[alt] [a]");
    expect(tips).toContain("[ctrl] [alt] [k]");
    expect(tips).toContain("[shift] [enter]");
  });
});

describe("pools", () => {
  it("carries enough of each to not repeat within a sitting", () => {
    expect(OPENERS.length).toBeGreaterThan(4);
    expect(TIPS.length).toBeGreaterThan(8);
  });

  it("holds no duplicates, which would read as a stuck rotation", () => {
    expect(new Set(OPENERS).size).toBe(OPENERS.length);
    expect(new Set(TIPS).size).toBe(TIPS.length);
  });

  it("keeps every tip short enough for one line in a narrow panel", () => {
    for (const tip of TIPS) expect(prose(tip).length).toBeLessThanOrEqual(40);
  });

  it("closes every chip it opens", () => {
    for (const tip of TIPS) {
      expect((tip.match(/\[/g) ?? []).length).toBe((tip.match(/\]/g) ?? []).length);
    }
  });
});
