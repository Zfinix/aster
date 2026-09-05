import { useEffect, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import type { ReviewData } from "../lib/session";
import type { Finding } from "../lib/types";
import { severityOf, SEV_RANK } from "../lib/severity";
import { Disclosure } from "../interior/disclosure";
import { TaskSteps, type TaskStep } from "../interior/task-steps";
import { ARRIVE, CROSSFADE, INSTANT } from "../interior/springs";
import { ErrorBox } from "./ErrorBox";
import { FindingRow } from "./FindingRow";
import { CheckIcon, ChevronIcon, ShieldIcon } from "./icons";

const SEVERITY_ORDER = ["critical", "high", "medium", "low", "info"];

function stepId(phase: string): string {
  if (phase.startsWith("Verifying")) return "verify";
  if (phase.startsWith("Index")) return "index";
  if (/candidates? to verify$/.test(phase)) return "hypothesize";
  return phase;
}

function formatTokens(total: number): string {
  return total >= 1000 ? `${(total / 1000).toFixed(1)}k` : String(total);
}

export function ReviewTurn({
  data,
  onOpenDiff,
  onFocusFinding,
  onApplyFix,
  onRetry,
}: {
  data: ReviewData;
  onOpenDiff: () => void;
  onFocusFinding: (finding: Finding) => void;
  onApplyFix: (finding: Finding) => Promise<boolean>;
  onRetry: () => void;
}) {
  const reduced = useReducedMotion() === true;
  const [open, setOpen] = useState(true);
  const [showRefuted, setShowRefuted] = useState(false);
  const [steps, setSteps] = useState<TaskStep[]>([]);

  useEffect(() => {
    const phase = data.phase;
    if (!phase) return;
    const id = stepId(phase);
    setSteps((prev) => {
      const last = prev[prev.length - 1];
      if (last?.id === id) {
        return last.label === phase ? prev : [...prev.slice(0, -1), { id, label: phase }];
      }
      return [...prev, { id, label: phase }];
    });
  }, [data.phase]);

  const findings = [...data.findings].sort(
    (a, b) => SEV_RANK[severityOf(a.severity)] - SEV_RANK[severityOf(b.severity)],
  );

  const running = data.status === "running";
  const done = data.status === "done";
  const failed = data.status === "error";
  const adds = data.files.reduce((s, f) => s + f.additions, 0);
  const dels = data.files.reduce((s, f) => s + f.deletions, 0);

  return (
    <div className="review-card" data-status={data.status}>
      <button type="button" className="review-head" onClick={() => setOpen(!open)} aria-expanded={open}>
        <span className="review-title">
          <ShieldIcon />
          Review
        </span>
        <span className="review-state">
          <AnimatePresence initial={false}>
            <motion.span
              key={data.status}
              className="review-state-face"
              initial={reduced ? { opacity: 0 } : { opacity: 0, y: 5, filter: "blur(3px)" }}
              animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
              exit={
                reduced
                  ? { opacity: 0, transition: INSTANT }
                  : { opacity: 0, y: -5, filter: "blur(3px)", transition: CROSSFADE }
              }
              transition={reduced ? INSTANT : CROSSFADE}
            >
              {running ? (
                <span className="review-phase shimmer">{data.phase || "Reading the diff"}</span>
              ) : failed ? (
                "Review failed"
              ) : findings.length === 0 ? (
                <span className="review-clean">
                  <CheckIcon />
                  No issues found
                </span>
              ) : (
                <>
                  {findings.length} finding{findings.length === 1 ? "" : "s"}
                  {SEVERITY_ORDER.filter(
                    (s) => s !== "info" && findings.some((f) => severityOf(f.severity) === s),
                  ).map((s) => (
                    <span key={s} className="review-sev" data-severity={s}>
                      {findings.filter((f) => severityOf(f.severity) === s).length} {s}
                    </span>
                  ))}
                </>
              )}
            </motion.span>
          </AnimatePresence>
        </span>
        <span className="review-caret">
          <ChevronIcon open={open} />
        </span>
      </button>

      <Disclosure open={open}>
        <div className="review-body">
          <Disclosure open={!done && steps.length > 0}>
            <div className="review-progress">
              <TaskSteps steps={steps} current={steps.length - 1} failed={failed} label="Review progress" />
            </div>
          </Disclosure>

          {failed && (
            <ErrorBox message={data.errorMsg || "aster exited with an error"}>
              <button type="button" className="btn" onClick={onRetry}>
                Try again
              </button>
            </ErrorBox>
          )}

          {data.summary && !running && <p className="review-summary">{data.summary}</p>}

          {findings.length > 0 && (
            <div className="finding-list">
              {findings.map((finding) => (
                <motion.div
                  key={`${finding.file_path}:${finding.line}:${finding.title}`}
                  layout={reduced ? undefined : "position"}
                  initial={reduced ? false : { opacity: 0, y: 5 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={reduced ? INSTANT : ARRIVE}
                >
                  <FindingRow finding={finding} onFocus={onFocusFinding} onApplyFix={onApplyFix} />
                </motion.div>
              ))}
            </div>
          )}

          {!running && !failed && (
            <div className="review-foot">
              <span className="review-meta">
                {data.files.length > 0 && (
                  <button type="button" className="review-meta-toggle" onClick={onOpenDiff}>
                    {data.files.length} file{data.files.length === 1 ? "" : "s"} · +{adds} −{dels}
                  </button>
                )}
                {data.refuted.length > 0 && (
                  <button
                    type="button"
                    className="review-meta-toggle"
                    onClick={() => setShowRefuted(!showRefuted)}
                    aria-expanded={showRefuted}
                  >
                    <ChevronIcon open={showRefuted} />
                    {data.refuted.length} refuted
                  </button>
                )}
                {data.usage && (
                  <span className="review-usage">
                    {formatTokens(data.usage.total_tokens)} tokens
                    {data.usage.estimated_cost_usd != null && ` · ~$${data.usage.estimated_cost_usd.toFixed(4)}`}
                  </span>
                )}
              </span>
            </div>
          )}

          <Disclosure open={showRefuted}>
            <div className="refuted-list">
              {data.refuted.map((r, i) => (
                <div key={i} className="refuted-item">
                  <span className="refuted-title">{r.title}</span>
                  <span className="refuted-reason">{r.reason}</span>
                </div>
              ))}
            </div>
          </Disclosure>
        </div>
      </Disclosure>
    </div>
  );
}
