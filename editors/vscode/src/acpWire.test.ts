import { describe, expect, it } from "vitest";
import { contentText, editedPath, toolResult } from "./acpWire";

// The frames below are copied from a live `aster acp` turn, where a tool
// call's fields sit next to its id rather than under a `fields` key.
describe("contentText", () => {
  it("reads a bare content block", () => {
    expect(contentText({ type: "text", text: "hi" })).toBe("hi");
  });

  it("unwraps the tool call content envelope a permission request carries", () => {
    const content = [{ type: "content", content: { type: "text", text: "run `echo hi`" } }];
    expect(contentText(content)).toBe("run `echo hi`");
  });

  it("falls back to a diff's new text", () => {
    expect(contentText([{ type: "diff", path: "a.txt", newText: "goodbye" }])).toBe("goodbye");
  });

  it("is empty for content it cannot read", () => {
    expect(contentText(undefined)).toBe("");
    expect(contentText([{ type: "terminal", terminalId: "t1" }])).toBe("");
  });
});

describe("toolResult", () => {
  it("closes a completed call with its raw output", () => {
    expect(
      toolResult({
        toolCallId: "call_1",
        status: "completed",
        rawOutput: "stdout:\nhi\n\nexit code: 0",
      })
    ).toEqual({ id: "call_1", result: "stdout:\nhi\n\nexit code: 0", error: false });
  });

  it("marks a failed call", () => {
    expect(toolResult({ toolCallId: "call_1", status: "failed", rawOutput: "boom" })?.error).toBe(
      true
    );
  });

  it("ignores an update that only reports progress", () => {
    expect(toolResult({ toolCallId: "call_1", status: "in_progress" })).toBeNull();
    expect(toolResult({ toolCallId: "call_1" })).toBeNull();
  });

  it("falls back to the content blocks when there is no raw output", () => {
    expect(
      toolResult({
        toolCallId: "call_1",
        status: "completed",
        content: [{ type: "content", content: { type: "text", text: "done" } }],
      })?.result
    ).toBe("done");
  });
});

describe("editedPath", () => {
  it("names the file an edit touched", () => {
    expect(editedPath({ name: "edit_file", rawInput: { path: "src/a.ts" } })).toBe("src/a.ts");
  });

  it("ignores other tools", () => {
    expect(editedPath({ name: "run_command", rawInput: { command: "echo" } })).toBeNull();
    expect(editedPath({ name: "edit_file", rawInput: {} })).toBeNull();
  });
});
