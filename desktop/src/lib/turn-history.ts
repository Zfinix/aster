import type { Conversation } from "./session";

export function branchAtTurn(
  conversation: Conversation,
  turnId: string,
  action: "edit" | "fork" | "rewind",
  forkId: string,
): Conversation | null {
  const index = conversation.turns.findIndex((t) => t.id === turnId && t.role === "user");
  if (index < 0) return null;
  return {
    ...conversation,
    id: action === "fork" ? forkId : conversation.id,
    sessionId: undefined,
    whenLabel: "now",
    turns: conversation.turns.slice(0, index + (action === "edit" ? 0 : 1)),
  };
}
