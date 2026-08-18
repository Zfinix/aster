import { useState } from "react";
import { ChevronIcon } from "./icons";

/** The model's thinking for a turn: a muted summary line that expands into the
 *  raw text. Collapsed by default, since reasoning usually runs longer than the
 *  answer it produced. The summary reflects the block's state: a live token
 *  count while it streams, a duration once it finishes, and a bare "Thought"
 *  when neither is known. */
export function ReasoningPanel({
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
      <button
        className="reasoning-head"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
      >
        <span className="reasoning-summary">{label}</span>
        <span className="reasoning-caret" data-open={open}>
          <ChevronIcon size={12} />
        </span>
      </button>
      {open && <div className="reasoning-body">{body}</div>}
    </div>
  );
}
