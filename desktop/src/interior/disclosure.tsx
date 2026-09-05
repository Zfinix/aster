import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { DISCLOSE, INSTANT } from "./springs";

/** interior.dev's accordion reveal without the accordion: height springs open
 *  and closed, and stays interruptible mid-flight. */
export function Disclosure({
  open,
  children,
}: {
  open: boolean;
  children: React.ReactNode;
}) {
  const reduced = useReducedMotion();

  return (
    <AnimatePresence initial={false}>
      {open && (
        <motion.div
          initial={reduced ? { opacity: 0 } : { height: 0, opacity: 0 }}
          animate={reduced ? { opacity: 1 } : { height: "auto", opacity: 1 }}
          exit={
            reduced
              ? { opacity: 0, transition: INSTANT }
              : { height: 0, opacity: 0, transition: DISCLOSE }
          }
          transition={reduced ? INSTANT : DISCLOSE}
          style={{ overflow: "hidden" }}
        >
          {children}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
