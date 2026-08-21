import { useEffect, useState } from "react";
import type { ConfigUnit } from "../../../src/protocol";

const SUFFIX: Record<ConfigUnit, string> = {
  none: "",
  seconds: "s",
  chars: "chars",
  bytes: "bytes",
  tokens: "tokens",
  percent: "%",
};

export function NumberInput({
  value,
  label,
  unit,
  onCommit,
}: {
  value: number | null;
  label: string;
  unit: ConfigUnit;
  onCommit: (next: number) => void;
}) {
  const text = value === null ? "" : String(value);
  const [draft, setDraft] = useState(text);

  useEffect(() => setDraft(text), [text]);

  const commit = () => {
    const parsed = Number.parseFloat(draft);
    // A blank or unparseable box reverts rather than writing NaN; clearing a
    // key is what the row's own reset button is for.
    if (!Number.isFinite(parsed)) {
      setDraft(text);
      return;
    }
    if (parsed !== value) onCommit(parsed);
  };

  const suffix = SUFFIX[unit];
  return (
    <div className="set-number">
      <input
        type="text"
        inputMode="decimal"
        className="set-input"
        aria-label={label}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.currentTarget.blur();
          } else if (e.key === "Escape") {
            setDraft(text);
            e.currentTarget.blur();
          }
        }}
      />
      {suffix && <span className="set-unit">{suffix}</span>}
    </div>
  );
}
