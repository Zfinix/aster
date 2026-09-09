import { describe, expect, test } from "bun:test";
import { fileKey, parseUnifiedDiff } from "../src/lib/diff";

const MODIFIED = `diff --git a/src/lib/one.ts b/src/lib/one.ts
index 1111111..2222222 100644
--- a/src/lib/one.ts
+++ b/src/lib/one.ts
@@ -10,3 +10,4 @@ export function one() {
 const kept = 1;
-const gone = 2;
+const added = 2;
+const alsoAdded = 3;
`;

describe("parseUnifiedDiff", () => {
  test("numbers context, additions and deletions from the hunk header", () => {
    const [file] = parseUnifiedDiff(MODIFIED);
    expect(file.newPath).toBe("src/lib/one.ts");
    expect(file.status).toBe("modified");
    expect(file.additions).toBe(2);
    expect(file.deletions).toBe(1);
    expect(file.lines.map((l) => [l.kind, l.oldNo, l.newNo])).toEqual([
      ["hunk", null, null],
      ["ctx", 10, 10],
      ["del", 11, null],
      ["add", null, 11],
      ["add", null, 12],
    ]);
  });

  test("the hunk row carries the trailing section label", () => {
    expect(parseUnifiedDiff(MODIFIED)[0].lines[0].text).toBe("export function one() {");
  });

  test("a/ and b/ prefixes are stripped from both sides", () => {
    const [file] = parseUnifiedDiff(MODIFIED);
    expect(file.oldPath).toBe("src/lib/one.ts");
  });

  test("a new file keeps its path from +++ and not from /dev/null", () => {
    const [file] = parseUnifiedDiff(`diff --git a/new.ts b/new.ts
new file mode 100644
--- /dev/null
+++ b/new.ts
@@ -0,0 +1,1 @@
+const fresh = 1;
`);
    expect(file.status).toBe("added");
    expect(file.oldPath).toBe("new.ts");
    expect(file.newPath).toBe("new.ts");
  });

  test("a deleted file is marked and counted", () => {
    const [file] = parseUnifiedDiff(`diff --git a/old.ts b/old.ts
deleted file mode 100644
--- a/old.ts
+++ /dev/null
@@ -1,1 +0,0 @@
-const stale = 1;
`);
    expect(file.status).toBe("deleted");
    expect(file.deletions).toBe(1);
  });

  test("a rename is marked", () => {
    const [file] = parseUnifiedDiff(`diff --git a/from.ts b/to.ts
rename from from.ts
rename to to.ts
@@ -1,1 +1,1 @@
-const a = 1;
+const a = 2;
`);
    expect(file.status).toBe("renamed");
  });

  test("several files in one patch stay separate", () => {
    const files = parseUnifiedDiff(MODIFIED + MODIFIED.replace(/one/g, "two"));
    expect(files.map((f) => f.newPath)).toEqual(["src/lib/one.ts", "src/lib/two.ts"]);
  });

  test("a file with a header but no hunk is dropped", () => {
    expect(parseUnifiedDiff("diff --git a/mode.ts b/mode.ts\nold mode 100644\nnew mode 100755\n")).toEqual([]);
  });

  test("lines before the first header are ignored", () => {
    expect(parseUnifiedDiff("commit abc\nAuthor: someone\n\n+not a diff\n")).toEqual([]);
  });

  test("a single-line hunk header without counts still parses", () => {
    const [file] = parseUnifiedDiff(`diff --git a/x.ts b/x.ts
--- a/x.ts
+++ b/x.ts
@@ -5 +5 @@
-a
+b
`);
    expect(file.lines[1]).toMatchObject({ kind: "del", oldNo: 5 });
    expect(file.lines[2]).toMatchObject({ kind: "add", newNo: 5 });
  });
});

describe("fileKey", () => {
  test("prefers the right-hand path", () => {
    expect(fileKey({ oldPath: "from.ts", newPath: "to.ts" } as never)).toBe("to.ts");
  });

  test("falls back to the left when the file was deleted", () => {
    expect(fileKey({ oldPath: "gone.ts", newPath: "" } as never)).toBe("gone.ts");
  });
});
