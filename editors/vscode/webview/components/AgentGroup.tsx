import { useState } from "react";
import { post } from "../lib/host";
import type { AgentTaskState } from "../lib/thread";
import { Disclosure } from "../interior/disclosure";
import { ChevronIcon, ExternalIcon } from "./icons";
import { Markdown } from "./Markdown";

/** One `agent` tool call's swarm: a summary line over per-agent rows with live
 *  status; each finished report expands inline and opens as a markdown tab. */
export function AgentGroup({ tasks }: { tasks: AgentTaskState[] }) {
  const [open, setOpen] = useState(false);
  const settled = tasks.filter((t) => t.status !== "running").length;
  const failed = tasks.filter((t) => t.status === "error").length;
  const names = [...new Set(tasks.map((t) => t.agent))];

  return (
    <div className="agent-group">
      <button className="agent-group-head" onClick={() => setOpen(!open)} aria-expanded={open}>
        <span className="agent-group-title">Agent</span>
        <span className="agent-group-summary">
          {names.length > 1 ? `×${names.length} ` : ""}
          {names.join(", ")}
          {failed > 0 ? ` · ${failed} failed` : ""}
          <span className="agent-group-count"> {settled}/{tasks.length}</span>
        </span>
        <span className="agent-group-caret">
          <ChevronIcon open={open} />
        </span>
      </button>
      <Disclosure open={open}>
        <div className="agent-card">
          {tasks.map((t) => (
            <AgentRow key={`${t.callId}:${t.agent}`} task={t} />
          ))}
        </div>
      </Disclosure>
    </div>
  );
}

function AgentRow({ task }: { task: AgentTaskState }) {
  const [open, setOpen] = useState(false);
  const glyph = task.status === "done" ? "✓" : task.status === "error" ? "✗" : "◌";
  const errorText = task.status === "error" ? ` · ${task.error ?? "failed"}` : "";
  const hasReport = Boolean(task.report?.trim());

  return (
    <div className="agent-row" data-error={task.status === "error"} data-running={task.status === "running"}>
      <div className="agent-row-head">
        <button
          className="agent-row-toggle"
          onClick={() => hasReport && setOpen(!open)}
          disabled={!hasReport}
          aria-expanded={hasReport ? open : undefined}
        >
          <span className="agent-row-glyph">{glyph}</span>
          <span className="agent-row-label">{task.agent}</span>
          {errorText && <span className="agent-row-error">{errorText}</span>}
          {hasReport && (
            <span className="agent-row-caret">
              <ChevronIcon open={open} />
            </span>
          )}
        </button>
        {hasReport && (
          <button
            className="icon-btn agent-row-open"
            onClick={() =>
              post({
                type: "openUntitled",
                content: task.report!,
                lang: "markdown",
                title: task.agent ? `${task.agent} report` : "report",
              })
            }
            title="Open report in a markdown tab"
            aria-label="Open report in a markdown tab"
          >
            <ExternalIcon />
          </button>
        )}
      </div>
      <Disclosure open={open}>
        <div className="agent-report">
          <Markdown text={task.report!} />
        </div>
      </Disclosure>
    </div>
  );
}
