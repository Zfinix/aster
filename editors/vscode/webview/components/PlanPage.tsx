import { useEffect, useState } from "react";
import { closePlan, onPlanAnswer, onPlanChange, readPlan, sendPlanAnswer } from "../lib/plan-tab";
import { Mark } from "./Mark";
import { Markdown } from "./Markdown";

/**
 * The `/plan` tab: a plan read as a document, with the decision at the foot of
 * it. Only `aster serve` opens this — an editor has tabs of its own.
 */
export function PlanPage() {
  const [markdown, setMarkdown] = useState(readPlan);
  const [answered, setAnswered] = useState<boolean | null>(null);
  const [elsewhere, setElsewhere] = useState(false);

  useEffect(() => {
    document.title = "Plan";
  }, []);

  useEffect(
    () =>
      onPlanChange((next) => {
        // Cleared means the thread answered while this tab sat open; a new plan
        // means the agent revised it, and this tab is where it lands.
        if (!next) {
          setElsewhere(true);
          return;
        }
        setMarkdown(next);
        setAnswered(null);
        setElsewhere(false);
      }),
    []
  );

  // The other tab's answer, so two open tabs never disagree about the plan.
  useEffect(() => onPlanAnswer(() => setElsewhere(true)), []);

  const decide = (allow: boolean) => {
    sendPlanAnswer(allow);
    closePlan();
    setAnswered(allow);
    // The tab exists for this decision, so it goes with it and hands the reader
    // back to the thread. A tab the browser refuses to close keeps the line
    // below instead.
    window.opener?.focus();
    window.close();
  };

  const done = answered !== null || elsewhere;

  return (
    <div className="plan-page">
      <header className="plan-page-head">
        <Mark px={2} />
        <span className="plan-page-title">Plan</span>
      </header>

      <article className="plan-page-doc">
        {markdown ? (
          <Markdown text={markdown} doc />
        ) : (
          <p className="plan-page-empty">
            No plan open. This tab opens itself when Aster puts one up for approval.
          </p>
        )}
      </article>

      {markdown && (
        <footer className="plan-page-actions">
          <div className="plan-page-bar">
            {done ? (
              <span className="plan-page-verdict">
                {elsewhere && answered === null
                  ? "Answered in the thread. You can close this tab."
                  : answered
                    ? "Approved. Back to the thread."
                    : "Rejected. Back to the thread."}
              </span>
            ) : (
              <>
                <span className="plan-page-verdict">Approve this plan and start editing?</span>
                <button className="btn" onClick={() => decide(false)}>
                  Reject
                </button>
                <button className="btn-primary" onClick={() => decide(true)}>
                  Approve
                </button>
              </>
            )}
          </div>
        </footer>
      )}
    </div>
  );
}
