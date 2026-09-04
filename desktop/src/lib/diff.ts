import type { DiffFile, DiffLine } from "./types";

function stripPrefix(path: string): string {
  if (path.startsWith("a/") || path.startsWith("b/")) return path.slice(2);
  return path;
}

/** Parse a unified git diff into files and their line rows. Kept deliberately
 *  small: it handles the shapes aster's CLI emits (standard `git diff`, GitHub
 *  PR patches) without trying to be a full patch implementation. */
export function parseUnifiedDiff(text: string): DiffFile[] {
  const files: DiffFile[] = [];
  const lines = text.split("\n");
  let current: DiffFile | null = null;
  let oldNo = 0;
  let newNo = 0;

  const push = (line: DiffLine) => current && current.lines.push(line);

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    if (line.startsWith("diff --git")) {
      current = {
        oldPath: "",
        newPath: "",
        status: "modified",
        lines: [],
        additions: 0,
        deletions: 0,
      };
      files.push(current);
      const m = line.match(/^diff --git (\S+) (\S+)/);
      if (m) {
        current.oldPath = stripPrefix(m[1]);
        current.newPath = stripPrefix(m[2]);
      }
      continue;
    }
    if (!current) continue;

    if (line.startsWith("new file")) current.status = "added";
    else if (line.startsWith("deleted file")) current.status = "deleted";
    else if (line.startsWith("rename ")) current.status = "renamed";

    if (line.startsWith("--- ")) {
      const p = line.slice(4).trim();
      if (p !== "/dev/null") current.oldPath = stripPrefix(p);
      continue;
    }
    if (line.startsWith("+++ ")) {
      const p = line.slice(4).trim();
      if (p !== "/dev/null") current.newPath = stripPrefix(p);
      continue;
    }

    const hunk = line.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)/);
    if (hunk) {
      oldNo = parseInt(hunk[1], 10);
      newNo = parseInt(hunk[2], 10);
      push({ kind: "hunk", oldNo: null, newNo: null, text: hunk[3].trim() });
      continue;
    }

    if (line.startsWith("+")) {
      push({ kind: "add", oldNo: null, newNo, text: line.slice(1) });
      newNo++;
      current.additions++;
    } else if (line.startsWith("-")) {
      push({ kind: "del", oldNo, newNo: null, text: line.slice(1) });
      oldNo++;
      current.deletions++;
    } else if (line.startsWith(" ")) {
      push({ kind: "ctx", oldNo, newNo, text: line.slice(1) });
      oldNo++;
      newNo++;
    }
    // Any other metadata line (index, mode, \ No newline) is intentionally ignored.
  }

  return files.filter((f) => f.lines.length > 0);
}

/** Path used to anchor findings, preferring the new path (right side). */
export function fileKey(f: DiffFile): string {
  return f.newPath || f.oldPath;
}
