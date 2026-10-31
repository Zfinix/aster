import type { ToolCall } from "../lib/thread";
import { isSerious } from "../lib/tools";
import { ReasoningBlock } from "./ReasoningBlock";
import { ToolGroup } from "./ToolGroup";

/** One round of work: the thought and the steps it led to, sharing one node on
 *  the rail. A recall/remember loop is a sequence of rounds, so keeping each
 *  pair together stops it reading as an endless alternation. */
export function RoundGroup({
  text,
  tokens,
  durationMs,
  done,
  calls,
  grouped,
  live,
}: {
  text: string;
  tokens?: number;
  durationMs?: number;
  done?: boolean;
  calls: ToolCall[];
  grouped: boolean;
  live: boolean;
}) {
  const running = live && calls.some((call) => call.result === undefined && !call.stopped);
  const failed = calls.some((call) => call.error === true);
  const serious = calls.some(isSerious);

  return (
    <div className="round" data-running={running} data-error={failed} data-serious={serious}>
      <ReasoningBlock
        text={text}
        tokens={tokens}
        durationMs={durationMs}
        done={done || !live}
      />
      <ToolGroup calls={calls} grouped={grouped} live={live} />
    </div>
  );
}
