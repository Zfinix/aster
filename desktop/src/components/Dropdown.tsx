import { useCallback, useEffect, useState, type ReactNode } from "react";
import { useDismiss } from "./chrome";
import { PickerRow } from "./PickerRow";
import { Popup, type PopAlign, type PopDir } from "./Popup";

export interface DropdownOption {
  value: string;
  label: ReactNode;
  detail?: ReactNode;
  icon?: ReactNode;
  danger?: boolean;
}

/** A small, outside-click-aware menu anchored to a trigger. */
export function Dropdown({
  trigger,
  triggerClass = "ghost",
  options,
  value,
  onSelect,
  onOpenChange,
  dir = "up",
  align = "left",
  width,
  pickerClass,
  label,
  title,
}: {
  trigger: (open: boolean) => ReactNode;
  triggerClass?: string;
  options: DropdownOption[];
  value?: string;
  onSelect: (value: string) => void;
  onOpenChange?: (open: boolean) => void;
  dir?: PopDir;
  align?: PopAlign;
  width?: "wide" | "fill";
  pickerClass?: string;
  label?: string;
  title?: string;
}) {
  const [open, setOpen] = useState(false);
  const close = useCallback(() => setOpen(false), []);
  const wrapRef = useDismiss(open, close);

  useEffect(() => {
    onOpenChange?.(open);
  }, [open, onOpenChange]);

  return (
    <div className="pop-wrap" ref={wrapRef}>
      <button
        type="button"
        className={triggerClass}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={label}
        title={title}
        onClick={() => setOpen((o) => !o)}
      >
        {trigger(open)}
      </button>
      {open && (
        <Popup dir={dir} align={align}>
          <div className={`picker ${pickerClass ?? ""}`} data-width={width} role="menu">
            {options.map((o) => (
              <PickerRow
                key={o.value}
                active={value === o.value}
                danger={o.danger}
                icon={o.icon}
                label={o.label}
                detail={o.detail}
                value={o.value}
                onSelect={() => {
                  onSelect(o.value);
                  setOpen(false);
                }}
              />
            ))}
          </div>
        </Popup>
      )}
    </div>
  );
}
