import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import type { ReviewData } from "../lib/thread";
import { post } from "../lib/host";
import { openFilePreview } from "../lib/filePreview";
import { Disclosure } from "../interior/disclosure";
import { LoadingButton } from "../interior/loading-button";
import { TaskSteps, type TaskStep } from "../interior/task-steps";
import { ARRIVE, CELL, CROSSFADE, INSTANT } from "../interior/springs";
import {
  findingKey,
  fixAllRunning,
  fixOf,
  setFix,
  startFixAll,
  takeNextFix,
  tally,
} from "../lib/review";
import { CheckIcon, ChevronIcon, CircleCheckFilledIcon, ShieldIcon } from "./icons";
import { ErrorBox } from "./ErrorBox";
import { FindingCard } from "./FindingCard";
import { RefutedItem } from "./RefutedItem";
import type { Finding } from "../../src/types";

const SEVERITY_ORDER = ["critical", "high", "medium", "low", "info"];

function stepId(phase: string): string {
  if (phase.startsWith("Verifying")) return "verify";
  if (phase.startsWith("Index")) return "index";
  return phase;
}

export function ReviewTurn({
  data,
  onChange,
}: {
  data: ReviewData;
  onChange: (patch: (data: ReviewData) => ReviewData) => void;
}) {
  const reduced = useReducedMotion() === true;
  const [open, setOpen] = useState(true);
  const [showFiles, setShowFiles] = useState(false);
  const [showRefuted, setShowRefuted] = useState(false);
  const [steps, setSteps] = useState<TaskStep[]>([]);
  const sent = useRef<string | null>(null);

  // "Fix all" sends one issue at a time: each reply clears the in-flight mark,
  // which lets the next one go. `sent` keeps a re-run effect from sending twice.
  useEffect(() => {
    const next = takeNextFix(data);
    if (!next) {
      sent.current = null;
      return;
    }
    const key = findingKey(next.finding);
    if (sent.current === key) return;
    sent.current = key;
    onChange((current) => takeNextFix(current)?.data ?? current);
    post({ type: "fixFinding", finding: next.finding });
  }, [data, onChange]);

  const fix = (finding: Finding) => {
    onChange((current) => setFix(current, findingKey(finding), { status: "fixing" }));
    post({ type: "fixFinding", finding });
  };

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
  const counts = tally(data);
  const handled = findings.length > 0 && counts.open === 0 && counts.failed === 0;
  const fixingAll = fixAllRunning(data);
  const batch = data.fixAll && !fixingAll ? tally(data, data.fixAll.keys) : null;
  const fixable = counts.open + counts.failed;
  const tallyLine = [
    counts.open > 0 && `${counts.open} ${counts.open === 1 ? "issue" : "issues"}`,
    counts.fixed > 0 && `${counts.fixed} fixed`,
    counts.dismissed > 0 && `${counts.dismissed} dismissed`,
    counts.failed > 0 && `${counts.failed} not fixed`,
  ]
    .filter(Boolean)
    .join(" · ");
  const work = [
    data.files.length > 0 && `${data.files.length} ${data.files.length === 1 ? "file" : "files"}`,
    data.usage &&
      (data.usage.estimated_cost_usd != null
        ? `~$${data.usage.estimated_cost_usd.toFixed(3)}`
        : `${formatTokens(data.usage.total_tokens)} tokens`),
  ]
    .filter(Boolean)
    .join(" · ");
  // Real progress, like the agents card: checks landed while it runs, fixes
  // landed during a Fix all, nothing once it settles.
  const progress = running
    ? data.verify
      ? data.verify.index / data.verify.total
      : 0
    : fixingAll && data.fixAll
      ? (data.fixAll.keys.length - data.fixAll.queue.length - 1) / data.fixAll.keys.length
      : null;

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
                "Couldn't finish"
              ) : stopped ? (
                "Stopped"
              ) : findings.length === 0 ? (
                <span className="review-clean">
                  <CircleCheckFilledIcon />
                  No issues found
                </span>
              ) : handled ? (
                <span className="review-clean">
                  <CircleCheckFilledIcon />
                  All handled · {tallyLine}
                </span>
              ) : (
                tallyLine
              )}
            </motion.span>
          </AnimatePresence>
        </span>
        {work && <span className="review-work">{work}</span>}
        <span className="review-caret">
          <ChevronIcon open={open} />
        </span>
      </button>
      {progress != null && (
        <div className="agent-net-progress" role="progressbar" aria-label="Review progress">
          <motion.span
            className="agent-net-progress-fill"
            initial={false}
            animate={{ scaleX: progress }}
            transition={reduced ? INSTANT : CELL}
          />
        </div>
      )}

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
                  <FindingCard
                    finding={finding}
                    fix={fixOf(data, finding)}
                    onFix={() => fix(finding)}
                    onDismiss={() =>
                      onChange((current) =>
                        setFix(current, findingKey(finding), { status: "dismissed" })
                      )
                    }
                    onRestore={() =>
                      onChange((current) => setFix(current, findingKey(finding), null))
                    }
                  />
                </motion.div>
              ))}
            </div>
          )}

          <Disclosure open={showFiles}>
            <ul className="review-files-list">
              {data.files.map((file) => (
                <li key={file}>
                  <button
                    className="link"
                    onClick={() => openFilePreview(file)}
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
              <span className="refuted-heading">Ruled out</span>
              {data.refuted.map((r, i) => (
                <RefutedItem key={i} number={i + 1} item={r} />
              ))}
            </div>
          </Disclosure>
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
                    {data.refuted.length} ruled out
                  </button>
                )}
              </span>
              {batch && !fixingAll && (
                <span className="review-fix-result">
                  {[
                    batch.fixed > 0 && `${batch.fixed} fixed`,
                    batch.failed > 0 && `${batch.failed} not fixed`,
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </span>
              )}
              {(done || stopped) && (fixingAll || fixable > 0) && (
                <LoadingButton
                  status={fixingAll ? "pending" : "idle"}
                  disabled={fixingAll}
                  idleLabel={fixable === 1 ? "Fix it" : "Fix all"}
                  pendingLabel={
                    data.fixAll
                      ? `Fixing ${data.fixAll.keys.length - data.fixAll.queue.length} of ${data.fixAll.keys.length}`
                      : "Fixing…"
                  }
                  onClick={() => onChange(startFixAll)}
                />
              )}
            </div>
          )}

        </div>
      </Disclosure>
    </div>
  );
}

function formatTokens(total: number): string {
  return total >= 1000 ? `${(total / 1000).toFixed(1)}k` : String(total);
}
