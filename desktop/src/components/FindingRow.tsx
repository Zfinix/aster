import { useState } from "react";
import type { Finding } from "../lib/types";
import { severityOf } from "../lib/severity";
import { Disclosure } from "../interior/disclosure";
import { LoadingButton, type LoadingStatus } from "../interior/loading-button";
import { AssistantText } from "./AssistantText";
import { CheckIcon, ChevronIcon } from "./icons";

/** One finding as a quiet row: severity dot, title, location. It opens into
 *  the detail where the actions live. Reading comes before fixing. */
export function FindingRow({
  finding,
  onFocus,
  onApplyFix,
}: {
  finding: Finding;
  onFocus?: (f: Finding) => void;
  onApplyFix?: (f: Finding) => Promise<boolean>;
}) {
  const [open, setOpen] = useState(false);
  const [status, setStatus] = useState<LoadingStatus>("idle");
  const sev = severityOf(finding.severity);
  const fixed = status === "success";
  const file = finding.file_path.split("/").pop() ?? finding.file_path;

  const fix = async () => {
    if (!onApplyFix) return;
    setStatus("pending");
    try {
      setStatus((await onApplyFix(finding)) ? "success" : "error");
    } catch {
      setStatus("error");
    }
  };

  return (
    <div className="finding" data-severity={sev} data-open={open} data-fixed={fixed || undefined}>
      <button type="button" className="finding-row" onClick={() => setOpen((o) => !o)} aria-expanded={open}>
        <span className="finding-mark">{fixed ? <CheckIcon /> : <span className="finding-dot" />}</span>
        <span className="finding-title">{finding.title}</span>
        <span className="finding-loc" title={finding.file_path}>
          {file}
          {finding.line > 0 ? `:${finding.line}` : ""}
        </span>
        <span className="finding-caret">
          <ChevronIcon open={open} />
        </span>
      </button>

      <Disclosure open={open}>
        <div className="finding-detail">
          <AssistantText text={finding.description} />
          {finding.suggestion && (
            <div className="finding-fix">
              <span className="finding-fix-label">Suggested fix</span>
              <AssistantText text={finding.suggestion} />
            </div>
          )}
          <div className="finding-actions">
            <span className="finding-tags">
              {finding.category}
              {finding.confidence != null && ` · ${Math.round(finding.confidence * 100)}%`}
            </span>
            {onFocus && (
              <button type="button" className="btn" onClick={() => onFocus(finding)}>
                Show in diff
              </button>
            )}
            {onApplyFix && (
              <LoadingButton
                status={status}
                disabled={status !== "idle" && status !== "error"}
                idleLabel="Apply fix"
                pendingLabel="Fixing…"
                successLabel="Fixed"
                errorLabel="Failed"
                onClick={fix}
              />
            )}
          </div>
        </div>
      </Disclosure>
    </div>
  );
}
