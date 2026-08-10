import { describe, expect, it } from "vitest";
import { filter, type MenuSection } from "./CommandMenu";

const noop = () => {};

const sections: MenuSection[] = [
  {
    items: [
      { kind: "action", id: "new", label: "New conversation", run: noop },
      { kind: "action", id: "compact", label: "Compact conversation", run: noop },
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
    items: [
      { kind: "action", id: "s1", label: "write-tests", detail: "Add coverage", run: noop },
    ],
    note: "3 more; type to filter",
  },
];

const labels = (result: MenuSection[]) => result.flatMap((s) => s.items.map((i) => i.label));

describe("filter", () => {
  it("keeps every section when nothing is typed", () => {
    expect(labels(filter(sections, "  "))).toHaveLength(5);
  });

  it("matches labels regardless of case", () => {
    expect(labels(filter(sections, "COMPACT"))).toEqual(["Compact conversation"]);
  });

  it("keeps a whole section when its title matches, controls included", () => {
    expect(labels(filter(sections, "model"))).toEqual(["Switch model…", "Effort"]);
  });

  it("matches a skill by its description, not just its name", () => {
    expect(labels(filter(sections, "coverage"))).toEqual(["write-tests"]);
  });

  it("drops sections with nothing left", () => {
    expect(filter(sections, "effort").map((s) => s.title)).toEqual(["Model"]);
  });

  // The note counts what the unfiltered list omitted, so it would be a lie
  // sitting under a filtered one.
  it("drops the truncation note once a query is on", () => {
    expect(filter(sections, "write")[0].note).toBeUndefined();
  });

  it("returns nothing when no label matches", () => {
    expect(filter(sections, "zzz")).toEqual([]);
  });
});
