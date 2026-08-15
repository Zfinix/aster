/** Below this much left, the ring warns: a compact is close enough to plan for. */
const LOW = 0.25;

/** Hidden until the conversation has actually spent some of the window; an
 *  empty thread does not need a gauge. */
const SHOW_ABOVE = 0.05;

const SIZE = 16;
const R = 6;
const CIRCUMFERENCE = 2 * Math.PI * R;

/**
 * How much of the history budget is left before the CLI folds earlier turns
 * into a summary. The ring fills as the conversation grows; clicking compacts
 * now, which is the one thing a reader can do about it.
 */
export function ContextMeter({
  used,
  budget,
  onCompact,
}: {
  /** Characters the next turn would send. */
  used: number;
  /** Characters the CLI auto-compacts above; 0 when the CLI could not say. */
  budget: number;
  onCompact: () => void;
}) {
  if (budget <= 0) return null;
  const spent = Math.min(used / budget, 1);
  if (spent < SHOW_ABOVE) return null;

  const remaining = Math.round((1 - spent) * 100);
  const low = 1 - spent <= LOW;

  return (
    <button
      className="context-meter"
      data-low={low}
      onClick={onCompact}
      title={`${remaining}% of context remaining until auto-compact.\nClick to compact now.`}
      aria-label={`${remaining}% of context remaining. Compact now.`}
    >
      <svg width={SIZE} height={SIZE} viewBox={`0 0 ${SIZE} ${SIZE}`} aria-hidden="true">
        <circle className="context-track" cx={SIZE / 2} cy={SIZE / 2} r={R} />
        <circle
          className="context-fill"
          cx={SIZE / 2}
          cy={SIZE / 2}
          r={R}
          strokeDasharray={CIRCUMFERENCE}
          strokeDashoffset={CIRCUMFERENCE * (1 - spent)}
          // Twelve o'clock, so the ring reads as a dial rather than starting
          // from the right edge the way an SVG circle does.
          transform={`rotate(-90 ${SIZE / 2} ${SIZE / 2})`}
        />
      </svg>
    </button>
  );
}
