import { describe, expect, it } from "vitest";
import { redactSecrets } from "./redact";

describe("redactSecrets", () => {
  it("masks an inline env assignment but keeps the variable name", () => {
    const out = redactSecrets('npx -y @21st-dev/magic@latest API_KEY="6d4dcfff37d1a7fc956291bc"');
    expect(out).toContain("API_KEY=");
    expect(out).not.toContain("6d4dcfff37d1a7fc956291bc");
  });

  it("masks a credential flag's value but keeps the flag", () => {
    const out = redactSecrets("npx -y @supabase/mcp-server --access-token sbp_352056467896e2b2");
    expect(out).toContain("--access-token");
    expect(out).not.toContain("sbp_352056467896e2b2");
  });

  it("masks vendor-prefixed keys wherever they appear", () => {
    expect(redactSecrets("run sk-abcdef0123456789 now")).not.toContain("sk-abcdef0123456789");
    expect(redactSecrets("AKIAIOSFODNN7EXAMPLE")).not.toContain("AKIAIOSFODNN7EXAMPLE");
  });

  it("leaves ordinary commands, flags, and urls alone", () => {
    const plain = "npx -y chrome-devtools-mcp@latest";
    expect(redactSecrets(plain)).toBe(plain);
    const url = "https://mcp.linear.app/mcp";
    expect(redactSecrets(url)).toBe(url);
    const withFlags = "dart mcp-server --force-roots-fallback";
    expect(redactSecrets(withFlags)).toBe(withFlags);
  });

  it("leaves a long path readable", () => {
    const path = "node /Users/chizi/projects/work-projects/mcp/gta_mcp/build/server.js";
    expect(redactSecrets(path)).toBe(path);
  });
});
