import { useEffect, useState } from "react";
import { Button, Card } from "@heroui/react";
import type { ReviewData } from "../lib/session";
import type { Finding, Severity } from "../lib/types";
import { severityOf } from "../lib/severity";
import { money, reviewMessage } from "../lib/review-format";
import { Mark } from "./Mark";

const SEV_VAR: Record<Severity, string> = {
  critical: "var(--sev-crit)",
  high: "var(--sev-high)",
  medium: "var(--sev-med)",
  low: "var(--sev-low)",
  info: "var(--faint)",
};

/** One finding as a flat row: severity dot · title · location. Clicking the
 *  row reveals the why; the diff jump lives inside the expansion. */
function FindingRow({ f, onFocus }: { f: Finding; onFocus: (f: Finding) => void }) {
  const [open, setOpen] = useState(false);
  const sev = severityOf(f.severity);
  return (
    <li
      className="f rise"
      style={{ ["--sev" as string]: SEV_VAR[sev] }}
    >
      <button
        className="f-row"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
      >
        <span className="f-dot" />
        <span className="f-name">{f.title}</span>
        <span className="f-loc">
          {f.file_path.split("/").pop()}:{f.line}
        </span>
      </button>
      {open && (
        <div className="f-why rise">
          <p>{f.description}</p>
          <button className="rt-link" onClick={() => onFocus(f)}>
            Show in diff →
          </button>
        </div>
      )}
    </li>
  );
}

/** The review summary with a light word-by-word entrance. The space lives
 *  outside each animated span — inside an inline-block it would collapse. */
function Say({ text }: { text: string }) {
  return (
    <p className="say">
      {text.split(" ").map((w, i) => (
        <span key={i}>
          <span
            className="say-w"
            style={{ animationDelay: `${Math.min(i * 26, 1100)}ms` }}
          >
            {w}
          </span>{" "}
        </span>
      ))}
    </p>
  );
}

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
  const [elapsed, setElapsed] = useState(0);

  const running = data.status === "running";
  useEffect(() => {
    if (!running) return;
    const t0 = Date.now();
    const id = window.setInterval(
      () => setElapsed((Date.now() - t0) / 1000),
      100,
    );
    return () => window.clearInterval(id);
  }, [running]);

  if (data.status === "running") {
    return (
      <div className="pipe">
        <div className="a-run">
          <Mark px={1.5} className="a-run-mark" label="Aster working" />
          <span className="run-shimmer">{data.phase || "Reading the diff"}</span>
          <span className="run-dot">·</span>
          <span className="run-timer">{elapsed.toFixed(1)}s</span>
        </div>
        {data.findings.length > 0 && (
          <ul className="flist">
            {data.findings.map((f, i) => (
              <FindingRow key={i} f={f} onFocus={onFocusFinding} />
            ))}
          </ul>
        )}
        {data.refuted.length > 0 && (
          <div className="pipe-kills">
            {data.refuted.map((r, i) => (
              <div key={i} className="pipe-kill rise">
                <span className="x">✕</span>
                <span className="rt">{r.title}</span>
                <span className="why">refuted</span>
              </div>
            ))}
          </div>
        )}
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
  const n = data.findings.length;

  return (
    <div className="a-turn review-done">
      <button
        className="rt-label"
        aria-expanded={findingsOpen}
        onClick={() => setFindingsOpen((o) => !o)}
      >
        {n > 0 ? `Findings · ${n}` : "Clean diff"}
        <span className="rt-chev" data-open={findingsOpen}>
          ›
        </span>
      </button>

      <Say text={reviewMessage(data.findings, data.refuted.length)} />

      {findingsOpen && n > 0 && (
        <ul className="flist">
          {data.findings.map((f, i) => (
            <FindingRow key={i} f={f} onFocus={onFocusFinding} />
          ))}
        </ul>
      )}

      <div className="rt-foot">
        {data.files.length > 0 && (
          <button className="rt-link" onClick={onOpenDiff}>
            {data.files.length} file{data.files.length === 1 ? "" : "s"} · +{adds}{" "}
            −{dels}
          </button>
        )}
        {data.usage?.estimated_cost_usd != null && (
          <span>{money(data.usage.estimated_cost_usd)}</span>
        )}
      </div>

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
    </div>
  );
}
