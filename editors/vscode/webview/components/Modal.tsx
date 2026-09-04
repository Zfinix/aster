import type { ReactNode } from "react";
import { useLayer } from "../lib/layer";

/**
 * Anything that covers the thread. Escape and a click on the dimmed ground
 * close it when it can be closed; a prompt that must be answered leaves
 * `onClose` out and keeps both.
 */
export function Modal({
  label,
  className,
  align = "top",
  onClose,
  children,
}: {
  label: string;
  className: string;
  /** Top for something typed into, centre for something read, bottom for a
   *  question over the turn that asked it. */
  align?: "top" | "center" | "bottom";
  onClose?: () => void;
  children: ReactNode;
}) {
  useLayer(onClose);
  return (
    <div
      className="modal-overlay"
      data-align={align}
      onMouseDown={(e) => e.target === e.currentTarget && onClose?.()}
    >
      <div className={`modal ${className}`} role="dialog" aria-modal="true" aria-label={label}>
        {children}
      </div>
    </div>
  );
}
