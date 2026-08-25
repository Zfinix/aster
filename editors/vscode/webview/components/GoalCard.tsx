import type { GoalOutcome, GoalVerdictRow, TurnBlock } from "../lib/thread";
import { CheckIcon, SpinnerIcon, TargetIcon, XIcon } from "./icons";

type GoalBlock = Extract<TurnBlock, { kind: "goal" }>;

function finalLabel(outcome: GoalOutcome, verdicts: GoalVerdictRow[], maxTurns: number): string {
  const tries = verdicts.at(-1)?.turn ?? 0;
  switch (outcome) {
    case "met":
      return `Reached after ${tries} ${tries === 1 ? "try" : "tries"}`;
    case "impossible":
      return "Can't be reached";
    case "exhausted":
      return `Stopped after ${maxTurns} tries, not there yet`;
    case "stopped":
      return "Stopped";
  }
}

/** A judged goal loop as a card: the condition, one timeline row per verdict
 *  with the judge's reason, and a terminal row once the loop resolves. */
export function GoalCard({ block }: { block: GoalBlock }) {
  const running = block.outcome === undefined;

  return (
    <div className="goal-card" data-outcome={block.outcome ?? "running"}>
      <div className="goal-head">
        <span className="goal-title">
          <TargetIcon />
          Goal
        </span>
        <span className="goal-condition" title={block.condition}>
          {block.condition}
        </span>
        <span className="goal-state">
          {running ? <SpinnerIcon /> : block.outcome === "met" ? <CheckIcon /> : <XIcon />}
        </span>
      </div>
      {block.verdicts.length > 0 && (
        <div className="goal-timeline">
          {block.verdicts.map((v, i) => (
            <div key={i} className="goal-row" data-verdict={v.verdict}>
              <span className="goal-row-dot" />
              <span className="goal-row-turn">#{v.turn}</span>
              <span className="goal-row-reason">{v.reason}</span>
            </div>
          ))}
        </div>
      )}
      {block.outcome && (
        <div className="goal-final">
          {finalLabel(block.outcome, block.verdicts, block.maxTurns)}
        </div>
      )}
    </div>
  );
}
