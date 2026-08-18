import { useEffect, type RefObject } from "react";

/** Close a composer popup on Escape or a click outside it. */
export function useDismiss(ref: RefObject<HTMLElement | null>, onClose: () => void): void {
  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      const target = e.target as Node;
      // A row that opens another popup is removed from the DOM during this
      // mousedown, and the old popup's ref is cleared when it unmounts. The
      // same event must not read as "outside" the new popup and close it.
      if (!ref.current || !target.isConnected) return;
      if (!ref.current.contains(target)) {
        onClose();
      }
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [ref, onClose]);
}
