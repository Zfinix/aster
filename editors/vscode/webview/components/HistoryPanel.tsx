import { useEffect, useMemo, useRef, useState } from "react";
import type { SessionSummary } from "../../src/protocol";
import { SearchIcon } from "./icons";
import { groupSessions, relativeTime } from "../lib/history";

/**
 * Session history, as a command-palette popup: type to filter, arrows to move,
 * Enter to open. Rows group under when they happened, so the list reads as a
 * timeline rather than an undifferentiated stack.
 */
export function HistoryPanel({
  sessions,
  activeId,
  onPick,
  onClose,
}: {
  sessions: SessionSummary[];
  activeId: string | null;
  onPick: (id: string) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [cursor, setCursor] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  const groups = useMemo(() => groupSessions(sessions, query), [sessions, query]);
  const flat = useMemo(() => groups.flatMap((g) => g.sessions), [groups]);

  // A shorter result set must never leave the cursor pointing past the end.
  useEffect(() => setCursor(0), [query]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose();
        return;
      }
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setCursor((c) => {
          if (flat.length === 0) return 0;
          const next = e.key === "ArrowDown" ? c + 1 : c - 1;
          return (next + flat.length) % flat.length;
        });
        return;
      }
      if (e.key === "Enter" && flat[cursor]) {
        e.preventDefault();
        onPick(flat[cursor].id);
        onClose();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose, onPick, flat, cursor]);

  useEffect(() => {
    listRef.current
      ?.querySelector<HTMLElement>('[data-cursor="true"]')
      ?.scrollIntoView({ block: "nearest" });
  }, [cursor]);

  let index = -1;
  return (
    <div
      className="history-overlay"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div className="history-panel" role="dialog" aria-label="Session history">
        <div className="history-search">
          <SearchIcon />
          <input
            placeholder="Search sessions"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            autoFocus
            spellCheck={false}
            aria-label="Search sessions"
          />
          {sessions.length > 0 && (
            <span className="history-count">
              {flat.length}/{sessions.length}
            </span>
          )}
        </div>

        <div className="history-list" ref={listRef} role="listbox">
          {flat.length === 0 ? (
            <div className="history-empty">
              {sessions.length === 0
                ? "No saved sessions in this repo yet."
                : `Nothing matches "${query}".`}
            </div>
          ) : (
            groups.map((group) => (
              <div className="history-group" key={group.label}>
                <div className="history-group-label">{group.label}</div>
                {group.sessions.map((s) => {
                  index += 1;
                  const at = index;
                  return (
                    <button
                      key={s.id}
                      className="history-row"
                      role="option"
                      aria-selected={s.id === activeId}
                      data-cursor={at === cursor}
                      data-active={s.id === activeId}
                      onMouseMove={() => setCursor(at)}
                      onClick={() => {
                        onPick(s.id);
                        onClose();
                      }}
                    >
                      <span className="history-row-main">
                        <span className="history-row-title">
                          {s.title || "Untitled session"}
                        </span>
                        <span className="history-row-when">
                          {relativeTime(s.created_at)}
                        </span>
                      </span>
                      <span className="history-row-meta">
                        {s.turns} turn{s.turns === 1 ? "" : "s"}
                        {s.model ? <span className="history-dot">·</span> : null}
                        {s.model}
                      </span>
                    </button>
                  );
                })}
              </div>
            ))
          )}
        </div>

        <div className="history-foot">
          <kbd>↑↓</kbd> navigate <kbd>↵</kbd> open <kbd>esc</kbd> close
        </div>
      </div>
    </div>
  );
}
