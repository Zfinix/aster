import { motion, useReducedMotion } from "motion/react";
import { CELL, CROSSFADE, INSTANT } from "./springs";

export type LoadingStatus = "idle" | "pending" | "success" | "error";

function Spinner({ still }: { still: boolean }) {
  return (
    <motion.svg
      width="12"
      height="12"
      viewBox="0 0 12 12"
      fill="none"
      aria-hidden="true"
      animate={still ? undefined : { rotate: 360 }}
      transition={still ? undefined : { duration: 0.85, repeat: Infinity, ease: "linear" }}
    >
      <circle cx="6" cy="6" r="4.5" stroke="currentColor" strokeWidth="1.5" strokeOpacity="0.22" />
      <path d="M10.5 6A4.5 4.5 0 0 0 6 1.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
    </motion.svg>
  );
}

const CheckMark = (
  <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
    <path d="M2.6 6.3 4.9 8.6 9.4 3.6" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);

const AlertMark = (
  <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
    <path d="M6 2.9v3.5" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" />
    <path d="M6 9.05h.01" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" />
  </svg>
);

/** interior.dev's loading button, controlled: the host already owns the async
 *  state, so the button just crossfades between its faces without a re-layout. */
export function LoadingButton({
  status,
  onClick,
  disabled = false,
  idleLabel,
  pendingLabel,
  successLabel = "Done",
  errorLabel = "Try again",
  className = "btn-primary",
}: {
  status: LoadingStatus;
  onClick: () => void;
  disabled?: boolean;
  idleLabel: string;
  pendingLabel?: string;
  successLabel?: string;
  errorLabel?: string;
  className?: string;
}) {
  const reduced = useReducedMotion();
  const fade = reduced ? INSTANT : CROSSFADE;
  const pending = status === "pending";

  const faces: Array<{ key: LoadingStatus; text: string; icon: React.ReactNode }> = [
    { key: "idle", text: idleLabel, icon: null },
    { key: "pending", text: pendingLabel ?? idleLabel, icon: <Spinner still={reduced === true || !pending} /> },
    { key: "success", text: successLabel, icon: CheckMark },
    { key: "error", text: errorLabel, icon: AlertMark },
  ];

  const label = faces.find((f) => f.key === status)?.text ?? idleLabel;

  return (
    <>
      <motion.button
        type="button"
        className={`${className} btn-faces`}
        disabled={disabled}
        aria-label={label}
        aria-busy={pending || undefined}
        whileTap={disabled || pending || reduced ? undefined : { y: 1 }}
        transition={CELL}
        onClick={(event) => {
          if (pending) {
            event.preventDefault();
            return;
          }
          onClick();
        }}
      >
        <span aria-hidden="true" className="stack">
          {faces.map((face) => (
            <motion.span
              key={face.key}
              className="btn-face"
              initial={false}
              animate={
                face.key === status
                  ? { opacity: 1, y: 0, filter: "blur(0px)" }
                  : { opacity: 0, y: 3, filter: "blur(3px)" }
              }
              transition={fade}
            >
              {face.icon}
              {face.text}
            </motion.span>
          ))}
        </span>
      </motion.button>

      <span role="status" aria-live="polite" className="sr-only">
        {status === "success" ? successLabel : status === "error" ? errorLabel : ""}
      </span>
    </>
  );
}
