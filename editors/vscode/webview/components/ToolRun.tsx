import { useState } from "react";
import { runSummary, type ToolRun as Run } from "../lib/tools";
import { ToolCallRow } from "./ToolCallRow";

/** A folded run of steps. It stays closed until the reader opens it; the header
 *  still says when a step is running or failed. */
export function ToolRun({ run, live = true }: { run: Run; live?: boolean }) {
  const [open, setOpen] = useState(false);
  const running = live && run.calls.some((call) => call.result === undefined && !call.stopped);
  const failures = run.calls.filter((call) => call.error === true).length;
  const { verb, detail } = runSummary(run.name, run.calls.length, run.label);

  return (
    <div className="tool-run" data-error={failures > 0} data-running={running}>
      <button
        className="tool-row"
        onClick={() => setOpen(!open)}
        aria-expanded={open}
        title={open ? "Fold these steps" : "Show these steps"}
      >
        <span className="tool-label">
          <span className="tool-verb">{verb}</span>
          <span className="tool-detail"> {detail}</span>
        </span>
        <span className="tool-hint">
          {running ? "running…" : failures > 0 ? `${failures} failed` : undefined}
        </span>
      </button>

      {open && (
        <div className="tool-run-list">
          {run.calls.map((call) => (
            <ToolCallRow key={call.id} call={call} nested live={live} />
          ))}
        </div>
      )}
    </div>
  );
}
