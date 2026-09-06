import { useEffect, useRef, useState } from "react";
import { ArrowUpIcon, GripIcon, PencilIcon, TrashIcon } from "./icons";

/** The queued chips, docked at the top of the composer box: grip to reorder,
 *  Send now pushes one into the running turn, pencil edits, trash drops it.
 *  When the run ends or is stopped, the first chip dips into the thread. */
export function QueuedList({
  queued,
  onSteer,
  onEdit,
  onRemove,
  onReorder,
}: {
  queued: { id: string; text: string }[];
  onSteer: (id: string) => void;
  onEdit: (id: string, text: string) => void;
  onRemove: (id: string) => void;
  onReorder: (from: number, to: number) => void;
}) {
  const list = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState<number | null>(null);

  // Pointer events rather than HTML5 drag and drop: a webview does not give a
  // drag a reliable start, and this works the same under a finger.
  const beginDrag = (index: number, event: React.PointerEvent) => {
    event.preventDefault();
    let at = index;
    setDragging(index);
    const move = (moved: PointerEvent) => {
      const chips = [...(list.current?.querySelectorAll<HTMLElement>(".queued-chip") ?? [])];
      const over = chips.findIndex((chip) => {
        const box = chip.getBoundingClientRect();
        return moved.clientY >= box.top && moved.clientY <= box.bottom;
      });
      if (over === -1 || over === at) return;
      onReorder(at, over);
      at = over;
      setDragging(over);
    };
    const done = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", done);
      window.removeEventListener("pointercancel", done);
      setDragging(null);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", done);
    window.addEventListener("pointercancel", done);
  };

  if (queued.length === 0) return null;
  return (
    <div className="queued-list" ref={list} aria-label="Queued messages">
      {queued.map((item, i) => (
        <QueuedTurn
          key={item.id}
          index={i}
          text={item.text}
          dragging={dragging === i}
          onGrab={beginDrag}
          onSteer={() => onSteer(item.id)}
          onEdit={(text) => onEdit(item.id, text)}
          onRemove={() => onRemove(item.id)}
          onReorder={onReorder}
        />
      ))}
    </div>
  );
}

/** One queued message chip. */
export function QueuedTurn({
  index,
  text,
  dragging = false,
  onGrab,
  onSteer,
  onEdit,
  onRemove,
  onReorder,
}: {
  index: number;
  text: string;
  dragging?: boolean;
  onGrab?: (index: number, event: React.PointerEvent) => void;
  onSteer: () => void;
  onEdit: (next: string) => void;
  onRemove: () => void;
  onReorder: (from: number, to: number) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(text);
  const area = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (!editing) return;
    area.current?.focus();
    area.current?.setSelectionRange(draft.length, draft.length);
  }, [editing]);

  const commit = () => {
    setEditing(false);
    if (draft.trim() && draft !== text) onEdit(draft.trim());
  };

  if (editing) {
    return (
      <div className="queued-chip queued-chip-editing">
        <textarea
          ref={area}
          className="turn-edit-area"
          value={draft}
          rows={1}
          onChange={(e) => {
            setDraft(e.target.value);
            e.target.style.height = "auto";
            e.target.style.height = `${e.target.scrollHeight}px`;
          }}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              commit();
            } else if (e.key === "Escape") {
              e.preventDefault();
              setDraft(text);
              setEditing(false);
            }
          }}
        />
        <div className="turn-edit-hint">
          <span>Enter to save · Esc to cancel</span>
        </div>
      </div>
    );
  }

  return (
    <div
      className={`queued-chip${dragging ? " queued-dragging" : ""}`}
      tabIndex={0}
      aria-label={`Queued: ${text}. Alt with the arrow keys reorders it.`}
      onKeyDown={(e) => {
        if (!e.altKey || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return;
        e.preventDefault();
        const to = index + (e.key === "ArrowUp" ? -1 : 1);
        if (to >= 0) onReorder(index, to);
      }}
    >
      <span
        className="queued-grip"
        title="Drag to reorder"
        aria-hidden="true"
        onPointerDown={(e) => onGrab?.(index, e)}
      >
        <GripIcon />
      </span>
      <button className="queued-text" title={text} onClick={() => setEditing(true)}>
        {text}
      </button>
      <button
        className="queued-send-now"
        title="Send now, ahead of the rest of the queue"
        onClick={onSteer}
      >
        <ArrowUpIcon />
        Send now
      </button>
      <button className="icon-btn" title="Edit message" aria-label="Edit message" onClick={() => setEditing(true)}>
        <PencilIcon />
      </button>
      <button className="icon-btn" title="Remove from queue" aria-label="Remove from queue" onClick={onRemove}>
        <TrashIcon />
      </button>
    </div>
  );
}
