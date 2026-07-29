import { highlightCached } from "../lib/highlight";

const GUTTER = /^\s*(\d+) \| ?(.*)$/;

/** A tool step's output. File reads arrive as "  1 | code" lines: split the
 *  gutter off and syntax-highlight the code side. Anything else stays plain. */
export function ActivityOutput({ name, output }: { name: string; output: string }) {
  const lines = output.split("\n");
  const guttered = lines.some((l) => GUTTER.test(l));

  if (guttered) {
    const digits = lines.reduce((w, l) => {
      const m = GUTTER.exec(l);
      return m ? Math.max(w, m[1].length) : w;
    }, 1);
    return (
      <pre className="activity-output">
        {lines.map((l, i) => {
          const m = GUTTER.exec(l);
          return (
            <span key={i} className="out-line">
              {m ? (
                <>
                  <span className="out-gutter">{m[1].padStart(digits)}</span>
                  <code dangerouslySetInnerHTML={{ __html: highlightCached(m[2] || " ") }} />
                </>
              ) : (
                <code>{l || " "}</code>
              )}
            </span>
          );
        })}
      </pre>
    );
  }

  if (name === "read_file") {
    return (
      <pre className="activity-output">
        <code dangerouslySetInnerHTML={{ __html: highlightCached(output) }} />
      </pre>
    );
  }

  return (
    <pre className="activity-output">
      <code>{output}</code>
    </pre>
  );
}
