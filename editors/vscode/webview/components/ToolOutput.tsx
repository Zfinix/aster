import { useTokens } from "../lib/highlight";
import { openFilePreview } from "../lib/filePreview";
import { Code, TokenLine } from "./Code";
import { DiffView, looksLikeDiff } from "./DiffView";

const GUTTER = /^\s*(\d+) \| ?(.*)$/;

/** A tool step's output, rendered in whichever of the three shapes it arrives
 *  in: a diff, gutter-numbered file lines, or plain text. `path` is the file
 *  the step touched, when it has one, so numbered lines can open at their line. */
export function ToolOutput({ output, lang, path }: { output: string; lang?: string; path?: string }) {
  const lines = output.split("\n");

  if (looksLikeDiff(lines)) {
    return (
      <pre className="tool-output">
        <DiffView lines={lines} lang={lang} />
      </pre>
    );
  }
  if (lines.some((l) => GUTTER.test(l))) {
    return <GutteredOutput lines={lines} lang={lang} path={path} />;
  }

  return (
    <pre className="tool-output">
      <code>
        <Code code={output} lang={lang} />
      </code>
    </pre>
  );
}

function GutteredOutput({
  lines,
  lang,
  path,
}: {
  lines: string[];
  lang?: string;
  path?: string;
}) {
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
        const number = m ? Number(m[1]) : undefined;
        const jump = path && number ? () => openFilePreview(path, number) : undefined;
        return (
          <span
            key={i}
            className="out-line"
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
            title={jump ? `Open ${path}:${number}` : undefined}
          >
            <span className="out-gutter">{m ? m[1].padStart(digits) : ""}</span>
            <code>{tokens?.[i]?.length ? <TokenLine tokens={tokens[i]} /> : body || " "}</code>
          </span>
        );
      })}
    </pre>
  );
}
