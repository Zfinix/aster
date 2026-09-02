import { useState } from "react";

/** A write-only key field: commits on Enter or blur, then clears its draft so
 *  the stored value never round-trips into the input. */
export function SecretInput({
  label,
  placeholder,
  onCommit,
}: {
  label: string;
  placeholder: string;
  onCommit: (next: string) => void;
}) {
  const [draft, setDraft] = useState("");

  const commit = () => {
    const next = draft.trim();
    if (next) onCommit(next);
    setDraft("");
  };

  return (
    <input
      type="password"
      className="set-input mono"
      aria-label={label}
      placeholder={placeholder}
      value={draft}
      autoComplete="off"
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.currentTarget.blur();
        } else if (e.key === "Escape") {
          setDraft("");
          e.currentTarget.blur();
        }
      }}
    />
  );
}
