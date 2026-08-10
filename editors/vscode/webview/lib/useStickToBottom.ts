import { useCallback, useEffect, useRef, useState } from "react";

/** Within this many px of the bottom still counts as pinned. */
const THRESHOLD = 48;

/**
 * Follows the bottom of a scroll container as content grows, but only while the
 * reader has not scrolled away. Unconditional auto-scroll makes reading back
 * through a long run impossible: every streamed event yanks the view down.
 */
export function useStickToBottom() {
  const viewport = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  const [atBottom, setAtBottom] = useState(true);

  const scrollToBottom = useCallback((smooth = true) => {
    const el = viewport.current;
    if (!el) return;
    pinned.current = true;
    setAtBottom(true);
    el.scrollTo({ top: el.scrollHeight, behavior: smooth ? "smooth" : "auto" });
  }, []);

  const onScroll = useCallback(() => {
    const el = viewport.current;
    if (!el) return;
    const near = el.scrollHeight - el.scrollTop - el.clientHeight <= THRESHOLD;
    pinned.current = near;
    setAtBottom(near);
  }, []);

  // Content height changes on every streamed token, tool row, and expand, none
  // of which are a single React dep; observing the column catches them all.
  useEffect(() => {
    const el = viewport.current;
    const inner = content.current;
    if (!el || !inner) return;
    const observer = new ResizeObserver(() => {
      if (pinned.current) {
        el.scrollTop = el.scrollHeight;
      }
      // Content also shrinks (a reply replacing tool output, a collapsed row),
      // which leaves `pinned` false with no scroll event coming to correct it
      // and the jump-to-latest pill stranded over a thread that already fits.
      const near = el.scrollHeight - el.scrollTop - el.clientHeight <= THRESHOLD;
      pinned.current = near;
      setAtBottom(near);
    });
    observer.observe(inner);
    return () => observer.disconnect();
  }, []);

  return { viewport, content, atBottom, onScroll, scrollToBottom };
}
