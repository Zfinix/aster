import { useCallback, useEffect, useState } from "react";
import type { Finding } from "../../src/types";
import type { ToWebview } from "../../src/protocol";
import { onHostMessage, post } from "../lib/host";
import { ChevronIcon, SpinnerIcon } from "./icons";

type FixStatus = "idle" | "fixing" | "applied" | "cannot_fix" | "blocked" | "error";

export function FindingCard({ finding }: { finding: Finding }) {
  const [open, setOpen] = useState(false);
  const [status, setStatus] = useState<FixStatus>("idle");
  const [reason, setReason] = useState<string | undefined>();

  const handle = useCallback((message: ToWebview) => {
    if (message.type !== "fixResult") return;
    if (
      message.finding.file_path === finding.file_path &&
      message.finding.line === finding.line &&
      message.finding.title === finding.title
    ) {
      setStatus(message.status as FixStatus);
      setReason(message.reason);
    }
  }, [finding.file_path, finding.line, finding.title]);

  useEffect(() => onHostMessage(handle), [handle]);

  const onFix = () => {
    setStatus("fixing");
    post({ type: "fixFinding", finding });
  };

  return (
    <div className="finding" data-severity={finding.severity} data-open={open}>
      <button className="finding-row" onClick={() => setOpen(!open)} aria-expanded={open}>
        <span className="finding-caret">
          <ChevronIcon open={open} />
        </span>
        <span className="finding-title">{finding.title}</span>
        <span className="finding-sev">{finding.severity}</span>
      </button>

      {open && (
        <div className="finding-detail">
          <p className="finding-desc">{finding.description}</p>
          {finding.suggestion && (
            <p className="finding-fix">
              <span className="finding-fix-label">Fix</span>
              {finding.suggestion}
            </p>
          )}
          <div className="finding-meta">
            <button
              className="finding-loc"
              onClick={() => post({ type: "openFinding", finding })}
              title="Open in editor"
            >
              {finding.file_path}:{finding.line}
            </button>
            <span className="finding-tags">
              {finding.category}
              {finding.confidence != null && ` · ${Math.round(finding.confidence * 100)}%`}
            </span>
            <button
              className="finding-fix-btn"
              onClick={onFix}
              disabled={status === "fixing"}
              title={status === "idle" ? "Apply fix with Aster" : undefined}
            >
              {status === "idle" && "Apply fix"}
              {status === "fixing" && <SpinnerIcon />}
              {status === "applied" && "✓ Fixed"}
              {status === "cannot_fix" && "Can't fix"}
              {status === "blocked" && "Blocked"}
              {status === "error" && "Error"}
            </button>
          </div>
          {reason && status !== "idle" && status !== "fixing" && (
            <p className="finding-fix-reason">{reason}</p>
          )}
        </div>
      )}
    </div>
  );
}
