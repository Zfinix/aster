import { useCallback, useEffect, useState } from "react";
import type { Finding } from "../../src/types";
import type { ToWebview } from "../../src/protocol";
import { onHostMessage, post } from "../lib/host";
import { Disclosure } from "../interior/disclosure";
import { LoadingButton, type LoadingStatus } from "../interior/loading-button";
import { CodeBlock } from "./CodeBlock";
import { Markdown } from "./Markdown";
import { CheckIcon, ChevronIcon } from "./icons";

type FixStatus = "idle" | "fixing" | "applied" | "cannot_fix" | "blocked" | "error";

const FIX_FACE: Record<FixStatus, LoadingStatus> = {
  idle: "idle",
  fixing: "pending",
  applied: "success",
  cannot_fix: "error",
  blocked: "error",
  error: "error",
};

const FIX_FAIL: Partial<Record<FixStatus, string>> = {
  cannot_fix: "Can't fix",
  blocked: "Blocked",
  error: "Failed",
};

/** One finding as a quiet row — severity dot, title, location — that opens into
 *  the detail where the actions live. Reading comes before fixing. */
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

  const lang = finding.file_path.split(".").pop();
  const fixed = status === "applied";
  const file = finding.file_path.split("/").pop() ?? finding.file_path;

  return (
    <div
      className="finding"
      data-severity={finding.severity}
      data-open={open}
      data-fixed={fixed || undefined}
    >
      <button className="finding-row" onClick={() => setOpen(!open)} aria-expanded={open}>
        <span className="finding-mark">
          {fixed ? <CheckIcon /> : <span className="finding-dot" />}
        </span>
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
          <Markdown text={finding.description} />
          {finding.code_snippet && (
            <CodeBlock code={finding.code_snippet} lang={lang} />
          )}
          {finding.suggestion && (
            <div className="finding-fix">
              <span className="finding-fix-label">Suggested fix</span>
              <Markdown text={finding.suggestion} />
            </div>
          )}
          {reason && status !== "idle" && status !== "fixing" && (
            <p className="finding-fix-reason">{reason}</p>
          )}
          <div className="finding-actions">
            <span className="finding-tags">
              {finding.category}
              {finding.confidence != null && ` · ${Math.round(finding.confidence * 100)}%`}
            </span>
            <button
              className="btn"
              onClick={() => post({ type: "openFinding", finding })}
              title="Open in editor"
            >
              Open file
            </button>
            <LoadingButton
              status={FIX_FACE[status]}
              disabled={status !== "idle" && status !== "error"}
              idleLabel="Apply fix"
              pendingLabel="Fixing…"
              successLabel="Fixed"
              errorLabel={FIX_FAIL[status] ?? "Failed"}
              onClick={onFix}
            />
          </div>
        </div>
      </Disclosure>
    </div>
  );
}
