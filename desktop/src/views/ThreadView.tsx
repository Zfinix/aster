import { useEffect, useRef } from "react";
import type { Conversation } from "../lib/session";
import type { Finding } from "../lib/types";
import { TurnView } from "../components/TurnView";
import { ApprovalPrompt } from "../components/ApprovalPrompt";

export function ThreadView({
  conversation,
  approval,
  onRespondApproval,
  onOpenDiff,
  onFocusFinding,
  onRetry,
}: {
  conversation: Conversation;
  approval: { preview: string } | null;
  onRespondApproval: (allow: boolean) => void;
  onOpenDiff: () => void;
  onFocusFinding: (finding: Finding) => void;
  onRetry: () => void;
}) {
  const endRef = useRef<HTMLDivElement>(null);
  const turns = conversation.turns;
  const last = turns[turns.length - 1];
  const pending = last?.role === "assistant" && last.pending;

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "end", behavior: "smooth" });
  }, [conversation.id, turns.length, pending, last?.role === "assistant" ? last.text : undefined]);

  return (
    <div className="chat">
      {turns.map((t) => (
        <TurnView
          key={t.id}
          turn={t}
          onOpenDiff={onOpenDiff}
          onFocusFinding={onFocusFinding}
          onRetry={onRetry}
        />
      ))}
      {approval && (
        <ApprovalPrompt preview={approval.preview} onRespond={onRespondApproval} />
      )}
      <div ref={endRef} />
    </div>
  );
}
