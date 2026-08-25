import { describe, expect, it } from "vitest";
import { parseError } from "./error";

describe("parseError", () => {
  it("labels a known status code in plain language and keeps the detail", () => {
    const { label, hint, detail } = parseError(
      "bad request (400): Reasoning is mandatory for this endpoint and cannot be disabled."
    );
    expect(label).toBe("The request didn't go through");
    expect(hint).toContain("rejected");
    expect(detail).toBe("Reasoning is mandatory for this endpoint and cannot be disabled.");
  });

  it("tells the user what to do when rate limited", () => {
    const { label, hint } = parseError("rate limited (429): slow down");
    expect(label).toBe("Too many requests");
    expect(hint).toContain("slow down");
  });

  it("labels an unknown status code with its number", () => {
    const { label } = parseError("request failed (599): upstream hiccup");
    expect(label).toBe("Error 599");
  });

  it("passes through a message that is not a status error", () => {
    const { label, detail } = parseError("aster exited with an error");
    expect(label).toBe("Something went wrong");
    expect(detail).toBe("aster exited with an error");
  });

  it("explains a mid-stream drop in plain words", () => {
    const { label, hint, detail } = parseError(
      "reading stream chunk: error decoding response body: request or response body error: operation timed out"
    );
    expect(label).toBe("Connection dropped");
    expect(hint).toContain("Send again");
    expect(detail).toContain("operation timed out");
  });
});
