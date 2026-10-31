import { parseError } from "../lib/error";

/** One turn-level failure: a short label, at most a five-word line of advice,
 *  an action when the turn can be run again, and the raw provider string folded
 *  behind Details. */
export function ErrorBox({
  message,
  onRetry,
}: {
  message?: string;
  onRetry?: () => void;
}) {
  if (!message) return null;
  const { label, hint, detail } = parseError(message);
  return (
    <div className="error-box">
      <div className="error-box-head">
        <span className="error-box-label">{label}</span>
        {onRetry && (
          <button className="msg-action error-box-retry" onClick={onRetry}>
            Try again
          </button>
        )}
      </div>
      {hint && <div className="error-box-hint">{hint}</div>}
      {detail && (
        <details className="error-box-more">
          <summary>Details</summary>
          <div className="error-box-detail">{detail}</div>
        </details>
      )}
    </div>
  );
}
