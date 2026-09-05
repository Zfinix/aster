import { useState } from "react";
import { EyeIcon } from "../icons";

/** The stored value's tail, behind dots: what a filled password field looks
 *  like, with the part that tells two keys apart left readable. */
function dotted(masked: string | null | undefined): string {
  return "••••••••" + (masked ?? "").replace(/^[….]+/, "");
}

/** A secret shown the way it is stored: dots with the tail, the eye revealing
 *  it in place. Focusing empties the field for a new value; the draft is
 *  hidden while typed, the eye peeks at it, and blur or Enter stores it. */
export function SecretField({
  label,
  stored,
  masked,
  revealed,
  placeholder,
  onCommit,
  onReveal,
  onHide,
}: {
  label: string;
  stored: boolean;
  masked?: string | null;
  revealed?: string;
  placeholder: string;
  onCommit: (next: string) => void;
  onReveal: () => void;
  onHide: () => void;
}) {
  const [draft, setDraft] = useState("");
  const [focused, setFocused] = useState(false);
  const [peek, setPeek] = useState(false);
  const editing = focused || draft.length > 0;
  const value = editing ? draft : stored ? (revealed ?? dotted(masked)) : "";
  const eyeOn = editing ? peek : Boolean(revealed);
  const eye = editing ? draft.length > 0 : stored;

  const commit = () => {
    const next = draft.trim();
    if (next) onCommit(next);
    setDraft("");
    setPeek(false);
  };

  return (
    <div className="set-secret" data-editing={editing}>
      <input
        type={editing && !peek ? "password" : "text"}
        className="set-input mono"
        aria-label={label}
        placeholder={stored ? "Paste a new key…" : placeholder}
        value={value}
        autoComplete="off"
        spellCheck={false}
        onFocus={() => setFocused(true)}
        onBlur={() => {
          setFocused(false);
          commit();
        }}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.currentTarget.blur();
          } else if (e.key === "Escape") {
            setDraft("");
            setPeek(false);
            e.currentTarget.blur();
          }
        }}
      />
      {eye && (
        <button
          type="button"
          className="set-secret-eye"
          aria-label={eyeOn ? `Hide ${label}` : `Show ${label}`}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => {
            if (editing) setPeek(!peek);
            else if (revealed) onHide();
            else onReveal();
          }}
        >
          <EyeIcon off={eyeOn} />
        </button>
      )}
    </div>
  );
}
