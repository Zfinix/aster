import type { ReactNode } from "react";

export type PopDir = "up" | "down";
export type PopAlign = "left" | "right";

/** The slot a menu opens into, hung off the trigger it belongs to. */
export function Popup({
  dir = "up",
  align = "left",
  children,
}: {
  dir?: PopDir;
  align?: PopAlign;
  children: ReactNode;
}) {
  return (
    <div className="pop" data-dir={dir} data-align={align}>
      {children}
    </div>
  );
}
