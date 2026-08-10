import type { ToolCall } from "../lib/thread";
import { groupRuns, isRun } from "../lib/tools";
import { PlanCard } from "./PlanCard";
import { ToolCallRow } from "./ToolCallRow";
import { ToolRun } from "./ToolRun";

/** A stretch of tool calls in the order they happened, with repeated tools
 *  folded into a single run. Plan updates render inline as a PlanCard where
 *  they landed, the same as any other tool output. */
export function ToolGroup({ calls }: { calls: ToolCall[] }) {
  return (
    <div className="tool-list">
      {groupRuns(calls).map((item) =>
        isRun(item) ? (
          <ToolRun key={item.id} run={item} />
        ) : item.name === "update_plan" ? (
          <PlanCard key={item.id} call={item} />
        ) : (
          <ToolCallRow key={item.id} call={item} />
        )
      )}
    </div>
  );
}
