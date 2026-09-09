import { describe, expect, test } from "bun:test";
import { parseError } from "../src/lib/error";

describe("parseError", () => {
  test("a prefixed status maps to its explanation", () => {
    const parsed = parseError("api error(429): slow down");
    expect(parsed.label).toBe("Too many requests");
    expect(parsed.hint).toContain("Nothing was lost");
    expect(parsed.detail).toBe("slow down");
  });

  test("an unknown status still names the code", () => {
    const parsed = parseError("api error(418): teapot");
    expect(parsed.label).toBe("Error 418");
    expect(parsed.hint).toBeUndefined();
  });

  test("an empty detail falls back to the whole message", () => {
    expect(parseError("api error(500): ").detail).toBe("api error(500): ");
  });

  test("a missing key reads as not signed in, not as an auth failure", () => {
    expect(parseError("No API key found for provider").label).toBe("Not signed in");
  });

  test("a dropped stream wins over a status quoted from an earlier retry", () => {
    const parsed = parseError("stream chunk failed after a 503 from the provider");
    expect(parsed.label).toBe("Connection dropped");
  });

  test("a DNS failure reads as unreachable rather than a dropped stream", () => {
    expect(parseError("dns error: failed to lookup address").label).toBe("Can't reach the provider");
  });

  test("a status embedded in prose is recognised", () => {
    expect(parseError("provider returned status 401 for this model").label).toBe("Sign-in problem");
  });

  test("an exit-code wrapper without a cause blames the process, not the request", () => {
    const parsed = parseError("aster chat exited with code 1: something odd");
    expect(parsed.label).toBe("Aster stopped unexpectedly");
    expect(parsed.detail).toBe("something odd");
  });

  test("a wrapper whose only detail points at the log drops the detail", () => {
    const parsed = parseError("aster exited with code 1: See the Aster output channel.");
    expect(parsed.label).toBe("Aster stopped unexpectedly");
    expect(parsed.detail).toBe("");
  });

  test("an unrecognised message keeps its text verbatim", () => {
    const parsed = parseError("something nobody anticipated");
    expect(parsed).toEqual({ label: "Something went wrong", detail: "something nobody anticipated" });
  });
});
