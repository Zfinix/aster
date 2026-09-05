import { useState } from "react";
import type { EnvVar } from "../../src/protocol";
import { SecretField } from "./controls/SecretField";
import { Toggle } from "./controls/Toggle";
import { CloseIcon } from "./icons";
import { StatusLine } from "./StatusLine";
import { envGroups } from "./sections";

/** Every ASTER_* variable the CLI reads, one row each: its live value, which
 *  layer supplies it, and a field to set or clear it in the scoped .env. */
export function EnvSection({
  vars,
  errors,
  revealed,
  onSet,
  onUnset,
  onReveal,
  onHide,
}: {
  vars: EnvVar[];
  errors: Record<string, string>;
  revealed: Record<string, string>;
  onSet: (name: string, value: string) => void;
  onUnset: (name: string) => void;
  onReveal: (name: string) => void;
  onHide: (name: string) => void;
}) {
  return (
    <>
      {envGroups(vars).map(({ group, vars: rows }) => (
        <section key={group}>
          <h2 className="set-group-title">{group}</h2>
          <div className="set-card">
            {rows.map((v) => (
              <EnvRow
                key={v.var}
                row={v}
                error={errors[v.var]}
                shown={revealed[v.var]}
                onSet={onSet}
                onUnset={onUnset}
                onReveal={onReveal}
                onHide={onHide}
              />
            ))}
          </div>
        </section>
      ))}
    </>
  );
}

function EnvRow({
  row,
  error,
  shown,
  onSet,
  onUnset,
  onReveal,
  onHide,
}: {
  row: EnvVar;
  error?: string;
  shown?: string;
  onSet: (name: string, value: string) => void;
  onUnset: (name: string) => void;
  onReveal: (name: string) => void;
  onHide: (name: string) => void;
}) {
  return (
    <div className={row.set ? "set-row set" : "set-row"}>
      <div className="set-row-text">
        <div className="set-row-head">
          <span className="set-row-label mono" title={row.var}>
            {row.var}
          </span>
          {row.set && (
            <button
              type="button"
              className="set-reset"
              title={`Clear ${row.var}`}
              onClick={() => onUnset(row.var)}
            >
              <CloseIcon />
            </button>
          )}
        </div>
        <p className="set-row-help">{row.help}</p>
        <StatusLine
          set={row.set}
          value={row.secret ? undefined : (row.value ?? undefined)}
          source={sourceLabel(row.source)}
        />
        {error && <p className="set-row-note error">{error}</p>}
      </div>
      <div className="set-row-control">
        <EnvControl
          row={row}
          shown={shown}
          onSet={onSet}
          onUnset={onUnset}
          onReveal={onReveal}
          onHide={onHide}
        />
      </div>
    </div>
  );
}

function EnvControl({
  row,
  shown,
  onSet,
  onUnset,
  onReveal,
  onHide,
}: {
  row: EnvVar;
  shown?: string;
  onSet: (name: string, value: string) => void;
  onUnset: (name: string) => void;
  onReveal: (name: string) => void;
  onHide: (name: string) => void;
}) {
  const [draft, setDraft] = useState("");
  const [invalid, setInvalid] = useState(false);

  const commit = () => {
    const next = draft.trim();
    if (!next) return;
    if (row.kind === "number" && !Number.isFinite(Number.parseFloat(next))) {
      setInvalid(true);
      return;
    }
    if (row.kind === "json") {
      try {
        JSON.parse(next);
      } catch {
        setInvalid(true);
        return;
      }
    }
    setInvalid(false);
    setDraft("");
    onSet(row.var, next);
  };

  if (row.kind === "bool") {
    return (
      <Toggle
        checked={row.set}
        label={row.var}
        onChange={(on) => (on ? onSet(row.var, "1") : onUnset(row.var))}
      />
    );
  }

  if (row.secret) {
    return (
      <SecretField
        label={row.var}
        stored={row.set}
        masked={row.masked}
        revealed={shown}
        placeholder="Add value…"
        onCommit={(value) => onSet(row.var, value)}
        onReveal={() => onReveal(row.var)}
        onHide={() => onHide(row.var)}
      />
    );
  }

  return (
    <input
      type="text"
      className={invalid ? "set-input mono invalid" : "set-input mono"}
      aria-label={`Set ${row.var}`}
      placeholder={row.set ? "Replace value…" : "Add value…"}
      value={draft}
      autoComplete="off"
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.currentTarget.blur();
        } else if (e.key === "Escape") {
          setDraft("");
          setInvalid(false);
          e.currentTarget.blur();
        }
      }}
    />
  );
}

function sourceLabel(source: string): string {
  switch (source) {
    case "shell":
      return "your shell";
    case "local":
      return "this repo's .env";
    case "global":
      return "~/.aster/.env";
    default:
      return source;
  }
}
