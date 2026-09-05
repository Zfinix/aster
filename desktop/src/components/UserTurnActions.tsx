import { useRef, useState } from "react";
import { GitBranchIcon, PencilIcon, SendIcon, UndoIcon, XIcon } from "./icons";
import { TurnActions } from "./TurnActions";
import "../styles/user-turn-actions.css";

export interface UserTurnActionHandlers {
  disabled: boolean;
  onEditSend: (text: string) => void;
  onFork: () => void;
  onRewind: () => void;
}

export function UserTurnActions({ text, ts, actions }: { text: string; ts?: number; actions: UserTurnActionHandlers }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(text);
  const editRef = useRef<HTMLButtonElement>(null);
  const cancel = () => {
    setEditing(false);
    requestAnimationFrame(() => editRef.current?.focus());
  };
  const send = () => {
    if (actions.disabled || !draft.trim()) return;
    actions.onEditSend(draft.trim());
    cancel();
  };

  return (
    <>
      <div className={`turn-user${editing ? " user-turn-editing" : ""}`}>
        {editing ? (
          <div className="user-turn-editor">
            <textarea
              autoFocus
              aria-label="Edit message"
              value={draft}
              disabled={actions.disabled}
              rows={Math.min(10, Math.max(2, draft.split("\n").length))}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.nativeEvent.isComposing || e.keyCode === 229) return;
                if (e.key === "Escape") {
                  e.preventDefault();
                  cancel();
                } else if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  send();
                }
              }}
            />
            <div className="user-turn-editor-buttons">
              <button type="button" className="icon-btn" aria-label="Cancel edit" title="Cancel edit" disabled={actions.disabled} onClick={cancel}><XIcon /></button>
              <button type="button" className="icon-btn" aria-label="Send edited message" title="Send edited message" disabled={actions.disabled || !draft.trim()} onClick={send}><SendIcon /></button>
            </div>
          </div>
        ) : <div className="turn-user-text">{text}</div>}
      </div>
      <div className="user-turn-actions">
        <div className="user-turn-controls">
          <button ref={editRef} type="button" className="icon-btn" aria-label="Edit message" title="Edit message" disabled={actions.disabled || editing} onClick={() => { setDraft(text); setEditing(true); }}><PencilIcon /></button>
          <button type="button" className="icon-btn" aria-label="Fork from message" title="Fork from message" disabled={actions.disabled || editing} onClick={actions.onFork}><GitBranchIcon /></button>
          <button type="button" className="icon-btn" aria-label="Rewind to message" title="Rewind to message" disabled={actions.disabled || editing} onClick={actions.onRewind}><UndoIcon /></button>
        </div>
        <TurnActions text={text} ts={ts} />
      </div>
    </>
  );
}
