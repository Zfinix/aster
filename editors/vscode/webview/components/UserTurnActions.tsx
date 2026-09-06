import { useEffect, useRef, useState } from "react";
import { GitForkIcon, PencilIcon } from "./icons";

/** Hover actions on a sent message: edit and resend, fork into a new chat.
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

/** The message becoming the composer: same box, your text in it, Enter resends,
 *  Escape puts it back. No modal, no warning row. */
export function UserTurnEditor({
  text,
  onSend,
  onCancel,
}: {
  text: string;
  onSend: (next: string) => void;
  onCancel: () => void;
}) {
  const [draft, setDraft] = useState(text);
  const area = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    area.current?.focus();
    area.current?.setSelectionRange(text.length, text.length);
  }, []);

  const grow = (el: HTMLTextAreaElement) => {
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  };

  return (
    <div className="turn-user turn-user-editing">
      <textarea
        ref={area}
        className="turn-edit-area"
        value={draft}
        rows={1}
        onChange={(e) => {
          setDraft(e.target.value);
          grow(e.target);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            const next = draft.trim();
            if (next) onSend(next);
          } else if (e.key === "Escape") {
            e.preventDefault();
            onCancel();
          }
        }}
      />
      <div className="turn-edit-hint">
        <span>Enter to resend · Esc to cancel</span>
        <button
          className="turn-edit-cancel"
          onClick={onCancel}
        >
          Cancel
        </button>
      </div>
    </div>
  );
}
