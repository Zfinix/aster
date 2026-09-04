import type { ReactNode } from "react";
import type { PlanStepStatus, ToolCall } from "../lib/thread";
import { parsePlan } from "../lib/thread";
import {
  CircleCheckIcon,
  CircleDotIcon,
  CircleIcon,
  CircleSkipIcon,
  CircleXIcon,
} from "./icons";

const MARKS: Record<PlanStepStatus, ReactNode> = {
  pending: <CircleIcon />,
  in_progress: <CircleDotIcon />,
  done: <CircleCheckIcon />,
  skipped: <CircleSkipIcon />,
  blocked: <CircleXIcon />,
};

/** The agent's plan, rendered inline where the `update_plan` call landed —
 *  the same as any other tool output, not pinned outside the thread. */
export function PlanCard({ call }: { call: ToolCall }) {
  const steps = parsePlan(call.arguments);
  if (!steps) return null;

  const done = steps.filter((s) => s.status === "done").length;
  const moving = steps.filter((s) => s.status === "in_progress").length;
  const complete = done === steps.length;
  const summary = complete
    ? "all done"
    : `${done}/${steps.length} done${moving ? ` · ${moving} in progress` : ""}`;

  return (
    <div className="plan" role="status" aria-label="Plan" data-complete={complete}>
      <div className="plan-head">
        <span className="plan-title">Plan</span>
        <span className="plan-summary">{summary}</span>
      </div>
      <div className="plan-steps">
        {steps.map((step, i) => (
          <div key={i} className="plan-step" data-status={step.status}>
            <span className="plan-glyph">{MARKS[step.status]}</span>
            <span className="plan-label">{step.label}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
