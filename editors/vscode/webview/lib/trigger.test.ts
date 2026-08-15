import { describe, expect, it } from "vitest";
import { applyTrigger, dropTrigger, triggersAt } from "./trigger";

const command = (text: string, caret = text.length) => triggersAt(text, caret).command;
const mention = (text: string, caret = text.length) => triggersAt(text, caret).mention;

describe("triggersAt", () => {
  it("opens the menu on a bare slash", () => {
    expect(command("/")).toEqual({ query: "", start: 0, end: 1 });
  });

  it("carries what has been typed after it", () => {
    expect(command("/comp")?.query).toBe("comp");
  });

  it("takes a slash mid-sentence, since a command can follow a thought", () => {
    expect(command("do this /rev")).toEqual({ query: "rev", start: 8, end: 12 });
  });

  it("ignores a slash inside a word, so a path is not a command", () => {
    expect(command("src/lib")).toBeNull();
  });

  it("stops at a second slash, so a typed path closes the menu", () => {
    expect(command("/src/lib")).toBeNull();
  });

  it("ends at the space, so the menu closes once the name is done", () => {
    expect(command("/compact ")).toBeNull();
  });

  it("reopens when the caret moves back onto the name", () => {
    expect(command("/compact now", 5)?.query).toBe("compact");
  });

  it("finds a mention the same way", () => {
    expect(mention("look at @src/lib")).toEqual({ query: "src/lib", start: 8, end: 16 });
  });

  it("never reads one token as both", () => {
    expect(mention("/compact")).toBeNull();
  });
});

describe("applyTrigger", () => {
  it("replaces the token and leaves a space to type into", () => {
    const trigger = command("/comp")!;
    expect(applyTrigger("/comp", trigger, "/compact")).toBe("/compact ");
  });

  it("keeps what follows, and does not double its space", () => {
    const trigger = command("/comp now", 5)!;
    expect(applyTrigger("/comp now", trigger, "/compact")).toBe("/compact now");
  });

  it("leaves the words before it alone", () => {
    const trigger = command("please /rev")!;
    expect(applyTrigger("please /rev", trigger, "/review")).toBe("please /review ");
  });
});

describe("dropTrigger", () => {
  it("takes the name back out for a command that runs", () => {
    const trigger = command("/compact")!;
    expect(dropTrigger("/compact", trigger)).toBe("");
  });

  it("leaves the argument behind", () => {
    const trigger = command("/write-tests the parser", 12)!;
    expect(dropTrigger("/write-tests the parser", trigger)).toBe(" the parser");
  });
});
