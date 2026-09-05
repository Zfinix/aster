import { describe, expect, it } from "vitest";
import { fileTarget } from "./Link";

describe("fileTarget", () => {
  it.each([
    ["src/panel.ts", { path: "src/panel.ts", line: undefined }],
    ["src/panel.ts#L42", { path: "src/panel.ts", line: 42 }],
    ["src/panel.ts#L42-L51", { path: "src/panel.ts", line: 42 }],
    ["src/panel.ts:42", { path: "src/panel.ts", line: 42 }],
    ["README.md:12", { path: "README.md", line: 12 }],
    ["skills/aster-ui-demo/SKILL.md", { path: "skills/aster-ui-demo/SKILL.md", line: undefined }],
    ["file:///Users/me/notes.md", { path: "/Users/me/notes.md", line: undefined }],
    ["file:///Users/me/my%20notes.md:7", { path: "/Users/me/my notes.md", line: 7 }],
  ])("treats %s as a file", (url, expected) => {
    expect(fileTarget(url)).toEqual(expected);
  });

  it.each(["http://localhost:5173", "https://aster.dev/docs", "mailto:me@example.com", "#heading"])(
    "leaves %s to the host",
    (url) => {
      expect(fileTarget(url)).toBeUndefined();
    },
  );
});
