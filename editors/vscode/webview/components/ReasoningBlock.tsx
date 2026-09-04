import { useState } from "react";

/** One round's thinking, collapsed by default since it usually runs longer than
 *  the answer. The summary shows a live token count while it streams, a
 *  duration once it finishes, and a bare "Thought" when neither is known. */
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
    : tokens != null
      ? `Thinking... ${tokens} tokens`
      : "Thought";

  return (
    <div className="reasoning">
      <button className="reasoning-head" onClick={() => setOpen((o) => !o)} aria-expanded={open}>
        {label}
      </button>
      {open && <div className="reasoning-body">{body}</div>}
    </div>
  );
}
