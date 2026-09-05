import { useEffect, type ReactNode } from "react";

/** Anything that covers the app: Escape and a click on the dimmed ground close it. */
export function Modal({
  label,
  className,
  align = "top",
  onClose,
  children,
}: {
  label: string;
  className: string;
  align?: "top" | "center";
  onClose?: () => void;
  children: ReactNode;
}) {
  useEffect(() => {
    if (!onClose) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

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
