import { Streamdown } from "streamdown";

/** Inline card while the CLI blocks on an edit or plan approval. */
export function ApprovalPrompt({
  preview,
  markdown,
  onRespond,
}: {
  preview: string;
  markdown?: string;
  onRespond: (allow: boolean) => void;
}) {
  return (
    <div className="approval">
      <div className="approval-head">{markdown ? "Approve this plan?" : "Approve this change?"}</div>
      {markdown ? (
        <div className="approval-plan prose">
          <Streamdown parseIncompleteMarkdown={false}>{markdown}</Streamdown>
        </div>
      ) : (
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
      )}
      <div className="approval-actions">
        <button type="button" className="btn-primary" onClick={() => onRespond(true)}>
          Approve
        </button>
        <button type="button" className="btn" onClick={() => onRespond(false)}>
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
