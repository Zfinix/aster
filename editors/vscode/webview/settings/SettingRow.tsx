import type { ConfigKey, ConfigScope, ConfigValue, Provider } from "../../src/protocol";
import { Control } from "./Control";
import { ResetIcon, WarnIcon } from "./icons";
import { shownValue } from "./sections";

/** One key. The control edits the selected scope alone, so the row has to say
 *  when what you see is not what this scope holds: a value inherited from the
 *  other file, or a shell variable outranking both. */
export function SettingRow({
  keyRow,
  scope,
  models,
  providers,
  onSet,
  onUnset,
  error,
  index = 0,
}: {
  keyRow: ConfigKey;
  scope: ConfigScope;
  models: string[];
  providers: Provider[];
  onSet: (value: Exclude<ConfigValue, null>) => void;
  onUnset: () => void;
  error?: string;
  index?: number;
}) {
  const own = keyRow.scopes[scope];
  const other: ConfigScope = scope === "global" ? "local" : "global";
  const shown = shownValue(keyRow, own);
  const set = own !== null;
  const overridden = set && keyRow.scopes[other] !== null && scope === "global";
  const inherited = !set && keyRow.source !== "default";

  return (
    <div
      className={set ? "set-row set" : "set-row"}
      data-layout={keyRow.kind === "list" ? "stack" : undefined}
      style={{ "--i": index } as React.CSSProperties}
    >
      <div className="set-row-text">
        <div className="set-row-head">
          <span className="set-row-label" title={keyRow.key}>
            {keyRow.label}
          </span>
          {set && (
            <button
              type="button"
              className="set-reset"
              title={`Clear ${keyRow.key} from the ${scope === "global" ? "user" : "workspace"} config`}
              aria-label={`Reset ${keyRow.label}`}
              onClick={onUnset}
            >
              <ResetIcon />
            </button>
          )}
        </div>
        <p className="set-row-help">{keyRow.help}</p>
        {keyRow.shadowed && (
          <p className="set-row-note warn">
            <WarnIcon />
            <span>
              <code>{keyRow.shadowed}</code> is set in the environment and wins over both files.
            </span>
          </p>
        )}
        {overridden && <p className="set-row-note">The workspace config overrides this.</p>}
        {/* Only worth a line when a file supplies it: "inherited from the
            default" is what every untouched row would say. */}
        {inherited && <p className="set-row-note">From {keyRow.source}</p>}
        {error && <p className="set-row-note error">{error}</p>}
      </div>
      <div className="set-row-control">
        <Control
          keyRow={keyRow}
          shown={shown}
          models={models}
          providers={providers}
          onCommit={onSet}
          onUnset={onUnset}
        />
      </div>
    </div>
  );
}
