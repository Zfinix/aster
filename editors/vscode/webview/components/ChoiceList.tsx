import { useEffect, useRef, useState } from "react";

export interface Choice {
  key: string;
  label: string;
  title?: string;
  checked: boolean;
  onSelect: () => void;
}

/** A list of one-line options with a tick on the one in force, filterable by a
 *  search line like the model picker's. The panels that need icons or a second
 *  line render their own rows. */
export function ChoiceList({
  label,
  choices,
  empty,
  placeholder,
}: {
  label: string;
  choices: Choice[];
  empty?: string;
  placeholder?: string;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const typed = query.trim().toLowerCase();
  const visible = typed
    ? choices.filter((c) => c.label.toLowerCase().includes(typed) || c.key.toLowerCase().includes(typed))
    : choices;

  return (
    <div className="cmd" role="menu" aria-label={label}>
      <input
        ref={inputRef}
        className="cmd-filter"
        placeholder={placeholder}
        spellCheck={false}
        value={query}
        onChange={(e) => setQuery(e.currentTarget.value)}
      />
      <div className="cmd-list" role="listbox">
        {visible.length === 0 && empty && <div className="cmd-note">{empty}</div>}
        {visible.map((choice) => (
          <button
            key={choice.key}
            className="picker-row"
            role="menuitemradio"
            aria-checked={choice.checked}
            title={choice.title}
            onClick={choice.onSelect}
          >
            <span className="picker-body">
              <span className="picker-label">{choice.label}</span>
            </span>
            {choice.checked && <span className="picker-check">✓</span>}
          </button>
        ))}
      </div>
    </div>
  );
}
