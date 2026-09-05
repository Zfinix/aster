import type { ApiKey } from "../../src/protocol";
import { SecretField } from "./controls/SecretField";
import { CloseIcon } from "./icons";
import { StatusLine } from "./StatusLine";

/** The groups the CLI listed, in the order it listed them, so a new provider
 *  group shows up without this file being told about it. */
function groups(apiKeys: ApiKey[]): string[] {
  return [...new Set(apiKeys.map((key) => key.group))];
}

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
      {groups(apiKeys).map((group) => {
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
  return (
    <div className={keyRow.set ? "set-row set" : "set-row"}>
      <div className="set-row-text">
        <div className="set-row-head">
          <span className="set-row-label">{keyRow.provider}</span>
          <code className="set-tag">{keyRow.var}</code>
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
        {keyRow.help && <p className="set-row-help">{keyRow.help}</p>}
        <StatusLine set={keyRow.set} source={sourceLabel(keyRow.source)} />
        {error && <p className="set-row-note error">{error}</p>}
      </div>
      <div className="set-row-control">
        <SecretField
          label={`${keyRow.provider} key`}
          stored={keyRow.set}
          masked={keyRow.masked}
          revealed={shown}
          placeholder="Add key…"
          onCommit={(value) => onSet(keyRow.var, value)}
          onReveal={() => onReveal(keyRow.var)}
          onHide={() => onHide(keyRow.var)}
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
