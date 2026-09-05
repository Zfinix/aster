import type { AgentRun } from "../lib/session";
import { AssistantText } from "./AssistantText";
import { ToolRow } from "./ToolRow";
import { AgentIcon } from "./icons";

const STATE: Record<AgentRun["status"], string> = {
  running: "running…",
  done: "done",
  error: "failed",
};

/** The sub-agents a turn ran, one row each, opening into their reports. */
export function AgentList({ agents }: { agents: AgentRun[] }) {
  if (!agents.length) return null;
  return (
    <div className="tool-list">
      {agents.map((a) => (
        <ToolRow
          key={`${a.callId}:${a.agent}`}
          step={{ id: a.callId, name: "agent", label: a.agent, output: a.report }}
          running={a.status === "running"}
          icon={<AgentIcon />}
          verb={a.agent}
          detail={a.task ?? ""}
          hint={a.status === "error" && a.error ? a.error : STATE[a.status]}
          card={
            a.report?.trim() ? (
              <AssistantText text={a.report} />
            ) : undefined
          }
        />
      ))}
    </div>
  );
}
