import type { ReactNode } from "react";
import { SidebarIcon } from "./icons";

/** The bar above the view: title first, actions at the trailing edge. With the
 *  sidebar closed it also holds the button that brings it back and clears the
 *  window controls. */
export function Toolbar({
  inset,
  onExpand,
  title,
  sub,
  children,
}: {
  inset: boolean;
  onExpand: () => void;
  title?: string | null;
  sub?: string | null;
  children?: ReactNode;
}) {
  return (
    <header className="toolbar" data-inset={inset} data-tauri-drag-region>
      {inset && (
        <button
          type="button"
          className="ghost icon-action"
          aria-label="Show sidebar"
          title="Show sidebar"
          onClick={onExpand}
        >
          <SidebarIcon />
        </button>
      )}
      {title && (
        <span className="toolbar-title" data-tauri-drag-region>
          {title}
        </span>
      )}
      {sub && (
        <span className="toolbar-sub" data-tauri-drag-region>
          {sub}
        </span>
      )}
      <div className="toolbar-actions">{children}</div>
    </header>
  );
}
