import { useEffect, useRef, useState } from "react";
import { post } from "../lib/host";
import { openFilePreview } from "../lib/filePreview";
import type { LoginState } from "../lib/login";
import type { ReviewData, ToolCall, Turn, TurnBlock } from "../lib/thread";
import type { Provider } from "../../src/protocol";
import { ReplyContext } from "../lib/reply";
import { useStickToBottom } from "../lib/useStickToBottom";
import { NewItemsPill } from "../interior/new-items-pill";
import { ApprovalPrompt } from "./ApprovalPrompt";
import { CopyButton } from "./CopyButton";
import { ErrorBox } from "./ErrorBox";
import { InfoCard } from "./InfoCard";
import { Markdown } from "./Markdown";
import { ReasoningBlock } from "./ReasoningBlock";
import { QuestionPrompt } from "./QuestionPrompt";
import { ReviewTurn } from "./ReviewTurn";
import { SetupCard } from "./SetupCard";
import { UserText } from "./UserText";
import { UserTurnActions } from "./UserTurnActions";
import { StatusLine } from "./StatusLine";
import { AgentGroup } from "./AgentGroup";
import { GoalCard } from "./GoalCard";
import { ToolGroup } from "./ToolGroup";
import { RoundGroup } from "./RoundGroup";

const isUser = (turn: Turn) => turn.role === "user";

type RenderBlock =
  | TurnBlock
  | {
      kind: "round";
      id: string;
      reasoning: Extract<TurnBlock, { kind: "reasoning" }>;
      calls: ToolCall[];
    };

/** Pair a thought with the steps it produced. Prose, agents and other blocks
 *  break the pairing, so only a thought that directly precedes tools folds. */
function withRounds(blocks: TurnBlock[]): RenderBlock[] {
  const out: RenderBlock[] = [];
  for (let i = 0; i < blocks.length; i++) {
    const block = blocks[i];
    const next = blocks[i + 1];
    if (block.kind === "reasoning" && next?.kind === "tools") {
      out.push({ kind: "round", id: block.id, reasoning: block, calls: next.calls });
      i++;
    } else {
      out.push(block);
    }
  }
  return out;
}

export function Thread({
  turns,
  login,
  providers,
  busy,
  grouping,
  onApproval,
  onRedirect,
  onAnswer,
  editing,
  onEditStart,
  onFork,
  onRetry,
  onReviewChange,
  onSend,
}: {
  turns: Turn[];
  login: LoginState | null;
  providers: Provider[];
  busy: boolean;
  grouping: boolean;
  onApproval: (allow: boolean, always?: boolean) => void;
  onRedirect: (instead: string) => void;
  onAnswer: (choice: string | null) => void;
  editing: string | null;
  onEditStart: (id: string) => void;
  onFork: (id: string) => void;
  onRetry: (id: string) => void;
  onReviewChange: (id: string, patch: (data: ReviewData) => ReviewData) => void;
  onSend: (text: string) => void;
}) {
  const { viewport, content, atBottom, onScroll, scrollToBottom } = useStickToBottom();

  // Turns that arrived while the reader was scrolled up become the pill's
  // count; being at the bottom means there is nothing unread.
  const [unread, setUnread] = useState(0);
  const seen = useRef(turns.length);
  useEffect(() => {
    const added = turns.length - seen.current;
    seen.current = turns.length;
    if (atBottom) {
      setUnread(0);
    } else if (added > 0) {
      setUnread((n) => n + added);
    }
  }, [turns.length, atBottom]);

  const sent = useRef(turns.filter(isUser).length);
  useEffect(() => {
    const count = turns.filter(isUser).length;
    const mine = count > sent.current;
    sent.current = count;
    if (mine) scrollToBottom(false);
  }, [turns, scrollToBottom]);

  return (
    <div className="thread-container">
      <div className="thread-viewport" ref={viewport} onScroll={onScroll}>
        <div className="thread" ref={content}>
          {turns.map((turn) => {
            if (turn.role === "user") {
              return (
                <div
                  key={turn.id}
                  className={editing === turn.id ? "turn-user-wrap editing" : "turn-user-wrap"}
                >
                  <div className="turn-user">
                    <div className="turn-user-text">
                      <UserText text={turn.text} />
                    </div>
                  </div>
                  <UserTurnActions
                    text={turn.text}
                    busy={busy}
                    onEdit={() => onEditStart(turn.id)}
                    onFork={() => onFork(turn.id)}
                  />
                </div>
              );
            }
            if (turn.role === "review") {
              return (
                <ReviewTurn
                  key={turn.id}
                  data={turn.data}
                  onChange={(patch) => onReviewChange(turn.id, patch)}
                />
              );
            }
            if (turn.role === "info") {
              return <InfoCard key={turn.id} turn={turn} />;
            }
            if (turn.role === "divider") {
              return (
                <div key={turn.id} className="turn-divider" role="separator">
                  <span className="turn-divider-line" />
                  <span className="turn-divider-label">{turn.label}</span>
                  <span className="turn-divider-line" />
                </div>
              );
            }
            // Where the history the model sees restarts. What is above stays
            // readable, so the break has to be legible as one.
            if (turn.role === "compaction") {
              return (
                <div key={turn.id} className="turn-divider turn-compaction" role="separator">
                  <span className="turn-divider-line" />
                  <span className="turn-divider-label">
                    Compacted {turn.folded} {turn.folded === 1 ? "message" : "messages"}
                  </span>
                  <span className="turn-divider-line" />
                </div>
              );
            }
            return (
              <ReplyContext.Provider
                key={turn.id}
                value={{ streaming: !!turn.pending, send: onSend }}
              >
                <div className="turn-assistant">
                  {/* Arrival order, so steps sit under the thought that led to them. */}
                  {withRounds(turn.blocks).map((block) =>
                    block.kind === "round" ? (
                      <RoundGroup
                        key={block.id}
                        text={block.reasoning.text}
                        tokens={block.reasoning.tokens}
                        durationMs={block.reasoning.durationMs}
                        done={block.reasoning.done || !turn.pending}
                        calls={block.calls}
                        grouped={grouping}
                        live={Boolean(turn.pending)}
                      />
                    ) : block.kind === "tools" ? (
                      <ToolGroup
                        key={block.id}
                        calls={block.calls}
                        grouped={grouping}
                        live={Boolean(turn.pending)}
                      />
                    ) : block.kind === "reasoning" ? (
                      <ReasoningBlock
                        key={block.id}
                        text={block.text}
                        tokens={block.tokens}
                        durationMs={block.durationMs}
                        done={block.done || !turn.pending}
                      />
                    ) : block.kind === "agents" ? (
                      <AgentGroup key={block.id} tasks={block.tasks} />
                    ) : block.kind === "goal" ? (
                      <GoalCard key={block.id} block={block} />
                    ) : block.kind === "injected" ? (
                      <div key={block.id} className="turn-user turn-user-injected">
                        <div className="turn-user-text">
                          <UserText text={block.text} />
                        </div>
                      </div>
                    ) : (
                      <div key={block.id} className="turn-body">
                        <Markdown text={block.text} />
                      </div>
                    )
                  )}

                  {turn.approval && (
                    <ApprovalPrompt
                      ask={turn.approval}
                      onRespond={onApproval}
                      onRedirect={onRedirect}
                    />
                  )}

                  {turn.question && (
                    <QuestionPrompt question={turn.question} onAnswer={onAnswer} />
                  )}

                  {turn.setup ? (
                    <SetupCard login={login} providers={providers} />
                  ) : (
                    turn.errorMsg && (
                      <ErrorBox
                        message={turn.errorMsg}
                        onRetry={busy ? undefined : () => onRetry(turn.id)}
                      />
                    )
                  )}

                  {/* Stays up for the whole turn, including the gap between the last
                      tool result and the reply, which otherwise reads as a hang. */}
                  {turn.pending && !turn.approval && !turn.question && <StatusLine />}

                  {turn.stopped && <div className="turn-stopped">Stopped</div>}

                  {turn.edits && turn.edits.length > 0 && (
                    <div className="edits">
                      <span className="edits-label">Edited</span>
                      {[...new Set(turn.edits)].map((path) => (
                        <button
                          key={path}
                          className="edit-chip"
                          onClick={() => openFilePreview(path)}
                        >
                          {path}
                        </button>
                      ))}
                    </div>
                  )}

                  {/* Last, so the zero-height row hangs in the gap below the turn
                      rather than over whatever else trails it. */}
                  {!turn.pending && turn.text && (
                    <div className="turn-actions">
                      <CopyButton text={turn.text} label="Copy reply" />
                    </div>
                  )}
                </div>
              </ReplyContext.Provider>
            );
          })}
        </div>
      </div>

      {/* A sibling of the scroller, not a child: an absolutely positioned child
          of a scroll container scrolls away with the content. */}
      <NewItemsPill show={!atBottom} count={unread} onJump={() => scrollToBottom()} />
    </div>
  );
}
