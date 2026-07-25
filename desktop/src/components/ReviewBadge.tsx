import type { ReviewData } from "../lib/session";
import { severityOf } from "../lib/severity";
import { topSeverity, SEV_WORD } from "../lib/review-format";

export function ReviewBadge({ review }: { review: ReviewData }) {
  if (review.status === "error") {
    return (
      <span className="review-badge crit">
        <span className="badge-dot" />
        failed
      </span>
    );
  }
  if (review.status === "running") {
    return (
      <span className="review-badge med">
        <span className="badge-dot" />
        running
      </span>
    );
  }
  if (review.findings.length === 0) {
    return (
      <span className="review-badge clean">
        <span className="badge-dot" />
        Clean
      </span>
    );
  }
  const top = topSeverity(review.findings);
  const count = review.findings.filter((f) => severityOf(f.severity) === top).length;
  const cls = top === "critical" || top === "high" ? "crit" : "med";
  return (
    <span className={`review-badge ${cls}`}>
      <span className="badge-dot" />
      {count} {SEV_WORD[top]}
    </span>
  );
}
