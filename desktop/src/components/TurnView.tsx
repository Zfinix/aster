import type { Turn } from "../lib/session";
import type { Finding } from "../lib/types";
import { AgentList } from "./AgentList";
import { AssistantText } from "./AssistantText";
import { Mark } from "./Mark";
import { ReasoningPanel } from "./ReasoningPanel";
import { ReviewTurn } from "./ReviewTurn";
import { ToolList } from "./ToolList";
import { TurnActions } from "./TurnActions";
import { UserTurnActions, type UserTurnActionHandlers } from "./UserTurnActions";

export function TurnView({
  turn,
  onOpenDiff,
  onFocusFinding,
  onApplyFix,
  onRetry,
  actions,
}: {
  turn: Turn;
  actions?: UserTurnActionHandlers;
  onOpenDiff: () => void;
  onFocusFinding: (finding: Finding) => void;
  onApplyFix: (finding: Finding) => Promise<boolean>;
  onRetry: () => void;
}) {
  if (turn.role === "user") {
    if (actions) {
      return (
        <div className="turn">
          <UserTurnActions text={turn.text} ts={turn.ts} actions={actions} />
        </div>
      );
    }
    return (
      <div className="turn">
        <div className="turn-user">
          <div className="turn-user-text">{turn.text}</div>
        </div>
        <TurnActions text={turn.text} ts={turn.ts} />
      </div>
    );
  }
  if (turn.role === "assistant") {
    const hasBlocks = turn.reasoning || turn.steps?.length || turn.agents?.length || turn.text;
    return (
      <div className="turn">
        <div className="turn-assistant">
          {turn.reasoning && (
            <ReasoningPanel
              text={turn.reasoning}
              tokens={turn.reasoningTokens}
              durationMs={turn.reasoningDurationMs}
              done={turn.reasoningDone}
            />
          )}
          {turn.steps && turn.steps.length > 0 && <ToolList steps={turn.steps} pending={turn.pending} />}
          {turn.agents && turn.agents.length > 0 && <AgentList agents={turn.agents} />}
          {turn.text && <AssistantText text={turn.text} error={turn.error} />}
          {turn.pending && (
            <div className="status">
              <Mark px={1} label="Aster working" />
              <span className="shimmer">{hasBlocks ? "Working" : "Thinking"}</span>
            </div>
          )}
          {turn.stopped && <div className="turn-stopped">Stopped</div>}
        </div>
        {!turn.pending && <TurnActions text={turn.text} ts={turn.ts} />}
      </div>
    );
  }
  return <ReviewTurn data={turn.data} onOpenDiff={onOpenDiff} onFocusFinding={onFocusFinding} onApplyFix={onApplyFix} onRetry={onRetry} />;
}
