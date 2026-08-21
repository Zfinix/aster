import { useEffect, useId, useState } from "react";
import { ChevronIcon } from "../icons";

/** A menu that still takes a typed value. Model ids and endpoints are open sets
 *  the catalogue only partly covers, so offering the known ones must not stop
 *  anyone entering one it has never heard of. */
export function Combo({
  value,
  options,
  label,
  placeholder,
  onCommit,
}: {
  value: string;
  options: { id: string; detail?: string }[];
  label: string;
  placeholder?: string;
  onCommit: (next: string) => void;
}) {
  const listId = useId();
  const [draft, setDraft] = useState(value);

  useEffect(() => setDraft(value), [value]);

  const commit = () => {
    const next = draft.trim();
    if (next && next !== value) onCommit(next);
    else if (!next) setDraft(value);
  };

  return (
    <div className="set-combo">
      <input
        type="text"
        className="set-input mono"
        list={listId}
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
      <ChevronIcon />
      <datalist id={listId}>
        {options.map((option) => (
          <option key={option.id} value={option.id} label={option.detail} />
        ))}
      </datalist>
    </div>
  );
}
