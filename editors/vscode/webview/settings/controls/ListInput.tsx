import { useState } from "react";
import { CloseIcon, PlusIcon } from "../icons";

const RULE = /^([A-Za-z_][\w-]*)\((.*)\)$/s;

/** Permission rules and globs are lists, and a comma-separated text box hides
 *  both how many there are and where one ends, so each entry gets its own
 *  line and its own way out, with a rule's tool set apart from its pattern. */
export function ListInput({
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
  const pending = draft.trim();

  const add = () => {
    if (!pending || items.includes(pending)) {
      setDraft("");
      return;
    }
    onChange([...items, pending]);
    setDraft("");
  };

  return (
    <div className="set-list">
      {items.length > 0 && (
        <ul className="set-list-items">
          {items.map((item) => {
            const rule = RULE.exec(item);
            return (
              <li key={item} className="set-list-item">
                {rule ? (
                  <>
                    <span className="set-list-tool">{rule[1]}</span>
                    <span className="set-list-text mono">{rule[2]}</span>
                  </>
                ) : (
                  <span className="set-list-text mono">{item}</span>
                )}
                <button
                  type="button"
                  className="set-list-x"
                  aria-label={`Remove ${item}`}
                  onClick={() => onChange(items.filter((entry) => entry !== item))}
                >
                  <CloseIcon />
                </button>
              </li>
            );
          })}
        </ul>
      )}
      <div className="set-list-add">
        <PlusIcon />
        <input
          type="text"
          className="mono"
          aria-label={`Add to ${label}`}
          placeholder={placeholder}
          spellCheck={false}
          autoComplete="off"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={add}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              add();
            } else if (e.key === "Escape") {
              setDraft("");
            }
          }}
        />
        {pending && (
          <kbd className="set-kbd" aria-hidden="true">
            Enter
          </kbd>
        )}
      </div>
    </div>
  );
}
