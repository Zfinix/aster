import { useState } from "react";
import type { RuledOut } from "../../src/types";
import { post } from "../lib/host";
import { Disclosure } from "../interior/disclosure";
import { ChevronIcon, ExternalIcon } from "./icons";

const VERDICT: Record<NonNullable<RuledOut["kind"]>, string> = {
  not_real: "Not a real problem",
  unsure: "Too unsure to report",
  check_failed: "Couldn't be checked",
};

/** A possible issue the review ruled out, on the finding row's geometry but
 *  quieter: no fill until hovered, one line until opened for the reason. */
export function RefutedItem({ number, item }: { number: number; item: RuledOut }) {
  const [open, setOpen] = useState(false);
  const file = item.file_path?.split("/").pop();
  const where = file && item.line ? `${file}:${item.line}` : file;
  // An unsure verdict's reason is the gate's own arithmetic; the percentage
  // in the verdict line already says it in words.
  const reason = item.kind === "unsure" ? null : item.reason;

  return (
    <div className="refuted-item" data-open={open}>
      <button className="refuted-row" onClick={() => setOpen(!open)} aria-expanded={open}>
        <span className="refuted-mark">{number}</span>
        <span className="refuted-title">{item.title}</span>
        {where && (
          <span className="finding-loc" title={item.file_path}>
            {where}
          </span>
        )}
        <span className="finding-caret">
          <ChevronIcon open={open} />
        </span>
      </button>
      <Disclosure open={open}>
        <div className="refuted-detail">
          {item.kind && (
            <span className="refuted-verdict" data-kind={item.kind}>
              {VERDICT[item.kind]}
              {item.kind === "unsure" &&
                item.confidence != null &&
                ` · ${Math.round(item.confidence * 100)}% sure`}
            </span>
          )}
          {reason && <p className="refuted-reason">{reason}</p>}
          {item.file_path && (
            <div className="finding-actions">
              <span className="finding-tags">
                {[item.severity, item.category].filter(Boolean).join(" · ")}
              </span>
              <button
                className="btn"
                title={`Open ${item.file_path}`}
                onClick={() =>
                  post({
                    type: "openFinding",
                    finding: {
                      file_path: item.file_path ?? "",
                      line: item.line ?? 0,
                      severity: item.severity ?? "info",
                      category: item.category ?? "",
                      title: item.title,
                      description: item.reason,
                      suggestion: "",
                    },
                  })
                }
              >
                <ExternalIcon />
                Open file
              </button>
            </div>
          )}
        </div>
      </Disclosure>
    </div>
  );
}
