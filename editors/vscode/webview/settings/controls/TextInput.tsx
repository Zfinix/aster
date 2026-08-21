import { useEffect, useState } from "react";

/** Commits on blur and on Enter rather than per keystroke: every commit is a
 *  CLI write, and one per character would race the re-read that follows. */
export function TextInput({
  value,
  label,
  placeholder,
  mono,
  onCommit,
}: {
  value: string;
  label: string;
  placeholder?: string;
  mono?: boolean;
  onCommit: (next: string) => void;
}) {
  const [draft, setDraft] = useState(value);

  // A write the host normalized, or an edit made elsewhere, replaces the draft
  // unless it is the one being typed.
  useEffect(() => setDraft(value), [value]);

  const commit = () => {
    if (draft !== value) onCommit(draft);
  };

  return (
    <input
      type="text"
      className={mono ? "set-input mono" : "set-input"}
      aria-label={label}
      placeholder={placeholder}
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.currentTarget.blur();
        } else if (e.key === "Escape") {
          setDraft(value);
          e.currentTarget.blur();
        }
      }}
    />
  );
}
