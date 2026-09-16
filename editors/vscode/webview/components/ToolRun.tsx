import { useState } from "react";
import { runLabel, type ToolRun as Run } from "../lib/tools";
import { ToolCallRow } from "./ToolCallRow";
import { AlertIcon, ChevronIcon, LayersIcon } from "./icons";

/** A folded run of steps. It stays closed until the reader opens it; the header
 *  still says when a step is running or failed. */
export function ToolRun({ run }: { run: Run }) {
  const [open, setOpen] = useState(false);
  const running = run.calls.some((call) => call.result === undefined && !call.stopped);
  const failures = run.calls.filter((call) => call.error === true).length;

  return (
    <div className="tool-run" data-error={failures > 0} data-running={running}>
      <button
        className="tool-row"
        onClick={() => setOpen(!open)}
        aria-expanded={open}
        title={open ? "Fold these steps" : "Show these steps"}
      >
        <span className="tool-icon">{failures > 0 ? <AlertIcon /> : <LayersIcon />}</span>
        <span className="tool-label">
          <span className="tool-verb">{run.label ?? runLabel(run.name, run.calls.length)}</span>
        </span>
        <span className="tool-hint">
          {running ? "running…" : failures > 0 ? `${failures} failed` : undefined}
        </span>
        <span className="tool-chevron">
          <ChevronIcon open={open} />
        </span>
      </button>

      {open && (
        <div className="tool-run-list">
          {run.calls.map((call) => (
            <ToolCallRow key={call.id} call={call} nested />
          ))}
        </div>
      )}
    </div>
  );
}
