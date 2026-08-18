import { useState } from "react";
import { runLabel, type ToolRun as Run } from "../lib/tools";
import { ToolCallRow } from "./ToolCallRow";
import { AlertIcon, ChevronIcon, LayersIcon } from "./icons";

/**
 * A folded run of same-tool steps. Open while it is still working, so progress
 * stays visible, then closed once it lands — until the reader says otherwise,
 * after which their choice sticks.
 */
export function ToolRun({ run }: { run: Run }) {
  const [choice, setChoice] = useState<boolean>();
  const running = run.calls.some((call) => call.result === undefined);
  const failures = run.calls.filter((call) => call.error === true).length;
  const open = choice ?? (running || failures > 0);

  return (
    <div className="tool-run" data-error={failures > 0} data-running={running}>
      <button
        className="tool-row"
        onClick={() => setChoice(!open)}
        aria-expanded={open}
        title={open ? "Fold these steps" : "Show these steps"}
      >
        <span className="tool-icon">{failures > 0 ? <AlertIcon /> : <LayersIcon />}</span>
        <span className="tool-label">
          <span className="tool-verb">{runLabel(run.name, run.calls.length)}</span>
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
