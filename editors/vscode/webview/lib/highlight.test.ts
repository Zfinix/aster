import { describe, expect, it } from "vitest";
import { languageFromPath, normalizeLang } from "./highlight";

describe("normalizeLang", () => {
  it("accepts a bundled grammar by name", () => {
    expect(normalizeLang("rust")).toBe("rust");
  });

  it("resolves the aliases fences actually carry", () => {
    expect(normalizeLang("rs")).toBe("rust");
    expect(normalizeLang("bash")).toBe("shellscript");
    expect(normalizeLang("YML")).toBe("yaml");
  });

  it("gives up on a grammar it does not ship, so the caller renders plain text", () => {
    expect(normalizeLang("brainfuck")).toBeUndefined();
    expect(normalizeLang("")).toBeUndefined();
  });
});

describe("languageFromPath", () => {
  it("reads the language off the extension", () => {
    expect(languageFromPath("crates/aster-cli/src/init.rs")).toBe("rust");
    expect(languageFromPath("webview/App.tsx")).toBe("tsx");
  });

  it("knows Cargo.lock is TOML despite the extension", () => {
    expect(languageFromPath("Cargo.lock")).toBe("toml");
  });

  it("leaves an extensionless or dotfile path unhighlighted", () => {
    expect(languageFromPath("Makefile")).toBeUndefined();
    expect(languageFromPath(".env.local")).toBeUndefined();
    expect(languageFromPath(undefined)).toBeUndefined();
  });
});
