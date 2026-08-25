import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import type { ReviewData } from "../lib/thread";
import type { ToWebview } from "../../src/protocol";
import { onHostMessage, post } from "../lib/host";
import { Disclosure } from "../interior/disclosure";
import { LoadingButton, type LoadingStatus } from "../interior/loading-button";
import { TaskSteps, type TaskStep } from "../interior/task-steps";
import { ARRIVE, CROSSFADE, INSTANT } from "../interior/springs";
import { CheckIcon, ChevronIcon, ShieldIcon } from "./icons";
import { ErrorBox } from "./ErrorBox";
import { FindingCard } from "./FindingCard";

const SEVERITY_ORDER = ["critical", "high", "medium", "low", "info"];

type FixAllStatus = "idle" | "fixing" | "done";

const FIX_STATUS: Record<FixAllStatus, LoadingStatus> = {
  idle: "idle",
  fixing: "pending",
  done: "success",
};

/** The indexing phase renames itself when it lands and the verify phase streams
 *  a line per candidate; folding both keeps the list one row per stage. */
function stepId(phase: string): string {
  if (phase.startsWith("Verifying")) return "verify";
  if (phase.startsWith("Index")) return "index";
  return phase;
}

export function ReviewTurn({ data }: { data: ReviewData }) {
  const reduced = useReducedMotion() === true;
  const [open, setOpen] = useState(true);
  const [showFiles, setShowFiles] = useState(false);
  const [showRefuted, setShowRefuted] = useState(false);
  const [fixAllStatus, setFixAllStatus] = useState<FixAllStatus>("idle");
  const [steps, setSteps] = useState<TaskStep[]>([]);

  const handle = useCallback((message: ToWebview) => {
    if (message.type === "fixAllResult") {
      setFixAllStatus("done");
    }
  }, []);

  useEffect(() => onHostMessage(handle), [handle]);

  useEffect(() => {
    const phase = data.phase;
    if (!phase || phase === "Starting") return;
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
    (a, b) => SEVERITY_ORDER.indexOf(a.severity) - SEVERITY_ORDER.indexOf(b.severity)
  );

  const running = data.status === "running";
  const done = data.status === "done";
  const stopped = data.status === "stopped";
  const failed = data.status === "error";

  // Live tallies ride the step rows: candidate count on the hypothesis step,
  // verify progress on the verify step.
  const stepRows = steps.map((step) =>
    step.id === "verify" && data.verify
      ? { ...step, meta: `${data.verify.index}/${data.verify.total}` }
      : step.id === "Hypothesizing" && data.candidates != null
        ? { ...step, meta: String(data.candidates) }
        : step
  );

  return (
    <div className="review-card" data-status={data.status}>
      <button className="review-head" onClick={() => setOpen(!open)} aria-expanded={open}>
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
                <span className="review-phase shimmer">
                  {data.verify ? `Verifying ${data.verify.index} of ${data.verify.total}` : data.phase}
                </span>
              ) : failed ? (
                "Review failed"
              ) : stopped ? (
                "Stopped"
              ) : findings.length === 0 ? (
                <span className="review-clean">
                  <CheckIcon />
                  No issues found
                </span>
              ) : (
                <>
                  {findings.length} finding{findings.length === 1 ? "" : "s"}
                  {SEVERITY_ORDER.filter(
                    (s) => s !== "info" && findings.some((f) => f.severity === s)
                  ).map((s) => (
                    <span key={s} className="review-sev" data-severity={s}>
                      {findings.filter((f) => f.severity === s).length} {s}
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
          {/* The process, live while it runs: it stays for a stop or a failure
              to show how far the run got, and folds away once findings land. */}
          <Disclosure open={!done && stepRows.length > 0}>
            <div className="review-progress">
              <TaskSteps
                steps={stepRows}
                current={stepRows.length - 1}
                failed={stopped || failed}
                label="Review progress"
              />
              {running && data.verify && (
                <div className="review-live">{data.verify.title}</div>
              )}
            </div>
          </Disclosure>

          {failed && <ErrorBox message={data.errorMsg} />}

          {data.summary && !running && <p className="review-summary">{data.summary}</p>}

          {findings.length > 0 && (
            <div className="finding-list">
              {findings.map((finding, i) => (
                <motion.div
                  key={`${finding.file_path}:${finding.line}:${finding.title}`}
                  layout={reduced ? undefined : "position"}
                  initial={reduced ? false : { opacity: 0, y: 5 }}
                  animate={{ opacity: 1, y: 0 }}
                  transition={reduced ? INSTANT : ARRIVE}
                >
                  <FindingCard finding={finding} />
                </motion.div>
              ))}
            </div>
          )}

          {!running && !failed && (
            <div className="review-foot">
              <span className="review-meta">
                {data.files.length > 0 && (
                  <button
                    className="review-meta-toggle"
                    onClick={() => setShowFiles(!showFiles)}
                    aria-expanded={showFiles}
                  >
                    <ChevronIcon open={showFiles} />
                    {data.files.length} file{data.files.length === 1 ? "" : "s"}
                  </button>
                )}
                {data.refuted.length > 0 && (
                  <button
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
                    {data.usage.estimated_cost_usd != null &&
                      ` · ~$${data.usage.estimated_cost_usd.toFixed(4)}`}
                  </span>
                )}
              </span>
              {done && findings.length > 0 && (
                <LoadingButton
                  status={FIX_STATUS[fixAllStatus]}
                  disabled={fixAllStatus !== "idle"}
                  idleLabel="Fix all"
                  pendingLabel="Fixing…"
                  successLabel="Done"
                  onClick={() => {
                    setFixAllStatus("fixing");
                    post({ type: "fixAllFindings", findings });
                  }}
                />
              )}
            </div>
          )}

          <Disclosure open={showFiles}>
            <ul className="review-files-list">
              {data.files.map((file) => (
                <li key={file}>
                  <button
                    className="link"
                    onClick={() => post({ type: "openFile", path: file })}
                    title="Open file"
                  >
                    {file}
                  </button>
                </li>
              ))}
            </ul>
          </Disclosure>

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

function formatTokens(total: number): string {
  return total >= 1000 ? `${(total / 1000).toFixed(1)}k` : String(total);
}
