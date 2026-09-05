import type { ReactNode } from "react";

/** One setting in a card: what it is on the left, the control on the right. */
export function SettingsRow({ label, help, children }: { label: ReactNode; help?: ReactNode; children: ReactNode }) {
  return (
    <div className="settings-row">
      <div className="settings-row-text">
        <span className="settings-row-label">{label}</span>
        {help && <span className="settings-row-help">{help}</span>}
      </div>
      <div className="settings-row-control">{children}</div>
    </div>
  );
}
