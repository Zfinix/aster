import { useState } from "react";
import type { AgentRun } from "../lib/session";
import { ChevronIcon } from "./icons";
import { AssistantText } from "./AssistantText";

/** The sub-agent swarm for a turn: one row per agent with live status, expanding
 *  into each finished report rendered as markdown. Mirrors ActivityPanel. */
export function AgentPanel({ agents }: { agents: AgentRun[] }) {
  const [open, setOpen] = useState(false);
  if (!agents.length) return null;

  const settled = agents.filter((a) => a.status !== "running").length;
  const failed = agents.filter((a) => a.status === "error").length;

  return (
    <div className="activity">
      <button
        className="activity-head"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
      >
        <span className="activity-summary">
          Agents · {settled}/{agents.length}
          {failed > 0 ? ` · ${failed} failed` : ""}
        </span>
        <span className="activity-caret" data-open={open}>
          <ChevronIcon size={12} />
        </span>
      </button>
      {open && (
        <div className="activity-card">
          {agents.map((a) => (
            <AgentStep key={`${a.callId}:${a.agent}`} run={a} />
          ))}
        </div>
      )}
    </div>
  );
}

function AgentStep({ run }: { run: AgentRun }) {
  const [open, setOpen] = useState(false);
  const glyph =
    run.status === "done" ? "✓" : run.status === "error" ? "✗" : "◌";
  const label = `${glyph} ${run.agent}${run.status === "error" && run.error ? ` · ${run.error}` : ""}`;
  const hasReport = !!run.report?.trim();

  return (
    <div className="activity-row">
      <button
        className="activity-row-head"
        onClick={() => hasReport && setOpen((o) => !o)}
        aria-expanded={hasReport ? open : undefined}
        data-static={!hasReport}
      >
        <span className="activity-step-label">{label}</span>
        {hasReport && (
          <span className="activity-caret" data-open={open}>
            <ChevronIcon size={11} />
          </span>
        )}
      </button>
      {open && hasReport && (
        <div className="activity-output">
          <AssistantText id={`agent:${run.callId}:${run.agent}`} text={run.report!} />
        </div>
      )}
    </div>
  );
}
