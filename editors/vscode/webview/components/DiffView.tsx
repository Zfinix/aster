import { openFilePreview } from "../lib/filePreview";
import { Code } from "./Code";

const HUNK = /^@@ .* @@/;
const HUNK_AT = /^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/;
const FILE_MARKER = /^(\+\+\+|---)\s/;
const FILE_AT = /^\+\+\+ (?:b\/)?(\S+)/;

/** Unified hunks are unambiguous. The bare `- old` / `+ new` form the editor's
 *  own previews use is not, so it needs both signs present and to dominate the
 *  block, or a search result full of hyphens would render as a diff. */
export function looksLikeDiff(lines: string[]): boolean {
  if (lines.some((l) => HUNK.test(l))) return true;

  const body = lines.filter((l) => l.trim());
  const adds = body.filter((l) => l.startsWith("+")).length;
  const dels = body.filter((l) => l.startsWith("-")).length;
  return adds > 0 && dels > 0 && adds + dels >= body.length * 0.6;
}

function kindOf(line: string): string | undefined {
  if (HUNK.test(line)) return "hunk";
  if (FILE_MARKER.test(line)) return "meta";
  if (line.startsWith("+")) return "add";
  if (line.startsWith("-")) return "del";
  return undefined;
}

/** Each side of a change is highlighted on its own: the two versions interleave,
 *  so there is no single body to tokenise in one pass.
 *
 *  A changed or context line is clickable when the diff names its file and
 *  hunk: it opens that file on the line the new side shows. Bare `-/+` diffs
 *  carry no hunk headers, so there is nothing to land on and nothing clicks. */
export function DiffView({ lines, lang }: { lines: string[]; lang?: string }) {
  let file: string | undefined;
  let at = 0;
  return (
    <>
      {lines.map((line, i) => {
        const kind = kindOf(line);
        const marked = kind === "add" || kind === "del";
        const body = marked ? line.slice(1).replace(/^ /, "") : line;
        const fileMatch = FILE_AT.exec(line);
        if (fileMatch) file = fileMatch[1];
        const hunkMatch = HUNK_AT.exec(line);
        if (hunkMatch) at = Number(hunkMatch[1]);
        const targetFile: string = file ?? "";
        const targetLine = at;
        const lands = (kind === "add" || kind === undefined) && targetFile !== "" && targetLine > 0;
        const jump = lands ? () => openFilePreview(targetFile, targetLine) : undefined;
        if (kind === "add" || kind === undefined) at += 1;
        return (
          <span
            key={i}
            className="out-line"
            data-diff={kind}
            data-jump={jump ? "" : undefined}
            onClick={jump}
            onKeyDown={
              jump
                ? (e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      jump();
                    }
                  }
                : undefined
            }
            role={jump ? "button" : undefined}
            tabIndex={jump ? 0 : undefined}
            title={jump ? `Open ${targetFile}:${targetLine}` : undefined}
          >
            {marked && <span className="diff-marker">{line[0]}</span>}
            <code>{marked ? <Code code={body || " "} lang={lang} /> : body || " "}</code>
          </span>
        );
      })}
    </>
  );
}
