import { describe, expect, it } from "vitest";
import { rank, type MenuSection } from "./CommandMenu";

const noop = () => {};

const sections: MenuSection[] = [
  {
    items: [
      { kind: "action", id: "new", label: "New conversation", slash: "/new", run: noop },
      {
        kind: "action",
        id: "compact",
        label: "Compact conversation",
        slash: "/compact",
        run: noop,
      },
    ],
  },
  {
    title: "Model",
    items: [
      { kind: "action", id: "model", label: "Switch model…", hint: "sonnet", run: noop },
      {
        kind: "choice",
        id: "effort",
        label: "Effort",
        value: "low",
        options: [{ value: "low", label: "Low" }],
        onSelect: noop,
      },
    ],
  },
  {
    title: "Skills",
    limit: 1,
    items: [
      {
        kind: "action",
        id: "skill:write-tests",
        label: "/write-tests",
        slash: "/write-tests",
        detail: "Add coverage",
        run: noop,
      },
      {
        kind: "action",
        id: "skill:write-like-chizi",
        label: "/write-like-chizi",
        slash: "/write-like-chizi",
        detail: "Blog posts in his voice",
        run: noop,
      },
    ],
  },
];

const labels = (result: MenuSection[]) => result.flatMap((s) => s.items.map((i) => i.label));

describe("rank", () => {
  it("keeps every section when nothing is typed", () => {
    expect(labels(rank(sections, "  "))).toEqual([
      "New conversation",
      "Compact conversation",
      "Switch model…",
      "Effort",
      "/write-tests",
    ]);
  });

  it("cuts a section to its limit and says what it left out", () => {
    expect(rank(sections, "")[2].note).toBe("1 more; type to filter");
  });

  it("searches past the limit, so a name always reaches its row", () => {
    expect(labels(rank(sections, "write-like"))).toEqual(["/write-like-chizi"]);
  });

  it("drops the truncation note once a query is on", () => {
    expect(rank(sections, "write")[0].note).toBeUndefined();
  });

  it("matches labels regardless of case", () => {
    expect(labels(rank(sections, "COMPACT"))).toEqual(["Compact conversation"]);
  });

  it("ignores the slash the composer is still holding", () => {
    expect(labels(rank(sections, "/compact"))).toEqual(["Compact conversation"]);
  });

  it("keeps a whole section when its title matches, controls included", () => {
    expect(labels(rank(sections, "model"))).toEqual(["Switch model…", "Effort"]);
  });

  it("matches a skill by its description, not just its name", () => {
    expect(labels(rank(sections, "coverage"))).toEqual(["/write-tests"]);
  });

  it("puts the section holding the best match first", () => {
    expect(rank(sections, "write").map((s) => s.title)).toEqual(["Skills"]);
  });

  it("takes initials, so a long skill name is a few keystrokes", () => {
    expect(labels(rank(sections, "wlc"))).toEqual(["/write-like-chizi"]);
  });

  it("drops sections with nothing left", () => {
    expect(rank(sections, "effort").map((s) => s.title)).toEqual(["Model"]);
  });

  it("returns nothing when no label matches", () => {
    expect(rank(sections, "zzz")).toEqual([]);
  });
});
