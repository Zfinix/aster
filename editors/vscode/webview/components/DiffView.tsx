import { Code } from "./Code";

const HUNK = /^@@ .* @@/;
const FILE_MARKER = /^(\+\+\+|---)\s/;

/**
 * Unified hunks are unambiguous. The bare `- old` / `+ new` form the editor's
 * own previews use is not, so it needs both signs present and to dominate the
 * block, or a search result full of hyphens would render as a diff.
 */
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
 *  so there is no single body to tokenise in one pass. */
export function DiffView({ lines, lang }: { lines: string[]; lang?: string }) {
  return (
    <>
      {lines.map((line, i) => {
        const kind = kindOf(line);
        const marked = kind === "add" || kind === "del";
        const body = marked ? line.slice(1).replace(/^ /, "") : line;
        return (
          <span key={i} className="out-line" data-diff={kind}>
            {marked && <span className="diff-marker">{line[0]}</span>}
            <code>{marked ? <Code code={body || " "} lang={lang} /> : body || " "}</code>
          </span>
        );
      })}
    </>
  );
}
