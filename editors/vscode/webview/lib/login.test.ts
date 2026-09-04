import { describe, expect, it } from "vitest";
import { loginLine, setupCopy } from "./login";

describe("setupCopy", () => {
  it("offers the ChatGPT sign-in for the codex endpoint", () => {
    const copy = setupCopy({
      provider: "ChatGPT",
      base_url: "https://chatgpt.com/backend-api/codex",
      login: "codex",
      key_vars: [],
    });
    expect(copy.title).toBe("Sign in to ChatGPT");
    expect(copy.signIn).toBe("Sign in with ChatGPT");
    expect(copy.keyVar).toBeUndefined();
  });

  it("offers both a sign-in and a key for OpenRouter", () => {
    const copy = setupCopy({
      provider: "OpenRouter",
      base_url: "https://openrouter.ai/api/v1",
      login: "openrouter",
      key_vars: ["OPEN_ROUTER_API_KEY", "ASTER_API_KEY"],
    });
    expect(copy.signIn).toBe("Sign in with OpenRouter");
    expect(copy.keyVar).toBe("OPEN_ROUTER_API_KEY");
  });

  it("asks for a key when the endpoint has no browser login", () => {
    const copy = setupCopy({
      provider: "DeepSeek",
      base_url: "https://api.deepseek.com",
      login: null,
      key_vars: ["DEEPSEEK_API_KEY", "ASTER_API_KEY"],
    });
    expect(copy.title).toBe("Add an API key for DeepSeek");
    expect(copy.signIn).toBeUndefined();
    expect(copy.keyVar).toBe("DEEPSEEK_API_KEY");
  });
});

describe("loginLine", () => {
  it("keeps only the most recent lines", () => {
    let state = null;
    for (let i = 0; i < 20; i++) {
      state = loginLine(state, `line ${i}`);
    }
    expect(state?.lines.length).toBe(12);
    expect(state?.lines[0]).toBe("line 8");
  });
});
