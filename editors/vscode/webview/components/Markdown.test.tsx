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

describe("Markdown links", () => {
  it("makes a bare URL clickable", () => {
    const out = html("Opened http://localhost:5173/pricing in your browser.");
    expect(out).toContain('href="http://localhost:5173/pricing"');
    expect(out).toContain("md-link");
  });

  it("leaves the sentence's punctuation out of the URL", () => {
    const out = html("It is at http://localhost:5173.");
    expect(out).toContain('href="http://localhost:5173"');
    expect(out).toContain(".</p>");
  });

  it("links a file URL, which is what a built page usually is", () => {
    const out = html("file:///tmp/site/index.html");
    expect(out).toContain('href="file:///tmp/site/index.html"');
  });

  it("does not link a URL twice when it is already a markdown link", () => {
    const out = html("[the page](http://localhost:5173)");
    expect(out.match(/<a /g)).toHaveLength(1);
    expect(out).toContain(">the page</a>");
  });

  it("leaves a URL inside a code span alone", () => {
    const out = html("Run `curl http://localhost:5173`");
    expect(out).not.toContain("<a ");
  });
});

describe("Markdown task lists", () => {
  it("draws a checkbox instead of literal brackets", () => {
    const out = html("- [ ] Stage 1: the auth crate\n- [x] Stage 2: the adapter");
    expect(out).not.toContain("[ ]");
    expect(out).not.toContain("[x]");
    expect(out).toContain("☐");
    expect(out).toContain("☑");
    expect(out).toContain("Stage 1: the auth crate");
  });

  it("marks the list so the bullet does not double up with the box", () => {
    expect(html("- [ ] one")).toContain('data-tasks="true"');
  });

  it("leaves a plain bullet list alone", () => {
    const out = html("- one\n- two");
    expect(out).not.toContain("☐");
    expect(out).not.toContain("data-tasks");
  });
});
