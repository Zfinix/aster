import { useState } from "react";
import { ActivityOutput } from "./ActivityOutput";
import { ChevronIcon } from "./icons";
import { summarizeSteps, type ToolStep } from "../lib/session";

/** The chat agent's tool activity for a turn: a muted summary line that
 *  expands into one bordered card of step rows with hairline dividers —
 *  each row revealing its output in place. Mirrors Claude's activity panel. */
export function ActivityPanel({ steps }: { steps: ToolStep[] }) {
  const [open, setOpen] = useState(false);
  if (!steps.length) return null;

  return (
    <div className="activity">
      <button
        className="activity-head"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
      >
        <span className="activity-summary">{summarizeSteps(steps)}</span>
        <span className="activity-caret" data-open={open}>
          <ChevronIcon size={12} />
        </span>
      </button>
      {open && (
        <div className="activity-card">
          {steps.map((s) => (
            <ActivityStep key={s.id} step={s} />
          ))}
        </div>
      )}
    </div>
  );
}

function ActivityStep({ step }: { step: ToolStep }) {
  const [open, setOpen] = useState(false);
  const hasOutput = !!step.output?.trim();

  return (
    <div className="activity-row">
      <button
        className="activity-row-head"
        onClick={() => hasOutput && setOpen((o) => !o)}
        aria-expanded={hasOutput ? open : undefined}
        data-static={!hasOutput}
      >
        <span className="activity-step-label">{step.label}</span>
        {hasOutput && (
          <span className="activity-caret" data-open={open}>
            <ChevronIcon size={11} />
          </span>
        )}
      </button>
      {open && hasOutput && <ActivityOutput name={step.name} output={step.output!} />}
    </div>
  );
}
