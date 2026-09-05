import { useState } from "react";
import { ChevronIcon } from "./icons";

/** The model's thinking, collapsed by default since it usually runs longer
 *  than the answer. */
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
      ? `Thinking, ${tokens} tokens`
      : "Thinking";

  return (
    <div className="reasoning">
      <button type="button" className="reasoning-head" onClick={() => setOpen((o) => !o)} aria-expanded={open}>
        <span className={done ? undefined : "shimmer"}>{label}</span>
        <ChevronIcon open={open} />
      </button>
      {open && <div className="reasoning-body">{body}</div>}
    </div>
  );
}
