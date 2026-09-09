import type { RefObject } from "react";
import { ChevronIcon, TrashIcon } from "./icons";
import { Markdown } from "./Markdown";

export interface MemoryBody {
  body?: string;
  error?: string;
}

/** One remembered thing. Closed it is a name, an age, and the one line that
 *  says what it is; opened it is the fact itself, in the place the summary
 *  was, so the list never jumps somewhere else to answer the question. */
export function MemoryRow({
  seat,
  name,
  meta,
  description,
  open,
  body,
  cursor,
  onEnter,
  onToggle,
  onForget,
}: {
  seat?: RefObject<HTMLDivElement | null>;
  name: string;
  meta: string;
  description: string;
  open: boolean;
  body?: MemoryBody;
  cursor?: boolean;
  onEnter?: () => void;
  onToggle: () => void;
  onForget?: () => void;
}) {
  return (
    <div
      ref={seat}
      className="memory-row"
      data-cursor={cursor}
      data-open={open}
      onMouseEnter={onEnter}
    >
      <div className="memory-line">
        <button className="memory-open" aria-expanded={open} onClick={onToggle}>
          <span className="memory-chevron">
            <ChevronIcon open={open} />
          </span>
          <span className="memory-name">{name}</span>
        </button>
        <span className="memory-when">{meta}</span>
        {onForget && (
          <button
            className="icon-btn memory-forget"
            title="Forget"
            aria-label={`Forget ${name}`}
            onClick={onForget}
          >
            <TrashIcon />
          </button>
        )}
      </div>

      {description && !open && <p className="memory-desc">{description}</p>}

      {open && (
        <div className="memory-body">
          {body?.error ? (
            <p className="memory-desc">{body.error}</p>
          ) : body?.body === undefined ? (
            <p className="memory-desc">Reading…</p>
          ) : (
            <Markdown text={body.body} doc />
          )}
        </div>
      )}
    </div>
  );
}
