/** Below this much left, the ring warns: a compact is close enough to plan for. */
const LOW = 0.25;

/** Hidden until the conversation has actually spent some of the window; an
 *  empty thread does not need a gauge. */
const SHOW_ABOVE = 0.05;

const SIZE = 16;
const R = 6;
const CIRCUMFERENCE = 2 * Math.PI * R;

/**
 * How much of the history budget the conversation has used. The ring fills as
 * it grows; hovering opens a card with the exact figure and what happens when
 * the space runs out, and clicking compacts now.
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

  const usedPct = Math.round(spent * 100);
  const low = 1 - spent <= LOW;

  return (
    <span className="context-wrap">
      <button
        className="context-meter"
        data-low={low}
        onClick={onCompact}
        aria-label={`${usedPct}% of conversation space used. Click to fold older messages into a summary now.`}
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
      <span className="context-pop" role="tooltip" data-low={low}>
        <span className="context-pop-head">
          <span className="context-pop-title">Conversation space</span>
          <span className="context-pop-pct">{usedPct}% used</span>
        </span>
        <span className="context-pop-bar">
          <span className="context-pop-bar-fill" style={{ width: `${usedPct}%` }} />
        </span>
        <span className="context-pop-note">
          When it fills up, Aster folds older messages into a summary and keeps
          going. Click the ring to tidy up now.
        </span>
      </span>
    </span>
  );
}
