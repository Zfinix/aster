import { describe, expect, it } from "vitest";
import { parseError } from "./error";

describe("parseError", () => {
  it("labels a known status code and keeps the detail", () => {
    const { label, detail } = parseError(
      "bad request (400): Reasoning is mandatory for this endpoint and cannot be disabled."
    );
    expect(label).toBe("Bad request");
    expect(detail).toBe("Reasoning is mandatory for this endpoint and cannot be disabled.");
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
});
