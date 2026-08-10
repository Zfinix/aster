import { languageFromPath } from "../lib/highlight";
import { DiffView } from "./DiffView";

/** The preview opens with `edit <path>:`, which is the only clue to what
 *  language the lines below it are. */
const HEADER = /^\w+ (\S+?):?$/;

/** Shown inline in the thread while the CLI blocks on an `ask`-mode edit. */
export function ApprovalPrompt({
  preview,
  onRespond,
}: {
  preview: string;
  onRespond: (allow: boolean) => void;
}) {
  const lines = preview.split("\n");
  const lang = languageFromPath(HEADER.exec(lines[0])?.[1]);

  return (
    <div className="approval">
      <div className="approval-head">Approve this edit?</div>
      <pre className="approval-preview">
        <DiffView lines={lines} lang={lang} />
      </pre>
      <div className="approval-actions">
        <button className="btn-primary" onClick={() => onRespond(true)}>
          Approve
        </button>
        <button className="btn" onClick={() => onRespond(false)}>
          Reject
        </button>
      </div>
    </div>
  );
}
