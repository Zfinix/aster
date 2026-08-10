import { useState } from "react";
import type { InfoTurn } from "../lib/thread";
import { post } from "../lib/host";
import { ChevronIcon, ExternalIcon } from "./icons";
import { StatusLine } from "./StatusLine";
import { ToolOutput } from "./ToolOutput";

/** Long bodies (a whole `git diff`) stay folded; rows are the whole point of
 *  their card, so they do not. */
const BODY_PREVIEW_LINES = 24;

export function InfoCard({ turn }: { turn: InfoTurn }) {
  const [expanded, setExpanded] = useState(false);
  const lines = turn.body?.split("\n") ?? [];
  const long = lines.length > BODY_PREVIEW_LINES;
  const shown = long && !expanded ? lines.slice(0, BODY_PREVIEW_LINES).join("\n") : turn.body;

  return (
    <div className="info-card" data-error={turn.error === true}>
      <div className="info-head">
        <span className="info-title">{turn.title}</span>
        {turn.body && (
          <button
            className="icon-btn"
            onClick={() =>
              post({ type: "openUntitled", content: turn.body ?? "", lang: turn.lang, title: turn.title })
            }
            title="Open in editor"
            aria-label="Open in editor"
          >
            <ExternalIcon />
          </button>
        )}
      </div>

      {turn.pending && <StatusLine />}
      {turn.note && <div className="info-note">{turn.note}</div>}

      {turn.rows && turn.rows.length > 0 && (
        <dl className="info-rows">
          {turn.rows.map((row) => (
            <div key={row.label} className="info-row">
              <dt>{row.label}</dt>
              <dd>{row.value}</dd>
            </div>
          ))}
        </dl>
      )}

      {shown && (
        <>
          <ToolOutput output={shown} lang={turn.lang} />
          {long && (
            <button className="info-more" onClick={() => setExpanded(!expanded)}>
              <ChevronIcon open={expanded} />
              {expanded
                ? "Show less"
                : `${lines.length - BODY_PREVIEW_LINES} more lines`}
            </button>
          )}
        </>
      )}
    </div>
  );
}
