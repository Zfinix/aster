import { useTokens } from "../lib/highlight";
import { Code, TokenLine } from "./Code";
import { DiffView, looksLikeDiff } from "./DiffView";

const GUTTER = /^\s*(\d+) \| ?(.*)$/;

/** A tool step's output, rendered in whichever of the three shapes it arrives
 *  in: a diff, gutter-numbered file lines, or plain text. */
export function ToolOutput({ output, lang }: { output: string; lang?: string }) {
  const lines = output.split("\n");

  if (looksLikeDiff(lines)) {
    return (
      <pre className="tool-output">
        <DiffView lines={lines} lang={lang} />
      </pre>
    );
  }
  if (lines.some((l) => GUTTER.test(l))) {
    return <GutteredOutput lines={lines} lang={lang} />;
  }

  return (
    <pre className="tool-output">
      <code>
        <Code code={output} lang={lang} />
      </code>
    </pre>
  );
}

/**
 * Read output arrives as "  1 | code". The gutter moves to its own column and
 * the code side is tokenised as one body, so block comments and multi-line
 * strings still colour correctly.
 */
function GutteredOutput({ lines, lang }: { lines: string[]; lang?: string }) {
  const digits = lines.reduce((w, l) => {
    const m = GUTTER.exec(l);
    return m ? Math.max(w, m[1].length) : w;
  }, 1);

  const source = lines.map((l) => GUTTER.exec(l)?.[2] ?? l).join("\n");
  const tokens = useTokens(source, lang);

  return (
    <pre className="tool-output">
      {lines.map((line, i) => {
        const m = GUTTER.exec(line);
        const body = m?.[2] ?? line;
        return (
          <span key={i} className="out-line">
            <span className="out-gutter">{m ? m[1].padStart(digits) : ""}</span>
            <code>{tokens?.[i]?.length ? <TokenLine tokens={tokens[i]} /> : body || " "}</code>
          </span>
        );
      })}
    </pre>
  );
}
