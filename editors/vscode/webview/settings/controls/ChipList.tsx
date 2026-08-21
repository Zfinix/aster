import { useState } from "react";
import { CloseIcon } from "../icons";

/** Permission rules and globs are lists, and a comma-separated text box hides
 *  both how many there are and where one ends, so each entry gets its own chip
 *  and its own way out. */
export function ChipList({
  items,
  label,
  placeholder,
  onChange,
}: {
  items: string[];
  label: string;
  placeholder: string;
  onChange: (next: string[]) => void;
}) {
  const [draft, setDraft] = useState("");

  const add = () => {
    const entry = draft.trim();
    if (!entry || items.includes(entry)) {
      setDraft("");
      return;
    }
    onChange([...items, entry]);
    setDraft("");
  };

  return (
    <div className="set-chips">
      {items.length > 0 && (
        <ul className="set-chip-list">
          {items.map((item) => (
            <li key={item} className="set-chip">
              <span className="mono">{item}</span>
              <button
                type="button"
                className="set-chip-x"
                aria-label={`Remove ${item}`}
                onClick={() => onChange(items.filter((entry) => entry !== item))}
              >
                <CloseIcon />
              </button>
            </li>
          ))}
        </ul>
      )}
      <input
        type="text"
        className="set-input mono"
        aria-label={`Add to ${label}`}
        placeholder={placeholder}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={add}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            add();
          } else if (e.key === "Escape") {
            setDraft("");
          } else if (e.key === "Backspace" && !draft && items.length > 0) {
            onChange(items.slice(0, -1));
          }
        }}
      />
    </div>
  );
}
