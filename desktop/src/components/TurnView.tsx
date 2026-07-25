import type { Turn } from "../lib/session";
import type { Finding } from "../lib/types";
import { AssistantText } from "./AssistantText";
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
    return <div className="bubble">{turn.text}</div>;
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
    return <AssistantText id={turn.id} text={turn.text} error={turn.error} />;
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
