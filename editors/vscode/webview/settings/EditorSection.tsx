import type { ConfigValue, EditorSettings } from "../../src/protocol";
import { ChipList } from "./controls/ChipList";
import { NumberInput } from "./controls/NumberInput";
import { TextInput } from "./controls/TextInput";
import { Toggle } from "./controls/Toggle";

/** The four settings VS Code keeps rather than `aster.yaml`. They are written
 *  through the editor's own configuration API, so the scope switcher above does
 *  not apply to them and this page says so. */
export function EditorSection({
  editor,
  onSet,
}: {
  editor: EditorSettings;
  onSet: (key: keyof EditorSettings, value: ConfigValue) => void;
}) {
  return (
    <div className="set-card">
      <div className="set-row">
        <div className="set-row-text">
          <div className="set-row-head">
            <span className="set-row-label">Binary path</span>
          </div>
          <p className="set-row-help">Where the `aster` executable lives. Defaults to PATH.</p>
          <p className="set-row-key mono">aster.binaryPath</p>
        </div>
        <div className="set-row-control">
          <TextInput
            value={editor.binaryPath}
            label="Binary path"
            placeholder="aster"
            mono
            onCommit={(next) => onSet("binaryPath", next)}
          />
        </div>
      </div>

      <div className="set-row">
        <div className="set-row-text">
          <div className="set-row-head">
            <span className="set-row-label">Problems tab</span>
          </div>
          <p className="set-row-help">
            Also report findings as diagnostics. Off by default so a review does not bury real
            compiler and linter problems.
          </p>
          <p className="set-row-key mono">aster.publishDiagnostics</p>
        </div>
        <div className="set-row-control">
          <Toggle
            checked={editor.publishDiagnostics}
            label="Publish diagnostics"
            onChange={(next) => onSet("publishDiagnostics", next)}
          />
        </div>
      </div>

      <div className="set-row">
        <div className="set-row-text">
          <div className="set-row-head">
            <span className="set-row-label">Minimum confidence</span>
          </div>
          <p className="set-row-help">
            Drop findings below this. Falls back to the Code review setting when unset.
          </p>
          <p className="set-row-key mono">aster.minConfidence</p>
        </div>
        <div className="set-row-control">
          <NumberInput
            value={editor.minConfidence}
            label="Minimum confidence"
            unit="none"
            onCommit={(next) => onSet("minConfidence", next)}
          />
        </div>
      </div>

      <div className="set-row">
        <div className="set-row-text">
          <div className="set-row-head">
            <span className="set-row-label">Extra arguments</span>
          </div>
          <p className="set-row-help">Appended to every `aster review` this extension runs.</p>
          <p className="set-row-key mono">aster.extraArgs</p>
        </div>
        <div className="set-row-control">
          <ChipList
            items={editor.extraArgs}
            label="Extra arguments"
            placeholder="--no-index"
            onChange={(next) => onSet("extraArgs", next)}
          />
        </div>
      </div>
    </div>
  );
}
