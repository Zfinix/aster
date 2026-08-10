import { describe, expect, it } from "vitest";
import { looksLikeDiff } from "./DiffView";

const lines = (text: string) => text.split("\n");

describe("looksLikeDiff", () => {
  it("recognises a unified hunk", () => {
    expect(looksLikeDiff(lines("--- a/x.rs\n+++ b/x.rs\n@@ -1,2 +1,2 @@\n context"))).toBe(true);
  });

  it("recognises the bare form the editor's own previews use", () => {
    expect(looksLikeDiff(lines("- let a = 1;\n+ let a = 2;"))).toBe(true);
  });

  it("leaves a search result full of hyphens alone", () => {
    expect(looksLikeDiff(lines("-- comment\n-- another\nplain line"))).toBe(false);
  });

  it("leaves a one-sided list alone: a change has two sides", () => {
    expect(looksLikeDiff(lines("- one\n- two\n- three"))).toBe(false);
  });

  it("leaves plain output alone", () => {
    expect(looksLikeDiff(lines("Compiling aster v0.1.0\nFinished in 4s"))).toBe(false);
  });
});
