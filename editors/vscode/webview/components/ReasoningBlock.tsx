import { useState } from "react";

/** One round's thinking, folded to a single line until the reader opens it.
 *  The summary shows a live token count while it streams, a duration once it
 *  finishes, and a bare "Thought" when neither is known. */
export function ReasoningBlock({
  text,
  tokens,
  durationMs,
  done,
}: {
  text: string;
  tokens?: number;
  durationMs?: number;
  done?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const body = text.trim();

  if (!body) return null;

  const label = done
    ? durationMs != null
      ? `Thought for ${Math.max(1, Math.round(durationMs / 1000))}s`
      : "Thought"
    : "Thinking…";

  const title = open
    ? "Hide the thinking"
    : done
      ? "Show the thinking"
      : tokens
        ? `Thinking… ${tokens.toLocaleString()} tokens`
        : "Thinking…";

  return (
    <div className="reasoning" data-open={open} data-done={done === true}>
      <span className="reasoning-node" aria-hidden="true">
        <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor">
          <path d="M8 2l1.5 4.5L14 8l-4.5 1.5L8 14l-1.5-4.5L2 8l4.5-1.5z" />
        </svg>
      </span>
      <button
        className="reasoning-head"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        title={title}
      >
        <span className="reasoning-label">{label}</span>
      </button>
      {open && <div className="reasoning-body">{body}</div>}
    </div>
  );
}
