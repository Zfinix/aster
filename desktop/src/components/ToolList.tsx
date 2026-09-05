import type { ToolStep } from "../lib/session";
import { ToolRow } from "./ToolRow";

/** The steps a turn took, one row each on a shared rail. */
export function ToolList({ steps, pending }: { steps: ToolStep[]; pending?: boolean }) {
  if (!steps.length) return null;
  return (
    <div className="tool-list">
      {steps.map((s) => (
        <ToolRow key={s.id} step={s} running={!!pending && s.output === undefined} />
      ))}
    </div>
  );
}
