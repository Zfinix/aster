import type { ConfigPaths, ConfigScope } from "../../src/protocol";

/** Which file writes land in. The path under it is the whole point: `aster
 *  config set` picks a file by rule, and a settings page that hid which one
 *  would be a worse way to edit config than the CLI. */
export function ScopeSwitcher({
  scope,
  paths,
  hasWorkspace,
  onChange,
  onOpenFile,
}: {
  scope: ConfigScope;
  paths: ConfigPaths | null;
  hasWorkspace: boolean;
  onChange: (next: ConfigScope) => void;
  onOpenFile: () => void;
}) {
  const target =
    scope === "global"
      ? (paths?.global ?? "~/.aster/aster.yaml")
      : (paths?.project ?? paths?.project_default ?? "aster.yaml");
  const exists = scope === "global" ? paths?.global_exists : paths?.project_exists;

  return (
    <div className="set-scope">
      <div className="set-segmented" role="radiogroup" aria-label="Settings scope">
        <button
          type="button"
          role="radio"
          aria-checked={scope === "global"}
          className={scope === "global" ? "set-segment on" : "set-segment"}
          onClick={() => onChange("global")}
        >
          User
        </button>
        <button
          type="button"
          role="radio"
          aria-checked={scope === "local"}
          className={scope === "local" ? "set-segment on" : "set-segment"}
          onClick={() => onChange("local")}
          disabled={!hasWorkspace}
          title={hasWorkspace ? undefined : "Open a folder to edit workspace settings"}
        >
          Workspace
        </button>
      </div>
      <button type="button" className="set-path mono" onClick={onOpenFile} title="Open this file">
        {target}
        {exists === false && <span className="set-path-note">not created yet</span>}
      </button>
    </div>
  );
}
