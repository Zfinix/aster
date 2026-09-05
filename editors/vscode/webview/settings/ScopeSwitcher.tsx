import type { ConfigPaths, ConfigScope } from "../../src/protocol";
import { Segmented } from "./controls/Segmented";
import { OpenIcon } from "./icons";
import { homeFrom, shortPath } from "./sections";

/** Which file writes land in. The path under it is the whole point: `aster
 *  config set` picks a file by rule, and a settings page that hid which one
 *  would be a worse way to edit config than the CLI. */
export function ScopeSwitcher({
  scope,
  paths,
  workspaceRoot,
  hasWorkspace,
  onChange,
  onOpenFile,
}: {
  scope: ConfigScope;
  paths: ConfigPaths | null;
  workspaceRoot: string | null;
  hasWorkspace: boolean;
  onChange: (next: ConfigScope) => void;
  onOpenFile: () => void;
}) {
  const target =
    scope === "global"
      ? (paths?.global ?? "~/.aster/aster.yaml")
      : (paths?.project ?? paths?.project_default ?? "aster.yaml");
  const exists = scope === "global" ? paths?.global_exists : paths?.project_exists;
  const shown = shortPath(target, homeFrom(paths?.global), workspaceRoot);

  return (
    <div className="set-scope">
      <Segmented
        label="Settings scope"
        value={scope}
        onChange={(next) => onChange(next as ConfigScope)}
        options={[
          { value: "global", label: "User" },
          {
            value: "local",
            label: "Workspace",
            disabled: !hasWorkspace,
            title: hasWorkspace ? undefined : "Open a folder to edit workspace settings",
          },
        ]}
      />
      <button type="button" className="set-path" onClick={onOpenFile} title={`Open ${target}`}>
        <OpenIcon />
        {/* The text truncates from the left by rendering right-to-left, which
            would carry a leading "/" or "~" to the far end without this mark. */}
        <span className="set-path-text">
          {"‎"}
          {shown}
        </span>
        {exists === false && <span className="set-path-note">not created yet</span>}
      </button>
    </div>
  );
}
