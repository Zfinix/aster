import { useCallback, useEffect, useState } from "react";
import type { ReviewData } from "../lib/thread";
import type { ToWebview } from "../../src/protocol";
import { onHostMessage, post } from "../lib/host";
import { Disclosure } from "../interior/disclosure";
import { LoadingButton, type LoadingStatus } from "../interior/loading-button";
import { TaskSteps, type TaskStep } from "../interior/task-steps";
import { FindingCard } from "./FindingCard";
import { ChevronIcon } from "./icons";

const SEVERITY_ORDER = ["critical", "high", "medium", "low", "info"];

type FixAllStatus = "idle" | "fixing" | "done";

const FIX_STATUS: Record<FixAllStatus, LoadingStatus> = {
  idle: "idle",
  fixing: "pending",
  done: "success",
};

/** The verify phase streams one line per candidate; folding those into a single
 *  step keeps the list one row per stage instead of one per finding. */
function stepId(phase: string): string {
  if (phase.startsWith("Verifying ")) return "verify";
  if (phase.endsWith("to verify")) return "candidates";
  return phase;
}

export function ReviewTurn({ data }: { data: ReviewData }) {
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

  if (data.status === "error") {
    return <div className="turn-error">{data.errorMsg}</div>;
  }

  const findings = [...data.findings].sort(
    (a, b) => SEVERITY_ORDER.indexOf(a.severity) - SEVERITY_ORDER.indexOf(b.severity)
  );

  const running = data.status === "running";
  const stopped = data.status === "stopped";

  return (
    <div className="review">
      {running ? (
        <div className="review-head">
          <span className="status-pulse" />
          <span className="review-phase shimmer">{data.phase}</span>
        </div>
      ) : (
        <>
          {steps.length > 0 && (
            <TaskSteps
              steps={steps}
              current={stopped ? steps.length - 1 : steps.length}
              failed={stopped}
              label="Review progress"
            />
          )}
          <div className="review-head">
            <span className="review-verdict">
              {stopped
                ? "Stopped"
                : findings.length === 0
                  ? "No findings survived verification"
                  : `${findings.length} finding${findings.length === 1 ? "" : "s"}`}
            </span>
          </div>
        </>
      )}

      {data.summary && <p className="review-summary">{data.summary}</p>}

      {findings.length > 0 && !running && (
        <div className="review-actions">
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
        </div>
      )}

      {findings.length > 0 && (
        <div className="finding-list">
          {findings.map((finding, i) => (
            <FindingCard key={`${finding.file_path}:${finding.line}:${i}`} finding={finding} />
          ))}
        </div>
      )}

      {(data.refuted.length > 0 || data.usage) && (
        <div className="review-foot">
          {data.refuted.length > 0 && (
            <button
              className="link"
              onClick={() => setShowRefuted(!showRefuted)}
              aria-expanded={showRefuted}
            >
              <ChevronIcon open={showRefuted} />
              {data.refuted.length} refuted
            </button>
          )}
          {data.usage && (
            <span className="usage">
              {formatTokens(data.usage.total_tokens)} tokens
              {data.usage.estimated_cost_usd != null &&
                ` · ~$${data.usage.estimated_cost_usd.toFixed(4)}`}
            </span>
          )}
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
  );
}

function formatTokens(total: number): string {
  return total >= 1000 ? `${(total / 1000).toFixed(1)}k` : String(total);
}
