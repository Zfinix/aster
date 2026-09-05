import type { ReviewData } from "../lib/session";
import { severityOf } from "../lib/severity";
import { topSeverity } from "../lib/review-format";

export function ReviewBadge({ review }: { review: ReviewData }) {
  let color = "var(--fg-dim)";
  let text: string;
  if (review.status === "error") {
    color = "var(--error)";
    text = "failed";
  } else if (review.status === "running") {
    text = "running";
  } else if (review.findings.length === 0) {
    color = "var(--diff-add)";
    text = "clean";
  } else {
    const top = topSeverity(review.findings);
    const count = review.findings.filter((f) => severityOf(f.severity) === top).length;
    color = `var(--sev-${top})`;
    text = `${count} ${top}`;
  }
  return (
    <span className="review-badge">
      <span className="review-badge-dot" style={{ background: color }} />
      {text}
    </span>
  );
}
