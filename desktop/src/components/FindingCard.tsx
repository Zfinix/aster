import { useState } from "react";
import type { Finding } from "../lib/types";
import { severityOf } from "../lib/severity";
import { findingKey } from "../lib/match";
import { useToast } from "./chrome";
import { ChevronIcon } from "./icons";

/** A finding pinned under the diff line it points at, with the fix actions. */
export function FindingCard({
  finding,
  onApplyFix,
}: {
  finding: Finding;
  onApplyFix: (finding: Finding) => Promise<boolean>;
}) {
  const toast = useToast();
  const [resolved, setResolved] = useState(false);
  const [open, setOpen] = useState(true);
  const [fixing, setFixing] = useState(false);
  const sev = severityOf(finding.severity);

  const copySuggestion = async () => {
    await navigator.clipboard.writeText(finding.suggestion || finding.description);
    toast("Suggestion copied");
  };

  const runFix = async () => {
    setFixing(true);
    try {
      if (await onApplyFix(finding)) setResolved(true);
    } catch (e) {
      toast(String(e));
    } finally {
      setFixing(false);
    }
  };

  if (resolved) {
    return (
      <div className="finding-resolved">
        <span className="finding-resolved-title">{finding.title}</span>
        <button type="button" className="btn" onClick={() => setResolved(false)}>
          Reopen
        </button>
      </div>
    );
  }

  return (
    <div className="finding-card finding" data-severity={sev} data-fkey={findingKey(finding)}>
      <button type="button" className="finding-card-head" onClick={() => setOpen((o) => !o)} aria-expanded={open}>
        <span className="finding-mark">
          <span className="finding-dot" />
        </span>
        <span className="finding-card-title">{finding.title}</span>
        {finding.confidence != null && (
          <span className="finding-card-conf">{Math.round(finding.confidence * 100)}%</span>
        )}
        <span className="finding-caret">
          <ChevronIcon open={open} />
        </span>
      </button>
      {open && (
        <div className="finding-card-body">
          <p>{finding.description}</p>
          {finding.suggestion && <div className="finding-card-fix">{finding.suggestion}</div>}
          <div className="finding-card-actions">
            <button type="button" className="btn-primary" disabled={fixing} onClick={runFix}>
              {fixing ? "Fixing…" : "Apply fix"}
            </button>
            <button type="button" className="btn" onClick={copySuggestion}>
              Copy fix
            </button>
            <button type="button" className="btn" onClick={() => setResolved(true)}>
              Resolve
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
