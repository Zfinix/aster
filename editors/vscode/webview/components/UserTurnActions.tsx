import { useEffect, useRef, useState } from "react";
import { GitForkIcon, PencilIcon, UndoIcon } from "./icons";

/** Hover actions on a sent message: edit and resend, fork into a new chat,
 *  rewind to here. Kept quiet on purpose: three faint glyphs that only appear
 *  on hover, no labels in the flow. */
export function UserTurnActions({
  onEdit,
  onFork,
  onRewind,
}: {
  onEdit: () => void;
  onFork: () => void;
  onRewind: () => void;
}) {
  return (
    <div className="turn-hover" role="toolbar" aria-label="Message actions">
      <button className="icon-btn" title="Edit and resend" aria-label="Edit and resend" onClick={onEdit}>
        <PencilIcon />
      </button>
      <button className="icon-btn" title="Fork into a new chat" aria-label="Fork into a new chat" onClick={onFork}>
        <GitForkIcon />
      </button>
      <button className="icon-btn" title="Rewind to here" aria-label="Rewind to here" onClick={onRewind}>
        <UndoIcon />
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
