import type { InfoCardData } from "../lib/thread";
import { inEditor, post } from "../lib/host";
import { ExternalIcon } from "./icons";
import { InfoBody } from "./InfoBody";
import { Modal } from "./Modal";

/**
 * `/status`, `/memory`, `/diff`: a look at the session rather than part of it,
 * so it opens over the thread and closes without leaving anything behind.
 */
export function InfoModal({ card, onClose }: { card: InfoCardData; onClose: () => void }) {
  return (
    <Modal
      label={card.title}
      className={`info-modal${card.error ? " info-modal-error" : ""}${card.doc ? " info-modal-doc" : ""}`}
      align="center"
      onClose={onClose}
    >
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
    </Modal>
  );
}
