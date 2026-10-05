import { GitForkIcon, PencilIcon } from "./icons";

/** Hover actions on a sent message: edit loads it into the composer to resend,
 *  fork starts a new chat from it.
 *  Kept quiet on purpose: two faint glyphs that only appear on hover, no
 *  labels in the flow. While a turn runs the buttons read as disabled rather
 *  than swallowing the click. */
export function UserTurnActions({
  onEdit,
  onFork,
  busy,
}: {
  onEdit: () => void;
  onFork: () => void;
  busy: boolean;
}) {
  const wait = "Wait for the reply to finish";
  return (
    <div className="turn-hover" role="toolbar" aria-label="Message actions">
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

