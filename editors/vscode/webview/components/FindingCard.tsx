import { useState } from "react";
import type { Finding } from "../../src/types";
import type { FixState } from "../lib/review";
import { post } from "../lib/host";
import { Disclosure } from "../interior/disclosure";
import { LoadingButton, type LoadingStatus } from "../interior/loading-button";
import { CodeBlock } from "./CodeBlock";
import { DiffView } from "./DiffView";
import { Markdown } from "./Markdown";
import {
  ChevronIcon,
  CircleCheckFilledIcon,
  CircleXFilledIcon,
  ExternalIcon,
  EyeOffIcon,
  MinusIcon,
  SpinnerIcon,
  UndoIcon,
} from "./icons";

const STATE_LABEL: Record<Exclude<FixState["status"], "fixing">, string> = {
  fixed: "Fixed",
  failed: "Couldn't fix",
  blocked: "Not allowed",
  dismissed: "Dismissed",
};

const BUTTON_FACE: Record<FixState["status"], LoadingStatus> = {
  fixing: "pending",
  fixed: "success",
  failed: "error",
  blocked: "error",
  dismissed: "idle",
};

/** One issue as a quiet row that opens into the detail where the actions live.
 *  The row's mark and label carry what happened to it, so a list of issues
 *  reads as a checklist once the reader starts acting on them. */
export function FindingCard({
  finding,
  fix,
  onFix,
  onDismiss,
  onRestore,
}: {
  finding: Finding;
  fix: FixState | undefined;
  onFix: () => void;
  onDismiss: () => void;
  onRestore: () => void;
}) {
  const [open, setOpen] = useState(false);
  const lang = finding.file_path.split(".").pop();
  const file = finding.file_path.split("/").pop() ?? finding.file_path;
  const status = fix?.status;
  const settled = status === "fixed" || status === "dismissed";

  return (
    <div
      className="finding"
      data-severity={finding.severity}
      data-open={open}
      data-state={status ?? "open"}
    >
      <button className="finding-row" onClick={() => setOpen(!open)} aria-expanded={open}>
        <span className="finding-mark">
          {status === "fixing" ? (
            <SpinnerIcon />
          ) : status === "fixed" ? (
            <CircleCheckFilledIcon />
          ) : status === "dismissed" ? (
            <MinusIcon />
          ) : status === "failed" || status === "blocked" ? (
            <CircleXFilledIcon />
          ) : (
            <span className="finding-dot" />
          )}
        </span>
        <span className="finding-title">{finding.title}</span>
        {status && status !== "fixing" ? (
          <span className="finding-state">{STATE_LABEL[status]}</span>
        ) : (
          <span className="finding-loc" title={finding.file_path}>
            <span className="finding-sev">{finding.severity}</span>
            {file}
            {finding.line > 0 ? `:${finding.line}` : ""}
          </span>
        )}
        <span className="finding-caret">
          <ChevronIcon open={open} />
        </span>
      </button>

      <Disclosure open={open}>
        <div className="finding-detail">
          {!settled && <Markdown text={finding.description} />}
          {!settled && finding.code_snippet && (
            <CodeBlock code={finding.code_snippet} lang={lang} />
          )}
          {!settled && finding.suggestion && (
            <div className="finding-fix">
              <span className="finding-fix-label">Suggested fix</span>
              <Markdown text={finding.suggestion} />
            </div>
          )}
          {(fix?.status === "failed" || fix?.status === "blocked") && (
            <div className="finding-outcome" data-tone="warn">
              <p>
                {fix.status === "blocked"
                  ? "Not allowed to edit this file. Change the permission mode or fix it by hand."
                  : "Couldn't fix this. Try again or fix it by hand."}
              </p>
              <p className="finding-outcome-detail">{fix.reason}</p>
            </div>
          )}
          {fix?.status === "fixed" && fix.patch && (
            <pre className="approval-preview">
              <DiffView lines={fix.patch.trimEnd().split("\n")} lang={lang} />
            </pre>
          )}
          <div className="finding-actions">
            <span className="finding-tags">
              {finding.category}
              {finding.confidence != null && ` · ${Math.round(finding.confidence * 100)}% sure`}
            </span>
            <button
              className="btn"
              onClick={() => post({ type: "openFinding", finding })}
              title={`Open ${finding.file_path}`}
            >
              <ExternalIcon />
              Open file
            </button>
            {status === "dismissed" ? (
              <button className="btn" onClick={onRestore}>
                <UndoIcon />
                Restore
              </button>
            ) : (
              status !== "fixed" &&
              status !== "fixing" && (
                <button className="btn" onClick={onDismiss}>
                  <EyeOffIcon />
                  Dismiss
                </button>
              )
            )}
            {!settled && (
              <LoadingButton
                status={status ? BUTTON_FACE[status] : "idle"}
                disabled={status === "fixing"}
                idleLabel="Fix"
                pendingLabel="Fixing…"
                successLabel="Fixed"
                errorLabel="Try again"
                onClick={onFix}
              />
            )}
          </div>
        </div>
      </Disclosure>
    </div>
  );
}
