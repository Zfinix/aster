import { describe, expect, test } from "bun:test";
import { applyTrigger, dropTrigger, triggersAt } from "../src/lib/trigger";

describe("triggersAt", () => {
  test("an @ token at the caret opens the file list", () => {
    const { mention, command } = triggersAt("look at @src/li", 15);
    expect(mention).toEqual({ query: "src/li", start: 8, end: 15 });
    expect(command).toBeNull();
  });

  test("the caret on the sigil itself counts as inside", () => {
    expect(triggersAt("look at @", 8)?.mention).toEqual({ query: "", start: 8, end: 9 });
  });

  test("a caret past the token closes it", () => {
    expect(triggersAt("@file done", 10).mention).toBeNull();
  });

  test("an @ mid-word is not a trigger", () => {
    expect(triggersAt("chizi@example.com", 17).mention).toBeNull();
  });

  test("a leading / opens the command menu", () => {
    expect(triggersAt("/mod", 4).command).toEqual({ query: "mod", start: 0, end: 4 });
  });

  test("a path is not a command", () => {
    expect(triggersAt("/usr/bin", 8).command).toBeNull();
  });

  test("the token holding the caret wins when the text has several", () => {
    expect(triggersAt("@one @two", 9).mention?.query).toBe("two");
  });
});

describe("applyTrigger", () => {
  test("replaces the token and leaves one trailing space", () => {
    const trigger = triggersAt("see @sr", 7).mention!;
    expect(applyTrigger("see @sr", trigger, "@src/lib/diff.ts")).toBe("see @src/lib/diff.ts ");
  });

  test("does not double the space when one already follows", () => {
    const trigger = triggersAt("see @sr rest", 7).mention!;
    expect(applyTrigger("see @sr rest", trigger, "@src/x.ts")).toBe("see @src/x.ts rest");
  });
});

describe("dropTrigger", () => {
  test("removes the token and keeps the surrounding text", () => {
    const trigger = triggersAt("run /model now", 10).command!;
    // The span starts at the sigil, so the space that preceded it survives.
    expect(dropTrigger("run /model now", trigger)).toBe("run  now");
  });

  test("a command at the start leaves the rest of the line", () => {
    const trigger = triggersAt("/model gpt", 6).command!;
    expect(dropTrigger("/model gpt", trigger)).toBe(" gpt");
  });
});
