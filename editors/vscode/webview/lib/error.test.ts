import { describe, expect, it } from "vitest";
import { parseError } from "./error";

describe("parseError", () => {
  it("labels a known status code in plain language and keeps the detail", () => {
    const { label, hint, detail } = parseError(
      "bad request (400): Reasoning is mandatory for this endpoint and cannot be disabled."
    );
    expect(label).toBe("Request rejected");
    expect(hint).toBe("The provider said no.");
    expect(detail).toBe("Reasoning is mandatory for this endpoint and cannot be disabled.");
  });

  it("tells the user what to do when rate limited", () => {
    const { label, hint } = parseError("rate limited (429): slow down");
    expect(label).toBe("Too many requests");
    expect(hint).toBe("Wait a moment, then retry.");
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

  it("tells the user when there is no history to fold yet", () => {
    const { label, hint, detail } = parseError("nothing to compact yet");
    expect(label).toBe("Nothing to compact");
    expect(hint).toContain("still short");
    expect(detail).toBe("");
  });

  it("names a mid-stream drop without a paragraph of explanation", () => {
    const { label, hint, detail } = parseError(
      "reading stream chunk: error decoding response body: request or response body error: operation timed out"
    );
    expect(label).toBe("Connection dropped");
    expect(hint).toBeUndefined();
    expect(detail).toContain("operation timed out");
  });

  it("sees through an exit-code wrapper to the status inside", () => {
    const { label, hint, detail } = parseError(
      "aster exited with code 1: model endpoint returned 429"
    );
    expect(label).toBe("Too many requests");
    expect(hint).toBe("Wait a moment, then retry.");
    expect(detail).toBe("model endpoint returned 429");
  });

  it("explains a bare exit-code failure without plumbing", () => {
    const { label, hint, detail } = parseError(
      "aster chat exited with code 1. See the Aster output channel."
    );
    expect(label).toBe("Aster stopped");
    expect(hint).toBe("Send your message again.");
    expect(detail).toBe("");
  });

  it("does not mistake a panic's line number for an HTTP status", () => {
    const { label } = parseError(
      "aster chat exited with code 101: thread 'main' panicked at src/wire.rs:429"
    );
    expect(label).toBe("Aster stopped");
  });

  it("prefers the connection advice when a dropped stream quotes a status", () => {
    const { label } = parseError(
      "connection reset by peer (os error 104) after retrying status 503"
    );
    expect(label).toBe("Connection dropped");
  });

  it("maps provider 5xx errors to a retry hint", () => {
    const { label, hint } = parseError("bad gateway (502): upstream reset");
    expect(label).toBe("Provider trouble");
    expect(hint).toBe("Try again in a moment.");
  });

  it("explains a DNS failure as an unreachable provider", () => {
    const { label, hint, detail } = parseError(
      "chat request failed: error sending request for url (https://api.deepseek.com/v1/chat/completions): client error (Connect): dns error: failed to lookup address information: nodename nor servname provided, or not known"
    );
    expect(label).toBe("Can't reach the provider");
    expect(hint).toBe("Check your connection.");
    expect(detail).toContain("dns error");
  });

  it("names a broken reply without explaining the repetition", () => {
    const { label, hint } = parseError(
      "Stopped: model is repeating itself. Try a stronger model or higher effort."
    );
    expect(label).toBe("Reply broke");
    expect(hint).toBeUndefined();
  });

  it("keeps every hint to five words or fewer", () => {
    const messages = [
      "bad request (400): nope",
      "unauthorized (401): nope",
      "not found (404): nope",
      "timeout (408): nope",
      "rate limited (429): nope",
      "bad gateway (502): nope",
      "connection reset by peer",
      "dns error: failed to lookup address",
      "aster chat exited with code 1.",
      "nothing to compact yet",
    ];
    for (const message of messages) {
      const { hint } = parseError(message);
      if (!hint) continue;
      expect(hint.split(/\s+/).length, `${message}: ${hint}`).toBeLessThanOrEqual(5);
    }
  });
});

describe("parseError credentials", () => {
  it("says the endpoint needs a sign-in when the CLI found no key", () => {
    const { label, hint } = parseError(
      "aster chat exited with code 1: no API key found for DeepSeek. Run `aster init` to set one up globally"
    );
    expect(label).toBe("Not signed in");
    expect(hint).toContain("Sign in");
  });

  it("recognises a missing ChatGPT login", () => {
    const { label } = parseError("not signed in to ChatGPT. Run `aster login codex` to link your subscription");
    expect(label).toBe("Not signed in");
  });
});
