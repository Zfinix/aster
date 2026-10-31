import { CopyButton } from "./CopyButton";
import { GitForkIcon, PencilIcon } from "./icons";

/** Actions on a sent message: copy it, edit it back into the composer, or fork
 *  a new chat from it. They sit above the bubble's top-right corner and appear
 *  on hover, icon-only so they stay out of the message's way. While a turn
 *  runs, edit and fork read as disabled rather than swallowing the click. */
export function UserTurnActions({
  text,
  onEdit,
  onFork,
  busy,
}: {
  text: string;
  onEdit: () => void;
  onFork: () => void;
  busy: boolean;
}) {
  const wait = "Wait for the reply to finish";
  return (
    <div className="turn-hover" role="toolbar" aria-label="Message actions">
      <CopyButton text={text} label="Copy message" />
      <button
        className="icon-btn"
        title={busy ? wait : "Edit and resend"}
        aria-label="Edit and resend"
        disabled={busy}
        onClick={onEdit}
      >
        <PencilIcon />
      </button>
      <button
        className="icon-btn"
        title={busy ? wait : "Fork into a new chat"}
        aria-label="Fork into a new chat"
        disabled={busy}
        onClick={onFork}
      >
        <GitForkIcon />
      </button>
    </div>
  );
}
