import { useCallback, useEffect, useState } from "react";
import type { Finding } from "../../src/types";
import type { ToWebview } from "../../src/protocol";
import { onHostMessage, post } from "../lib/host";
import { Disclosure } from "../interior/disclosure";
import { CodeBlock } from "./CodeBlock";
import { Markdown } from "./Markdown";
import { ChevronIcon, SpinnerIcon } from "./icons";

type FixStatus = "idle" | "fixing" | "applied" | "cannot_fix" | "blocked" | "error";

/** One finding, on the same rail-and-disclosure pattern as an agent row: the
 *  severity glyph and file location stay visible, the detail opens inline. */
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

  return (
    <div className="finding" data-severity={finding.severity} data-open={open}>
      <div className="finding-head">
        <button
          className="finding-toggle"
          onClick={() => setOpen(!open)}
          aria-expanded={open}
        >
          <span className="finding-glyph" aria-hidden />
          <span className="finding-title">{finding.title}</span>
          <span className="finding-loc">
            {finding.file_path}
            {finding.line > 0 ? `:${finding.line}` : ""}
          </span>
          <span className="finding-caret">
            <ChevronIcon open={open} />
          </span>
        </button>
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

      <Disclosure open={open}>
        <div className="finding-detail">
          <Markdown text={finding.description} />
          {finding.code_snippet && (
            <CodeBlock code={finding.code_snippet} lang={lang} />
          )}
          {finding.suggestion && (
            <div className="finding-fix">
              <span className="finding-fix-label">Fix</span>
              <Markdown text={finding.suggestion} />
            </div>
          )}
          <div className="finding-meta">
            <span className="finding-tags">
              {finding.category}
              {finding.confidence != null && ` · ${Math.round(finding.confidence * 100)}%`}
            </span>
            <button
              className="finding-open"
              onClick={() => post({ type: "openFinding", finding })}
              title="Open in editor"
            >
              Open in editor
            </button>
          </div>
          {reason && status !== "idle" && status !== "fixing" && (
            <p className="finding-fix-reason">{reason}</p>
          )}
        </div>
      </Disclosure>
    </div>
  );
}
