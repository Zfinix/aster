import { timeAgo } from "../lib/session";
import { useToast } from "./chrome";
import { CopyIcon } from "./icons";

/** The hover row under a chat message: relative timestamp + copy. */
export function MessageActions({ text, ts }: { text: string; ts?: number }) {
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
    <div className="msg-actions">
      {ts != null && <span>{timeAgo(ts)}</span>}
      <button className="msg-copy" aria-label="Copy message" onClick={copy}>
        <CopyIcon />
      </button>
    </div>
  );
}
