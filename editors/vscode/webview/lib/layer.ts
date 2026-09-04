import { useEffect, useRef, type RefObject } from "react";

const layers: symbol[] = [];

/** Holds the topmost layer while mounted. Escape closes the top layer only, and
 *  given `panel` a mousedown outside it closes too. A surface with no `onClose`
 *  still holds the layer, which keeps Escape off whatever a blocking prompt covers. */
export function useLayer(onClose?: () => void, panel?: RefObject<HTMLElement | null>): void {
  const close = useRef(onClose);
  close.current = onClose;

  useEffect(() => {
    const id = Symbol("layer");
    layers.push(id);
    const top = () => layers[layers.length - 1] === id;

    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || !top() || !close.current) return;
      e.preventDefault();
      close.current();
    };
    const onDown = (e: MouseEvent) => {
      const target = e.target as Node;
      // A row that swaps one panel for another is gone from the DOM by the time
      // this runs; that mousedown must not read as "outside" the new panel.
      if (!panel?.current || !target.isConnected || !top()) return;
      if (!panel.current.contains(target)) close.current?.();
    };

    document.addEventListener("keydown", onKey);
    document.addEventListener("mousedown", onDown);
    return () => {
      layers.splice(layers.indexOf(id), 1);
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("mousedown", onDown);
    };
  }, [panel]);
}
