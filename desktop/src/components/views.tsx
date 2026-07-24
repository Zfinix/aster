import { useState } from "react";
import {
  latestReview,
  type Conversation,
  type ReviewData,
  type Turn,
} from "../lib/session";
import type { Finding, Severity } from "../lib/types";
import { severityOf, SEV_RANK, SEV_LABEL } from "../lib/severity";
import { Composer, type ComposerBinding } from "./Composer";
import { Mark } from "./Mark";
import { ListIcon } from "./icons";

const SEV_CLS: Record<Severity, string> = {
  critical: "crit",
  high: "high",
  medium: "med",
  low: "low",
  info: "info",
};

const SEV_WORD: Record<Severity, string> = {
  critical: "critical",
  high: "high",
  medium: "medium",
  low: "low",
  info: "info",
};

function money(n: number | null | undefined): string {
  if (n == null) return "";
  return `$${n.toFixed(n < 0.01 ? 4 : 3)}`;
}

function topSeverity(findings: Finding[]): Severity {
  return findings
    .map((f) => severityOf(f.severity))
    .sort((a, b) => SEV_RANK[a] - SEV_RANK[b])[0];
}

function clause(text: string): string {
  const first = text.split(/(?<=[.!?])\s/)[0] ?? text;
  return first.length > 110 ? `${first.slice(0, 107)}…` : first;
}

/* ---- Home ---- */

export function HomeView({
  composer,
  conversations,
  onOpen,
}: {
  composer: ComposerBinding;
  conversations: Conversation[];
  onOpen: (id: string) => void;
}) {
  return (
    <div className="hero">
      <Mark px={3.2} interactive />
      <h1>What should we review next?</h1>
      <Composer variant="home" {...composer} />

      {conversations.length > 0 && (
        <div className="home-work">
          <div className="h-label">Recent</div>
          {conversations.slice(0, 6).map((c) => {
            const review = latestReview(c);
            return (
              <button key={c.id} className="h-row" onClick={() => onOpen(c.id)}>
                <div className="h-main">
                  <div className="h-title">{c.title}</div>
                  <div className="h-meta">
                    {c.whenLabel} · {c.repoName}
                  </div>
                </div>
                {review && <ReviewBadge review={review} />}
                <span className="h-stats">
                  {review
                    ? `${review.findings.length} finding${review.findings.length === 1 ? "" : "s"}${
                        review.usage?.estimated_cost_usd != null
                          ? ` · ${money(review.usage.estimated_cost_usd)}`
                          : ""
                      }`
                    : `${c.turns.length} message${c.turns.length === 1 ? "" : "s"}`}
                </span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

function ReviewBadge({ review }: { review: ReviewData }) {
  if (review.status === "error") {
    return (
      <span className="review-badge crit">
        <span className="badge-dot" />
        failed
      </span>
    );
  }
  if (review.status === "running") {
    return (
      <span className="review-badge med">
        <span className="badge-dot" />
        running
      </span>
    );
  }
  if (review.findings.length === 0) {
    return (
      <span className="review-badge clean">
        <span className="badge-dot" />
        Clean
      </span>
    );
  }
  const top = topSeverity(review.findings);
  const count = review.findings.filter((f) => severityOf(f.severity) === top).length;
  const cls = top === "critical" || top === "high" ? "crit" : "med";
  return (
    <span className={`review-badge ${cls}`}>
      <span className="badge-dot" />
      {count} {SEV_WORD[top]}
    </span>
  );
}

/* ---- Thread ---- */

export function ThreadView({
  conversation,
  onOpenDiff,
  onRetry,
}: {
  conversation: Conversation;
  onOpenDiff: () => void;
  onRetry: () => void;
}) {
  return (
    <div className="chat">
      {conversation.turns.map((t) => (
        <TurnView
          key={t.id}
          turn={t}
          onOpenDiff={onOpenDiff}
          onRetry={onRetry}
        />
      ))}
    </div>
  );
}

function TurnView({
  turn,
  onOpenDiff,
  onRetry,
}: {
  turn: Turn;
  onOpenDiff: () => void;
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
    return (
      <div className="a-text" style={turn.error ? { color: "var(--red)" } : undefined}>
        {turn.text.split("\n\n").map((p, i) => (
          <p key={i}>{p}</p>
        ))}
      </div>
    );
  }
  return <ReviewTurn data={turn.data} onOpenDiff={onOpenDiff} onRetry={onRetry} />;
}

function ReviewTurn({
  data,
  onOpenDiff,
  onRetry,
}: {
  data: ReviewData;
  onOpenDiff: () => void;
  onRetry: () => void;
}) {
  const [refutedOpen, setRefutedOpen] = useState(false);
  const [findingsOpen, setFindingsOpen] = useState(true);

  if (data.status === "running") {
    return (
      <div className="a-run">
        <Mark px={2} className="a-run-mark" label="Aster working" />
        <span>
          {data.phase || "Reviewing the diff"}
          {data.findings.length > 0 && ` · ${data.findings.length} so far`}
        </span>
      </div>
    );
  }

  if (data.status === "error") {
    return (
      <div className="err-card">
        <div className="err-head">Review failed</div>
        <pre className="err-body">{data.errorMsg || "aster exited with an error"}</pre>
        <button className="btn btn-line" onClick={onRetry}>
          Retry
        </button>
      </div>
    );
  }

  const adds = data.files.reduce((n, f) => n + f.additions, 0);
  const dels = data.files.reduce((n, f) => n + f.deletions, 0);

  return (
    <>
      <div className="a-meta">
        Reviewed
        {data.usage?.estimated_cost_usd != null && ` · ${money(data.usage.estimated_cost_usd)}`}
      </div>

      {data.summary && (
        <div className="a-text">
          <p>{data.summary}</p>
        </div>
      )}

      {data.findings.length > 0 ? (
        <div className={`tsect ${findingsOpen ? "open" : ""}`}>
          <button
            className="tsect-toggle"
            aria-expanded={findingsOpen}
            onClick={() => setFindingsOpen((o) => !o)}
          >
            Findings · {data.findings.length} <span className="f-chev">▾</span>
          </button>
          {findingsOpen && (
            <ul className="a-list">
              {data.findings.map((f, i) => {
                const sev = severityOf(f.severity);
                return (
                  <li key={i}>
                    <span className={`sev ${SEV_CLS[sev]}`}>{SEV_LABEL[sev]}</span>
                    <span className="li-text">
                      <b>{f.title}</b>
                      {f.description ? `, ${clause(f.description)} ` : " "}
                      <span className="li-loc">
                        {f.file_path.split("/").pop()}:{f.line}
                      </span>
                    </span>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      ) : (
        <div className="a-text">
          <p>No findings survived verification. Clean diff.</p>
        </div>
      )}

      {data.files.length > 0 && (
        <button className="chip-card" onClick={onOpenDiff}>
          <span className="cc-ic">
            <ListIcon />
          </span>
          Changed {data.files.length} file{data.files.length === 1 ? "" : "s"}
          <span className="cc-stats">
            <span className="plus">+{adds}</span>
            <span className="minus">-{dels}</span>
          </span>
        </button>
      )}

      {data.refuted.length > 0 && (
        <div className={`refuted ${refutedOpen ? "open" : ""}`}>
          <button className="refuted-toggle" onClick={() => setRefutedOpen((o) => !o)}>
            Refuted by verifier · {data.refuted.length} <span className="f-chev">▾</span>
          </button>
          {refutedOpen && (
            <div className="refuted-list">
              {data.refuted.map((r, i) => (
                <div key={i} className="refuted-row">
                  <span className="x">✕</span>
                  <span className="rt">{r.title}</span>
                  <span className="why">{r.reason}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </>
  );
}
