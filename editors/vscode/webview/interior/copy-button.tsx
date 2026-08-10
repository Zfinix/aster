import { useCallback, useEffect, useRef, useState } from "react";
import { motion, useReducedMotion } from "motion/react";
import { CROSSFADE, DRAW, INSTANT } from "./springs";

export type CopyStatus = "idle" | "copied" | "error";

export type UseCopyToClipboardOptions = {
  timeout?: number;
  onCopy?: (value: string) => void;
  onError?: (reason: unknown) => void;
};

function writeFallback(text: string): boolean {
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.top = "0";
  area.style.left = "0";
  area.style.opacity = "0";
  document.body.appendChild(area);

  const selection = document.getSelection();
  const previous =
    selection && selection.rangeCount > 0 ? selection.getRangeAt(0) : null;

  area.select();
  let ok = false;
  try {
    ok = document.execCommand("copy");
  } catch {
    ok = false;
  }

  document.body.removeChild(area);
  if (selection && previous) {
    selection.removeAllRanges();
    selection.addRange(previous);
  }
  return ok;
}

export function useCopyToClipboard({
  timeout = 2000,
  onCopy,
  onError,
}: UseCopyToClipboardOptions = {}) {
  const [status, setStatus] = useState<CopyStatus>("idle");
  const [ticket, setTicket] = useState(0);

  const mounted = useRef(true);
  const copied = useRef(onCopy);
  copied.current = onCopy;
  const failed = useRef(onError);
  failed.current = onError;

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const copy = useCallback(async (text: string) => {
    if (!text) return false;

    let ok = false;
    let reason: unknown = null;

    try {
      if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(text);
        ok = true;
      } else {
        ok = writeFallback(text);
      }
    } catch (error) {
      reason = error;
      try {
        ok = writeFallback(text);
      } catch {
        ok = false;
      }
    }

    if (!mounted.current) return ok;

    setStatus(ok ? "copied" : "error");
    setTicket((t) => t + 1);

    if (ok) copied.current?.(text);
    else failed.current?.(reason);

    return ok;
  }, []);

  useEffect(() => {
    if (ticket === 0 || status === "idle") return;
    const id = setTimeout(() => setStatus("idle"), timeout);
    return () => clearTimeout(id);
  }, [ticket, status, timeout]);

  return { copy, status, copied: status === "copied" };
}

const icon = {
  viewBox: "0 0 16 16",
  width: 14,
  height: 14,
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.4,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};

/** interior.dev's copy button, restyled as this panel's icon button: the glyph
 *  crossfades to a drawn check or a cross, and reverts on a timer. */
export function CopyButton({
  text,
  label = "Copy",
  timeout = 2000,
}: {
  text: string;
  label?: string;
  timeout?: number;
}) {
  const { copy, status } = useCopyToClipboard({ timeout });
  const reduced = useReducedMotion();

  const fade = reduced ? INSTANT : CROSSFADE;
  const draw = reduced ? INSTANT : DRAW;
  const title = status === "copied" ? "Copied" : status === "error" ? "Copy failed" : label;

  return (
    <button
      className="icon-btn"
      title={title}
      aria-label={label}
      onClick={() => {
        void copy(text);
      }}
    >
      <span className="stack" aria-hidden="true">
        <motion.svg
          {...icon}
          initial={false}
          animate={{ opacity: status === "idle" ? 1 : 0, scale: status === "idle" ? 1 : 0.92 }}
          transition={fade}
        >
          <rect x="5.5" y="5.5" width="8" height="8" rx="1.3" />
          <path d="M10.5 5.5v-1a1.3 1.3 0 00-1.3-1.3H3.8A1.3 1.3 0 002.5 4.5v5.4a1.3 1.3 0 001.3 1.3h1" />
        </motion.svg>

        <motion.svg
          {...icon}
          initial={false}
          animate={{ opacity: status === "copied" ? 1 : 0, scale: status === "copied" ? 1 : 0.92 }}
          transition={fade}
        >
          <motion.path
            d="M3.5 8.5l3 3 6-7"
            initial={false}
            animate={{ pathLength: status === "copied" ? 1 : 0 }}
            transition={draw}
          />
        </motion.svg>

        <motion.svg
          {...icon}
          initial={false}
          animate={{ opacity: status === "error" ? 1 : 0, scale: status === "error" ? 1 : 0.92 }}
          transition={fade}
        >
          <path d="M4.5 4.5l7 7M11.5 4.5l-7 7" />
        </motion.svg>
      </span>

      <span role="status" aria-live="polite" className="sr-only">
        {status === "copied" ? "Copied" : status === "error" ? "Copy failed" : ""}
      </span>
    </button>
  );
}
