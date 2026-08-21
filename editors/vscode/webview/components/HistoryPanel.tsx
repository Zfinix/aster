import { useEffect, useMemo, useRef, useState } from "react";
import type { SessionSummary } from "../../src/protocol";
import { PencilIcon, SearchIcon, TrashIcon } from "./icons";
import { filterSessions, relativeTime } from "../lib/history";

/**
 * Session history, as a command-palette popup: type to filter, arrows to move,
 * Enter to open. One line a session, because a list you scan for a title reads
 * faster without a second line of metadata under every row. Rename and delete
 * live on the row and appear on hover, so the list stays a list until you want
 * to change something in it.
 */
export function HistoryPanel({
  sessions,
  activeId,
  onPick,
  onRename,
  onDelete,
  onClose,
}: {
  sessions: SessionSummary[];
  activeId: string | null;
  onPick: (id: string) => void;
  onRename: (id: string, title: string) => void;
  onDelete: (id: string) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [cursor, setCursor] = useState(0);
  /** The row being renamed, if any: its id and the name typed so far. */
  const [editing, setEditing] = useState<{ id: string; title: string }>();
  const listRef = useRef<HTMLDivElement>(null);

  const rows = useMemo(() => filterSessions(sessions, query), [sessions, query]);

  // A shorter result set must never leave the cursor pointing past the end.
  useEffect(() => setCursor(0), [query]);

  const commit = () => {
    if (editing && editing.title.trim()) {
      onRename(editing.id, editing.title.trim());
    }
    setEditing(undefined);
  };

  useEffect(() => {
    // While a name is being typed, the list's own keys belong to the input.
    if (editing) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose();
        return;
      }
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setCursor((c) => {
          if (rows.length === 0) return 0;
          const next = e.key === "ArrowDown" ? c + 1 : c - 1;
          return (next + rows.length) % rows.length;
        });
        return;
      }
      if (e.key === "Enter" && rows[cursor]) {
        e.preventDefault();
        onPick(rows[cursor].id);
        onClose();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose, onPick, rows, cursor, editing]);

  useEffect(() => {
    listRef.current
      ?.querySelector<HTMLElement>('[data-cursor="true"]')
      ?.scrollIntoView({ block: "nearest" });
  }, [cursor]);

  return (
    <div
      className="history-overlay"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div className="history-panel" role="dialog" aria-label="Session history">
        <div className="history-search">
          <SearchIcon />
          <input
            placeholder="Search sessions…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            autoFocus
            spellCheck={false}
            aria-label="Search sessions"
          />
        </div>

        <div className="history-list" ref={listRef} role="listbox">
          {rows.length === 0 ? (
            <div className="history-empty">
              {sessions.length === 0
                ? "No saved sessions in this repo yet."
                : `Nothing matches "${query}".`}
            </div>
          ) : (
            rows.map((s, at) => (
              <div
                key={s.id}
                className="history-row"
                role="option"
                aria-selected={s.id === activeId}
                data-cursor={at === cursor}
                data-active={s.id === activeId}
                data-editing={editing?.id === s.id}
                onMouseMove={() => setCursor(at)}
              >
                {editing?.id === s.id ? (
                  <input
                    className="history-rename"
                    value={editing.title}
                    autoFocus
                    spellCheck={false}
                    aria-label="Session name"
                    onChange={(e) => setEditing({ id: s.id, title: e.target.value })}
                    onBlur={commit}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
                        commit();
                      }
                      if (e.key === "Escape") {
                        e.preventDefault();
                        e.stopPropagation();
                        setEditing(undefined);
                      }
                    }}
                  />
                ) : (
                  <>
                    <button
                      className="history-open"
                      // What the row no longer spends a second line on.
                      title={`${s.turns} turn${s.turns === 1 ? "" : "s"}${s.model ? ` · ${s.model}` : ""}`}
                      onClick={() => {
                        onPick(s.id);
                        onClose();
                      }}
                    >
                      <span className="history-row-title">{s.title || "Untitled session"}</span>
                    </button>
                    <span className="history-row-when">{relativeTime(s.created_at)}</span>
                    <span className="history-row-actions">
                      <button
                        className="icon-btn"
                        title="Rename"
                        aria-label="Rename session"
                        onClick={() => setEditing({ id: s.id, title: s.title })}
                      >
                        <PencilIcon />
                      </button>
                      <button
                        className="icon-btn history-delete"
                        title="Delete"
                        aria-label="Delete session"
                        onClick={() => onDelete(s.id)}
                      >
                        <TrashIcon />
                      </button>
                    </span>
                  </>
                )}
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
