import { useEffect, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { ARRIVE, EASE, INSTANT } from "./springs";

/** interior.dev's new-items pill anchored over the thread: springs in when the
 *  reader scrolls away, counts what arrived below, and announces it politely. */
export function NewItemsPill({
  show,
  count,
  onJump,
}: {
  show: boolean;
  count: number;
  onJump: () => void;
}) {
  const reduced = useReducedMotion();
  const [announced, setAnnounced] = useState(0);

  useEffect(() => {
    if (count === 0) {
      setAnnounced(0);
      return;
    }
    const t = setTimeout(() => setAnnounced(count), 700);
    return () => clearTimeout(t);
  }, [count]);

  const phrase = (n: number) => `${n} new ${n === 1 ? "reply" : "replies"}`;
  const text = count > 0 ? phrase(count) : "Jump to latest";

  return (
    <div className="scroll-pill-slot">
      <AnimatePresence initial={false}>
        {show && (
          <motion.button
            type="button"
            className="scroll-pill"
            data-labeled={count > 0}
            onClick={onJump}
            title={text}
            aria-label={text}
            initial={reduced ? { opacity: 0 } : { opacity: 0, scale: 0.94, y: 10 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={
              reduced
                ? { opacity: 0, transition: INSTANT }
                : { opacity: 0, scale: 0.96, y: 5, transition: { duration: 0.16, ease: EASE } }
            }
            transition={reduced ? INSTANT : { ...ARRIVE, opacity: { duration: 0.16, ease: EASE } }}
          >
            <svg
              width="14"
              height="14"
              viewBox="0 0 16 16"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.4"
              strokeLinecap="round"
              strokeLinejoin="round"
              aria-hidden="true"
            >
              <path d="M8 3v10M4 9l4 4 4-4" />
            </svg>
            {count > 0 && (
              <span className="scroll-pill-label" aria-hidden="true">
                {count}
              </span>
            )}
          </motion.button>
        )}
      </AnimatePresence>
      <span role="status" aria-live="polite" className="sr-only">
        {announced > 0 ? phrase(announced) : ""}
      </span>
    </div>
  );
}
