import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ContextMeter } from "./ContextMeter";

const noop = () => {};
const html = (used: number, budget: number) =>
  renderToStaticMarkup(<ContextMeter used={used} budget={budget} onCompact={noop} />);

describe("ContextMeter", () => {
  it("says nothing when the CLI could not name a budget", () => {
    expect(html(50_000, 0)).toBe("");
  });

  it("stays out of the way until the conversation has spent something", () => {
    expect(html(1_000, 100_000)).toBe("");
  });

  it("reports usage in plain words on the hover card", () => {
    const out = html(25_000, 100_000);
    expect(out).toContain("25% used");
    expect(out).toContain("Conversation space");
    expect(out).toContain("folds older messages");
  });

  it("warns once the window is nearly gone", () => {
    expect(html(80_000, 100_000)).toContain('data-low="true"');
    expect(html(50_000, 100_000)).toContain('data-low="false"');
  });

  // A conversation can exceed the budget before the next turn compacts it, and
  // a ring reading past full or a negative percentage would be nonsense.
  it("clamps a conversation already over budget", () => {
    const out = html(200_000, 100_000);
    expect(out).toContain("100% used");
    expect(out).toContain('stroke-dashoffset="0"');
  });
});
