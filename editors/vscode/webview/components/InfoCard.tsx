import type { InfoTurn } from "../lib/thread";
import { inEditor, post } from "../lib/host";
import { ExternalIcon } from "./icons";
import { InfoBody } from "./InfoBody";

export function InfoCard({ turn }: { turn: InfoTurn }) {
  return (
    <div className="info-card" data-error={turn.error === true}>
      <div className="info-head">
        <span className="info-title">{turn.title}</span>
        {inEditor && turn.body && (
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

      <InfoBody card={turn} />
    </div>
  );
}
