import { useEffect, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { CELL, INSTANT, POP } from "./springs";

export type TaskStep = {
  id: string;
  label: string;
  meta?: string;
};

export type TaskStepStatus = "pending" | "active" | "done" | "error";

const Tick = (
  <svg viewBox="0 0 16 16" width="10" height="10" fill="none" aria-hidden="true">
    <polyline
      points="13 4.5 6.5 11.5 3 8"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  </svg>
);

const Cross = (
  <svg viewBox="0 0 16 16" width="9" height="9" fill="none" aria-hidden="true">
    <path
      d="M12.5 3.5 3.5 12.5 M3.5 3.5l9 9"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
    />
  </svg>
);

const Arc = ({ spin }: { spin: boolean }) => (
  <motion.svg
    viewBox="0 0 16 16"
    width="12"
    height="12"
    aria-hidden="true"
    animate={spin ? { rotate: 360 } : { rotate: 0 }}
    transition={spin ? { duration: 0.8, ease: "linear", repeat: Infinity } : INSTANT}
  >
    <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" strokeOpacity="0.25" strokeWidth="2" />
    <path d="M8 2 a6 6 0 0 1 6 6" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
  </motion.svg>
);

/** interior.dev's task steps on this panel's tokens: each row's badge pops from
 *  dot to spinner to tick, and the active label carries the house shimmer. */
export function TaskSteps({
  steps,
  current,
  failed = false,
  label = "Progress",
}: {
  steps: TaskStep[];
  current: number;
  failed?: boolean;
  label?: string;
}) {
  const reduced = useReducedMotion() === true;
  const complete = !failed && current >= steps.length;

  const rows = steps.map((step, i) => ({
    ...step,
    status: (i < current
      ? "done"
      : i === current && failed
        ? "error"
        : i === current && !complete
          ? "active"
          : "pending") as TaskStepStatus,
  }));

  const active = rows.find((r) => r.status === "active");
  const sentence = failed
    ? `Stopped at ${steps[Math.min(current, steps.length - 1)]?.label ?? "step"}`
    : complete
      ? `All ${steps.length} steps complete`
      : active
        ? `${active.label}, step ${current + 1} of ${steps.length}`
        : "";

  const [spoken, setSpoken] = useState("");
  useEffect(() => {
    if (!sentence) return;
    const t = setTimeout(() => setSpoken(sentence), 500);
    return () => clearTimeout(t);
  }, [sentence]);

  return (
    <div>
      <ol aria-label={label} className="steps">
        {rows.map((row) => (
          <li
            key={row.id}
            className="step"
            data-status={row.status}
            aria-current={row.status === "active" ? "step" : undefined}
          >
            <span className="step-badge stack">
              <AnimatePresence initial={false}>
                {row.status === "done" ? (
                  <motion.span
                    key="done"
                    className="step-mark step-tick stack"
                    initial={reduced ? { opacity: 0 } : { opacity: 0, scale: 0.4 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, transition: INSTANT }}
                    transition={reduced ? INSTANT : POP}
                  >
                    {Tick}
                  </motion.span>
                ) : row.status === "error" ? (
                  <motion.span
                    key="error"
                    className="step-mark step-cross stack"
                    initial={reduced ? { opacity: 0 } : { opacity: 0, scale: 0.4 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, transition: INSTANT }}
                    transition={reduced ? INSTANT : POP}
                  >
                    {Cross}
                  </motion.span>
                ) : row.status === "active" ? (
                  <motion.span
                    key="active"
                    className="step-spin stack"
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0, transition: INSTANT }}
                    transition={reduced ? INSTANT : CELL}
                  >
                    <Arc spin={!reduced} />
                  </motion.span>
                ) : (
                  <motion.span
                    key="pending"
                    className="step-dot"
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0, transition: INSTANT }}
                    transition={INSTANT}
                  />
                )}
              </AnimatePresence>
            </span>

            <span className={`step-label${row.status === "active" && !reduced ? " shimmer" : ""}`}>
              {row.label}
            </span>

            {row.meta && (
              <span className="step-meta" aria-hidden={row.status !== "done"}>
                {row.meta}
              </span>
            )}
          </li>
        ))}
      </ol>
      <span role="status" className="sr-only">
        {spoken}
      </span>
    </div>
  );
}
