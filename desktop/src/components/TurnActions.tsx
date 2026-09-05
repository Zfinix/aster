import { timeAgo } from "../lib/session";
import { useToast } from "./chrome";
import { CopyIcon } from "./icons";

/** Revealed on hover under a message: when it was sent, and a copy button. */
export function TurnActions({ text, ts }: { text: string; ts?: number }) {
  const toast = useToast();
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      toast("Copied");
    } catch {
      toast("Copy failed");
    }
  };
  return (
    <div className="turn-actions">
      {ts != null && <span className="turn-when">{timeAgo(ts)}</span>}
      <button type="button" className="icon-btn" aria-label="Copy message" title="Copy" onClick={copy}>
        <CopyIcon />
      </button>
    </div>
  );
}
