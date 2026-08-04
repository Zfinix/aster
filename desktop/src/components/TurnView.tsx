import type { Turn } from "../lib/session";
import type { Finding } from "../lib/types";
import { ActivityPanel } from "./ActivityPanel";
import { AgentPanel } from "./AgentPanel";
import { AssistantText } from "./AssistantText";
import { MessageActions } from "./MessageActions";
import { ReviewTurn } from "./ReviewTurn";

export function TurnView({
  turn,
  onOpenDiff,
  onFocusFinding,
  onRetry,
}: {
  turn: Turn;
  onOpenDiff: () => void;
  onFocusFinding: (finding: Finding) => void;
  onRetry: () => void;
}) {
  if (turn.role === "user") {
    return (
      <div className="turn-wrap user">
        <div className="bubble">{turn.text}</div>
        <MessageActions text={turn.text} ts={turn.ts} />
      </div>
    );
  }
  if (turn.role === "assistant") {
    if (turn.pending && !turn.text) {
      return (
        <div className="a-typing">
          <span />
          <span />
          <span />
        </div>
      );
    }
    return (
      <div className="turn-wrap">
        <div className="a-turn">
          {turn.steps && turn.steps.length > 0 && <ActivityPanel steps={turn.steps} />}
          {turn.agents && turn.agents.length > 0 && <AgentPanel agents={turn.agents} />}
          <AssistantText id={turn.id} text={turn.text} error={turn.error} />
          {turn.stopped && <div className="a-stopped">Stopped</div>}
        </div>
        {!turn.pending && <MessageActions text={turn.text} ts={turn.ts} />}
      </div>
    );
  }
  return (
    <ReviewTurn
      data={turn.data}
      onOpenDiff={onOpenDiff}
      onFocusFinding={onFocusFinding}
      onRetry={onRetry}
    />
  );
}
