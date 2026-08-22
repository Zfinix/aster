import { Streamdown } from "streamdown";

/** Inline card while the CLI blocks on an `ask`-mode edit approval. */
export function ApprovalPrompt({
  preview,
  markdown,
  onRespond,
}: {
  preview: string;
  /** Set for a plan, which is prose to read rather than a command to scan. */
  markdown?: string;
  onRespond: (allow: boolean) => void;
}) {
  if (markdown) {
    return (
      <div className="approval">
        <div className="approval-head">Approve plan</div>
        <div className="approval-plan md">
          <Streamdown parseIncompleteMarkdown={false}>{markdown}</Streamdown>
        </div>
        <div className="approval-actions">
          <button className="btn btn-primary" onClick={() => onRespond(true)}>
            Approve
          </button>
          <button className="btn btn-ghost" onClick={() => onRespond(false)}>
            Reject
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="approval">
      <div className="approval-head">Approve command</div>
      <pre className="approval-preview">
        <code>
          {preview.split("\n").map((line, i) => (
            <span key={i} className={diffClass(line)}>
              {line}
              {"\n"}
            </span>
          ))}
        </code>
      </pre>
      <div className="approval-actions">
        <button className="btn btn-primary" onClick={() => onRespond(true)}>
          Approve
        </button>
        <button className="btn btn-ghost" onClick={() => onRespond(false)}>
          Reject
        </button>
      </div>
    </div>
  );
}

function diffClass(line: string): string | undefined {
  if (line.startsWith("+")) return "add";
  if (line.startsWith("-")) return "del";
  return undefined;
}
