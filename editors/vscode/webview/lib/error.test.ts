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

  it("sees through an exit-code wrapper to the status inside", () => {
    const { label, hint, detail } = parseError(
      "aster exited with code 1: model endpoint returned 429"
    );
    expect(label).toBe("Too many requests");
    expect(hint).toContain("slow down");
    expect(detail).toBe("model endpoint returned 429");
  });

  it("explains a bare exit-code failure without plumbing", () => {
    const { label, hint, detail } = parseError(
      "aster chat exited with code 1. See the Aster output channel."
    );
    expect(label).toBe("Aster stopped unexpectedly");
    expect(hint).toContain("Send your message again");
    expect(hint).not.toContain("engine");
    expect(detail).toBe("");
  });

  it("does not mistake a panic's line number for an HTTP status", () => {
    const { label } = parseError(
      "aster chat exited with code 101: thread 'main' panicked at src/wire.rs:429"
    );
    expect(label).toBe("Aster stopped unexpectedly");
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
    expect(hint).toContain("Send again");
  });

  it("explains a DNS failure as an unreachable provider", () => {
    const { label, hint, detail } = parseError(
      "chat request failed: error sending request for url (https://api.deepseek.com/v1/chat/completions): client error (Connect): dns error: failed to lookup address information: nodename nor servname provided, or not known"
    );
    expect(label).toBe("Can't reach the provider");
    expect(hint).toContain("internet connection");
    expect(detail).toContain("dns error");
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
