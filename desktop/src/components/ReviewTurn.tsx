import { useState } from "react";
import { Button, Card } from "@heroui/react";
import type { ReviewData } from "../lib/session";
import type { Finding } from "../lib/types";
import { severityOf, SEV_LABEL } from "../lib/severity";
import { money, clause, SEV_CLS, SEV_ICON } from "../lib/review-format";
import { Mark } from "./Mark";
import { ListIcon } from "./icons";

export function ReviewTurn({
  data,
  onOpenDiff,
  onFocusFinding,
  onRetry,
}: {
  data: ReviewData;
  onOpenDiff: () => void;
  onFocusFinding: (finding: Finding) => void;
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
      <Card className="err-card">
        <div className="err-head">Review failed</div>
        <pre className="err-body">{data.errorMsg || "aster exited with an error"}</pre>
        <Button className="btn btn-line" onPress={onRetry}>
          Retry
        </Button>
      </Card>
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
          <Button
            className="tsect-toggle"
            aria-expanded={findingsOpen}
            onPress={() => setFindingsOpen((o) => !o)}
          >
            Findings · {data.findings.length} <span className="f-chev">▾</span>
          </Button>
          {findingsOpen && (
            <ul className="a-list">
              {data.findings.map((f, i) => {
                const sev = severityOf(f.severity);
                return (
                  <li key={i}>
                    <button
                      className="li-jump"
                      onClick={() => onFocusFinding(f)}
                      title="Show in diff"
                    >
                      <b className="li-title">{f.title}</b>
                      {f.description && <span className="li-desc">{clause(f.description)}</span>}
                      <span className="li-meta">
                        <span className={`sev ${SEV_CLS[sev]}`}>
                          {(() => {
                            const Icon = SEV_ICON[sev];
                            return <Icon />;
                          })()}
                          {SEV_LABEL[sev]}
                        </span>
                        <span className="li-loc">
                          {f.file_path.split("/").pop()}:{f.line}
                        </span>
                      </span>
                    </button>
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
        <Button className="chip-card" onPress={onOpenDiff}>
          <span className="cc-ic">
            <ListIcon />
          </span>
          Changed {data.files.length} file{data.files.length === 1 ? "" : "s"}
          <span className="cc-stats">
            <span className="plus">+{adds}</span>
            <span className="minus">-{dels}</span>
          </span>
        </Button>
      )}

      {data.refuted.length > 0 && (
        <div className={`refuted ${refutedOpen ? "open" : ""}`}>
          <Button className="refuted-toggle" onPress={() => setRefutedOpen((o) => !o)}>
            Refuted by verifier · {data.refuted.length} <span className="f-chev">▾</span>
          </Button>
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
