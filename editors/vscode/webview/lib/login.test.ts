import { describe, expect, it } from "vitest";
import { loginLine } from "./login";

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
