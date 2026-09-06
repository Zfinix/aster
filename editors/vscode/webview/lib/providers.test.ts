import { describe, expect, it } from "vitest";
import type { Provider } from "../../src/protocol";
import { keyPage, providerAuth, providerDetail, providerLabel, shortlist } from "./providers";

const provider = (name: string, base_url: string, key_env: string[] = []): Provider => ({
  name,
  base_url,
  example_model: "m",
  current: false,
  key_env,
});

const openrouter = provider("OpenRouter", "https://openrouter.ai/api/v1", ["OPEN_ROUTER_API_KEY"]);
const chatgpt = provider("ChatGPT (Codex subscription)", "https://chatgpt.com/backend-api/codex");
const openai = provider("OpenAI", "https://api.openai.com/v1", ["OPENAI_API_KEY"]);
const anthropic = provider("Anthropic (OpenAI compat layer)", "https://api.anthropic.com/v1", [
  "ANTHROPIC_API_KEY",
]);
const gemini = provider("Google Gemini", "https://generativelanguage.googleapis.com/v1beta/openai/", [
  "GEMINI_API_KEY",
  "GOOGLE_API_KEY",
]);
const deepseek = provider("DeepSeek", "https://api.deepseek.com/v1", ["DEEPSEEK_API_KEY"]);
const zai = provider("Z.ai (GLM)", "https://api.z.ai/api/paas/v4", ["ZAI_API_KEY"]);
const ollama = provider("Ollama (local)", "http://localhost:11434/v1");

describe("providerAuth", () => {
  it("signs in through the browser for OpenRouter and ChatGPT", () => {
    expect(providerAuth(openrouter)).toEqual({ kind: "login", target: "openrouter" });
    expect(providerAuth(chatgpt)).toEqual({ kind: "login", target: "codex" });
  });

  it("takes a key for everyone else, the first var in the catalog's order", () => {
    expect(providerAuth(gemini)).toEqual({ kind: "key", keyVar: "GEMINI_API_KEY" });
  });

  it("never offers the z.ai sign-in, which cannot finish without a terminal", () => {
    expect(providerAuth(zai)).toEqual({ kind: "key", keyVar: "ZAI_API_KEY" });
  });

  it("needs nothing for a server on this machine", () => {
    expect(providerAuth(ollama)).toEqual({ kind: "none" });
  });
});

describe("providerLabel and providerDetail", () => {
  it("moves the bracketed note to the second line", () => {
    expect(providerLabel(anthropic)).toEqual({ label: "Anthropic", note: "OpenAI compat layer" });
    expect(providerDetail(anthropic)).toBe("OpenAI compat layer · Paste an API key");
  });

  it("drops a bare local note, which the detail already says", () => {
    expect(providerLabel(ollama)).toEqual({ label: "Ollama", note: null });
    expect(providerDetail(ollama)).toBe("Runs on this machine");
  });

  it("describes sign-in providers by the browser", () => {
    expect(providerDetail(openrouter)).toBe("Sign in with your browser");
  });
});

describe("shortlist", () => {
  it("puts sign-in first, then the big four, in a fixed order", () => {
    const { first, rest } = shortlist([ollama, deepseek, openai, zai, chatgpt, gemini, anthropic, openrouter]);
    expect(first.map((p) => p.name)).toEqual([
      openrouter.name,
      chatgpt.name,
      openai.name,
      anthropic.name,
      gemini.name,
      deepseek.name,
    ]);
    expect(rest.map((p) => p.name)).toEqual([ollama.name, zai.name]);
  });

  it("shows everything when none of the shortlist is in the catalog", () => {
    const { first, rest } = shortlist([ollama, zai]);
    expect(first).toEqual([ollama, zai]);
    expect(rest).toEqual([]);
  });
});

describe("keyPage", () => {
  it("knows where the common keys live and admits when it does not", () => {
    expect(keyPage(openai)).toBe("https://platform.openai.com/api-keys");
    expect(keyPage(ollama)).toBeNull();
  });
});
