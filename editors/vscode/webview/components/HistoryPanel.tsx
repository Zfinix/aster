import { useMemo, useState } from "react";
import type { SessionSummary } from "../../src/protocol";
import { useListNav } from "../lib/listnav";
import { filterSessions, relativeTime } from "../lib/history";
import { PencilIcon, SearchIcon, TrashIcon } from "./icons";
import { Modal } from "./Modal";

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
  /** The row being renamed, if any: its id and the name typed so far. */
  const [editing, setEditing] = useState<{ id: string; title: string }>();

  const rows = useMemo(() => filterSessions(sessions, query), [sessions, query]);

  const open = (id: string) => {
    onPick(id);
    onClose();
  };

  const { active: cursor, setActive: setCursor, leave, onKey, seat } = useListNav<HTMLDivElement>({
    count: rows.length,
    resetOn: query,
    onPick: (index) => open(rows[index].id),
  });

  const commit = () => {
    if (editing && editing.title.trim()) {
      onRename(editing.id, editing.title.trim());
    }
    setEditing(undefined);
  };

  return (
    <Modal label="Session history" className="history-panel" onClose={onClose}>
      {/* Keys are read off the whole panel, so the arrows work from the search
          box and from a row's buttons alike. While a name is being typed they
          belong to that input. */}
      <div
        className="history-keys"
        onKeyDown={(e) => {
          if (!editing) onKey(e);
        }}
      >
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

        <div className="history-list" role="listbox" onMouseLeave={leave}>
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
                ref={seat(at)}
                className="history-row"
                role="option"
                aria-selected={s.id === activeId}
                data-cursor={at === cursor}
                data-active={s.id === activeId}
                data-editing={editing?.id === s.id}
                onMouseEnter={() => setCursor(at)}
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
                      onClick={() => open(s.id)}
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
    </Modal>
  );
}
