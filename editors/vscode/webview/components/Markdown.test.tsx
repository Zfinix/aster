import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { Markdown } from "./Markdown";

const html = (text: string) => renderToStaticMarkup(<Markdown text={text} />);

const TABLE = `| Command | What it does |
| --- | --- |
| /status | Shows the status |`;

describe("Markdown tables", () => {
  it("renders a whole table as a table", () => {
    const out = html(TABLE);
    expect(out).toContain("<table");
    expect(out).toContain("<th>Command</th>");
    expect(out).toContain("<td>/status</td>");
  });

  /**
   * The states a table passes through while it streams in. Each one used to
   * match TABLE_ROW without matching the table branch, so no branch advanced
   * the cursor and the renderer looped, pushing an empty <p> until it died.
   */
  it.each([
    ["header only", "| Command | What it does |"],
    ["header and a partial separator", "| Command | What it does |\n| ---"],
    ["header and a whole separator", "| Command | What it does |\n| --- | --- |"],
    ["a row mid-cell", "| Command | What it does |\n| --- | --- |\n| /status | Shows"],
    ["prose then a bare row", "Here is the table:\n\n| Command | What it does |"],
  ])("terminates on a table that is still streaming: %s", (_name, text) => {
    expect(() => html(text)).not.toThrow();
  });

  it("keeps a row with no separator as prose rather than dropping it", () => {
    expect(html("| Command | What it does |")).toContain("Command");
  });

  // The header row is a block start, so the paragraph must stop before it
  // rather than swallowing the table that follows.
  it("does not absorb a following table into the paragraph above it", () => {
    const out = html(`Here is the table:\n${TABLE}`);
    expect(out).toContain("<table");
    expect(out).toContain("<th>Command</th>");
  });
});
