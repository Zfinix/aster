import { useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { useLayer } from "../lib/layer";

/** Anything that opens above the composer. The scrim takes the click meant to
 *  put it away, so nothing in the thread underneath gets it too; the panel
 *  brings its own chrome, this only decides where it hangs and how it leaves. */
export function Popover({
  onClose,
  anchor,
  children,
}: {
  onClose: () => void;
  anchor?: RefObject<HTMLElement | null>;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [left, setLeft] = useState<number>();
  useLayer(onClose, ref);

  useLayoutEffect(() => {
    const box = ref.current;
    const target = anchor?.current;
    if (!box || !target) {
      setLeft(undefined);
      return;
    }
    const base = (box.offsetParent as HTMLElement | null)?.getBoundingClientRect().left ?? 0;
    setLeft(target.getBoundingClientRect().left - base);
  }, [anchor]);

  return (
    <>
      <div className="scrim" />
      <div
        className="pop"
        data-anchored={left !== undefined}
        style={left !== undefined ? { left } : undefined}
        ref={ref}
      >
        {children}
      </div>
    </>
  );
}
