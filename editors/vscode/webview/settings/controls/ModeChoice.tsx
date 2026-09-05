import type { PermissionMode } from "../../../src/protocol";
import { permissionDanger, permissionDetail, permissionLabel } from "../../components/ApprovalPicker";
import { Segmented } from "./Segmented";

/** The mode dial with the chat panel's own words for each stop, so "yolo"
 *  is explained here the same way it is where it gets picked. */
export function ModeChoice({
  choices,
  value,
  label,
  onChange,
}: {
  choices: string[];
  value: string;
  label: string;
  onChange: (next: string) => void;
}) {
  const mode = value as PermissionMode;
  const danger = permissionDanger(mode);
  return (
    <div className="set-mode">
      <Segmented
        options={choices.map((choice) => ({
          value: choice,
          label: permissionLabel(choice as PermissionMode),
          danger: permissionDanger(choice as PermissionMode),
        }))}
        value={value}
        label={label}
        onChange={onChange}
      />
      <p key={value} className="set-mode-detail" data-danger={danger}>
        {permissionDetail(mode)}
      </p>
    </div>
  );
}
