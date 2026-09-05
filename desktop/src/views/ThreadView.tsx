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
  onApplyFix,
  onRetry,
  actionsDisabled,
  onEditSend,
  onFork,
  onRewind,
}: {
  conversation: Conversation;
  actionsDisabled: boolean;
  onEditSend: (turnId: string, text: string) => void;
  onFork: (turnId: string) => void;
  onRewind: (turnId: string) => void;
  approval: { preview: string; markdown?: string } | null;
  onRespondApproval: (allow: boolean) => void;
  onOpenDiff: () => void;
  onFocusFinding: (finding: Finding) => void;
  onApplyFix: (finding: Finding) => Promise<boolean>;
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
    <div className="thread-container">
      <div className="thread-viewport">
        <div className="thread">
          {turns.map((t) => (
            <TurnView key={t.id} turn={t} actions={t.role === "user" ? { disabled: actionsDisabled, onEditSend: (text) => onEditSend(t.id, text), onFork: () => onFork(t.id), onRewind: () => onRewind(t.id) } : undefined} onOpenDiff={onOpenDiff} onFocusFinding={onFocusFinding} onApplyFix={onApplyFix} onRetry={onRetry} />
          ))}
          {approval && (
            <ApprovalPrompt preview={approval.preview} markdown={approval.markdown} onRespond={onRespondApproval} />
          )}
          <div ref={endRef} />
        </div>
      </div>
    </div>
  );
}
