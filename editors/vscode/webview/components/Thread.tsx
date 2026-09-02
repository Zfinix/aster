import { useEffect, useRef, useState } from "react";
import { post } from "../lib/host";
import type { Turn } from "../lib/thread";
import { useStickToBottom } from "../lib/useStickToBottom";
import { NewItemsPill } from "../interior/new-items-pill";
import { ApprovalPrompt } from "./ApprovalPrompt";
import { CopyButton } from "./CopyButton";
import { ErrorBox } from "./ErrorBox";
import { InfoCard } from "./InfoCard";
import { Markdown } from "./Markdown";
import { QueuedTurn } from "./QueuedTurn";
import { ReasoningBlock } from "./ReasoningBlock";
import { QuestionPrompt } from "./QuestionPrompt";
import { ReviewTurn } from "./ReviewTurn";
import { StatusLine } from "./StatusLine";
import { AgentGroup } from "./AgentGroup";
import { GoalCard } from "./GoalCard";
import { ToolGroup } from "./ToolGroup";

const isUser = (turn: Turn) => turn.role === "user";

export function Thread({
  turns,
  queued,
  onApproval,
  onRedirect,
  onAnswer,
  onUnqueue,
}: {
  turns: Turn[];
  queued: string[];
  onApproval: (allow: boolean, always?: boolean) => void;
  onRedirect: (instead: string) => void;
  onAnswer: (choice: string | null) => void;
  onUnqueue: (index: number) => void;
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
                <div key={turn.id} className="turn-user">
                  <div className="turn-user-text">{turn.text}</div>
                </div>
              );
            }
            if (turn.role === "review") {
              return <ReviewTurn key={turn.id} data={turn.data} />;
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
            // An `agent` call's swarm card says everything its tool row would,
            // so the row is dropped once the card exists.
            const swarmCalls = new Set(
              turn.blocks.flatMap((b) => (b.kind === "agents" ? [b.callId] : []))
            );
            return (
              <div key={turn.id} className="turn-assistant">
                {/* Arrival order, so steps sit under the thought that led to them. */}
                {turn.blocks.map((block) =>
                  block.kind === "tools" ? (
                    <ToolGroup
                      key={block.id}
                      calls={block.calls.filter((c) => !swarmCalls.has(c.id))}
                    />
                  ) : block.kind === "reasoning" ? (
                    <ReasoningBlock
                      key={block.id}
                      text={block.text}
                      tokens={block.tokens}
                      durationMs={block.durationMs}
                      done={block.done}
                    />
                  ) : block.kind === "agents" ? (
                    <AgentGroup key={block.id} tasks={block.tasks} />
                  ) : block.kind === "goal" ? (
                    <GoalCard key={block.id} block={block} />
                  ) : block.kind === "injected" ? (
                    <div key={block.id} className="turn-user turn-user-injected">
                      <div className="turn-user-text">{block.text}</div>
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

                {turn.errorMsg && <ErrorBox message={turn.errorMsg} />}

                {/* Stays up for the whole turn, including the gap between the last
                    tool result and the reply, which otherwise reads as a hang. */}
                {turn.pending && !turn.approval && !turn.question && <StatusLine />}

                {turn.stopped && <div className="turn-stopped">Stopped</div>}

                {turn.edits && turn.edits.length > 0 && (
                  <div className="edits">
                    <span className="edits-label">Edited</span>
                    {turn.edits.map((path) => (
                      <button
                        key={path}
                        className="edit-chip"
                        onClick={() => post({ type: "openFile", path })}
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
            );
          })}

          {queued.map((text, i) => (
            <QueuedTurn key={`q${i}`} text={text} onRemove={() => onUnqueue(i)} />
          ))}
        </div>
      </div>

      {/* A sibling of the scroller, not a child: an absolutely positioned child
          of a scroll container scrolls away with the content. */}
      <NewItemsPill show={!atBottom} count={unread} onJump={() => scrollToBottom()} />
    </div>
  );
}
