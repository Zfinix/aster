import type { ReactNode } from "react";
import { CheckIcon } from "./icons";

/** One row of a popup menu, shared by every picker so they read as one. */
export function PickerRow({
  active,
  danger,
  icon,
  label,
  detail,
  value,
  title,
  onSelect,
}: {
  active?: boolean;
  danger?: boolean;
  icon?: ReactNode;
  label: ReactNode;
  detail?: ReactNode;
  value?: string;
  title?: string;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      className="picker-row"
      data-active={active || undefined}
      data-danger={danger || undefined}
      data-value={value}
      title={title}
      onClick={onSelect}
    >
      {icon}
      <span className="picker-body">
        <span className="picker-label">{label}</span>
        {detail && <span className="picker-detail">{detail}</span>}
      </span>
      {active && (
        <span className="picker-check">
          <CheckIcon />
        </span>
      )}
    </button>
  );
}
