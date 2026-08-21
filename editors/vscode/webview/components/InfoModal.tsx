import { useEffect } from "react";
import type { InfoCardData } from "../lib/thread";
import { inEditor, post } from "../lib/host";
import { ExternalIcon } from "./icons";
import { InfoBody } from "./InfoBody";

/**
 * `/status`, `/memory`, `/diff`: a look at the session rather than part of it,
 * so it opens over the thread and closes without leaving anything behind.
 */
export function InfoModal({ card, onClose }: { card: InfoCardData; onClose: () => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="info-overlay"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div className="info-modal" role="dialog" aria-label={card.title} data-error={card.error === true}>
        <div className="info-head">
          <span className="info-title">{card.title}</span>
          {inEditor && card.body && (
            <button
              className="icon-btn"
              onClick={() =>
                post({ type: "openUntitled", content: card.body ?? "", lang: card.lang, title: card.title })
              }
              title="Open in editor"
              aria-label="Open in editor"
            >
              <ExternalIcon />
            </button>
          )}
        </div>

        <div className="info-modal-body">
          <InfoBody card={card} />
        </div>
      </div>
    </div>
  );
}
