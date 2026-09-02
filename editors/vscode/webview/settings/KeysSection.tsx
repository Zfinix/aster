import type { ApiKey } from "../../src/protocol";
import { SecretInput } from "./controls/SecretInput";
import { CloseIcon, EyeIcon } from "./icons";

const GROUPS = ["Model", "Web tools"];

/** Every key Aster reads, one row each: what is stored (masked, revealable),
 *  where it came from, and a write-only field to set or replace it. */
export function KeysSection({
  apiKeys,
  errors,
  revealed,
  onSet,
  onUnset,
  onReveal,
  onHide,
}: {
  apiKeys: ApiKey[];
  errors: Record<string, string>;
  revealed: Record<string, string>;
  onSet: (name: string, value: string) => void;
  onUnset: (name: string) => void;
  onReveal: (name: string) => void;
  onHide: (name: string) => void;
}) {
  return (
    <>
      {GROUPS.map((group) => {
        const rows = apiKeys.filter((key) => key.group === group);
        if (rows.length === 0) return null;
        return (
          <section key={group}>
            <h2 className="set-group-title">{group}</h2>
            <div className="set-card">
              {rows.map((key) => (
                <KeyRow
                  key={key.var}
                  keyRow={key}
                  error={errors[key.var]}
                  shown={revealed[key.var]}
                  onSet={onSet}
                  onUnset={onUnset}
                  onReveal={onReveal}
                  onHide={onHide}
                />
              ))}
            </div>
          </section>
        );
      })}
    </>
  );
}

function KeyRow({
  keyRow,
  error,
  shown,
  onSet,
  onUnset,
  onReveal,
  onHide,
}: {
  keyRow: ApiKey;
  error?: string;
  shown?: string;
  onSet: (name: string, value: string) => void;
  onUnset: (name: string) => void;
  onReveal: (name: string) => void;
  onHide: (name: string) => void;
}) {
  const state = keyRow.set
    ? `${shown ?? keyRow.masked ?? "set"} · ${sourceLabel(keyRow.source)}`
    : "not set";
  return (
    <div className="set-row">
      <div className="set-row-text">
        <div className="set-row-head">
          <span className="set-row-label">{keyRow.provider}</span>
          {keyRow.set && (
            <button
              type="button"
              className="set-reset"
              title={`Clear ${keyRow.var}`}
              onClick={() => onUnset(keyRow.var)}
            >
              <CloseIcon />
            </button>
          )}
        </div>
        <p className="set-row-key mono">
          {keyRow.var} · {state}
        </p>
        {!keyRow.set && keyRow.help && <p className="set-row-help">{keyRow.help}</p>}
        {error && <p className="set-row-note error">{error}</p>}
      </div>
      <div className="set-row-control set-key-control">
        {keyRow.set && (
          <button
            type="button"
            className="set-eye"
            title={shown ? "Hide the key" : "Show the key"}
            onClick={() => (shown ? onHide(keyRow.var) : onReveal(keyRow.var))}
          >
            <EyeIcon off={Boolean(shown)} />
          </button>
        )}
        <SecretInput
          label={`Set ${keyRow.var}`}
          placeholder={keyRow.set ? "Replace key…" : "Add key…"}
          onCommit={(value) => onSet(keyRow.var, value)}
        />
      </div>
    </div>
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
