import { useState } from "react";
import type { InfoCardData } from "../lib/thread";
import { ChevronIcon } from "./icons";
import { Markdown } from "./Markdown";
import { StatusLine } from "./StatusLine";
import { ToolOutput } from "./ToolOutput";

const BODY_PREVIEW_LINES = 24;

/** What a local answer says, without the frame around it: the same rows and
 *  body whether they are shown inline or over the thread. */
export function InfoBody({ card }: { card: InfoCardData }) {
  const [expanded, setExpanded] = useState(false);
  const lines = card.body?.split("\n") ?? [];
  // A document is read from the top, so folding it would hide the thing it was
  // opened for.
  const long = !card.doc && lines.length > BODY_PREVIEW_LINES;
  const shown = long && !expanded ? lines.slice(0, BODY_PREVIEW_LINES).join("\n") : card.body;

  return (
    <>
      {card.pending && <StatusLine />}
      {card.note && <div className="info-note">{card.note}</div>}

      {card.rows && card.rows.length > 0 && (
        <dl className="info-rows">
          {card.rows.map((row) => (
            <div key={row.label} className="info-row">
              <dt>{row.label}</dt>
              <dd>{row.value}</dd>
            </div>
          ))}
        </dl>
      )}

      {shown && card.doc && <Markdown text={shown} doc />}

      {shown && !card.doc && (
        <>
          <ToolOutput output={shown} lang={card.lang} />
          {long && (
            <button className="info-more" onClick={() => setExpanded(!expanded)}>
              <ChevronIcon open={expanded} />
              {expanded ? "Show less" : `${lines.length - BODY_PREVIEW_LINES} more lines`}
            </button>
          )}
        </>
      )}
    </>
  );
}
