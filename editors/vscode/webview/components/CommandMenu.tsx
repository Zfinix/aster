import { useEffect, useMemo, useRef, useState } from "react";
import { useDismiss } from "../lib/dismiss";

/** One choice in an inline segmented control, e.g. an effort level. */
export interface MenuOption {
  value: string;
  label: string;
}

export type MenuItem =
  /** Runs and closes the menu. A trailing `…` means it opens something. */
  | { kind: "action"; id: string; label: string; detail?: string; hint?: string; run: () => void }
  /** Set inline, without leaving the menu: the value is the whole control. */
  | {
      kind: "choice";
      id: string;
      label: string;
      value: string;
      options: MenuOption[];
      onSelect: (value: string) => void;
    };

export interface MenuSection {
  title?: string;
  items: MenuItem[];
  /** Shown under the section when its list was cut short. */
  note?: string;
}

/**
 * The panel's command surface. A GUI has no reason to make people type a
 * command name at a prompt, so everything the panel can do is listed here,
 * filterable, with the settings that have a current value showing it.
 */
export function CommandMenu({
  sections,
  onClose,
}: {
  sections: MenuSection[];
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  useDismiss(ref, onClose);

  useEffect(() => inputRef.current?.focus(), []);
  useEffect(() => setActive(0), [query]);

  const filtered = useMemo(() => filter(sections, query), [sections, query]);
  const flat = useMemo(() => filtered.flatMap((s) => s.items), [filtered]);
  const current = flat[active];

  /** Enter on a choice steps to its next option, so the row is a control
   *  rather than a dead end; every other row runs and closes. */
  const step = (item: Extract<MenuItem, { kind: "choice" }>, by: number) => {
    const at = item.options.findIndex((o) => o.value === item.value);
    const next = item.options[(at + by + item.options.length) % item.options.length];
    item.onSelect(next.value);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const by = e.key === "ArrowDown" ? 1 : -1;
      setActive((i) => (flat.length ? (i + by + flat.length) % flat.length : 0));
      return;
    }
    if ((e.key === "ArrowRight" || e.key === "ArrowLeft") && current?.kind === "choice") {
      e.preventDefault();
      step(current, e.key === "ArrowRight" ? 1 : -1);
      return;
    }
    if (e.key === "Enter" && current) {
      e.preventDefault();
      if (current.kind === "choice") {
        step(current, 1);
      } else {
        current.run();
        onClose();
      }
    }
  };

  let index = -1;

  return (
    <div className="cmd" ref={ref} role="dialog" aria-label="Commands">
      <input
        ref={inputRef}
        className="cmd-filter"
        placeholder="Filter actions…"
        spellCheck={false}
        value={query}
        onChange={(e) => setQuery(e.currentTarget.value)}
        onKeyDown={onKeyDown}
      />

      <div className="cmd-list" role="listbox">
        {flat.length === 0 && <div className="cmd-empty">No matching actions.</div>}
        {filtered.map((section) => (
          <div key={section.title ?? "top"} className="cmd-section">
            {section.title && <div className="cmd-section-title">{section.title}</div>}
            {section.items.map((item) => {
              index += 1;
              const at = index;
              return item.kind === "choice" ? (
                <div
                  key={item.id}
                  className="cmd-row cmd-row-choice"
                  data-active={at === active}
                  onMouseEnter={() => setActive(at)}
                >
                  <span className="cmd-label">{item.label}</span>
                  <span className="cmd-options">
                    {item.options.map((option) => (
                      <button
                        key={option.value}
                        className="cmd-option"
                        data-selected={option.value === item.value}
                        // Mousedown, not click: the filter must keep focus so
                        // the arrow keys still drive the list afterwards.
                        onMouseDown={(e) => {
                          e.preventDefault();
                          item.onSelect(option.value);
                        }}
                      >
                        {option.label}
                      </button>
                    ))}
                  </span>
                </div>
              ) : (
                <button
                  key={item.id}
                  className="cmd-row"
                  role="option"
                  aria-selected={at === active}
                  data-active={at === active}
                  onMouseEnter={() => setActive(at)}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    item.run();
                    onClose();
                  }}
                >
                  <span className="cmd-body">
                    <span className="cmd-label">{item.label}</span>
                    {item.detail && <span className="cmd-detail">{item.detail}</span>}
                  </span>
                  {item.hint && <span className="cmd-hint">{item.hint}</span>}
                </button>
              );
            })}
            {section.note && <div className="cmd-note">{section.note}</div>}
          </div>
        ))}
      </div>
    </div>
  );
}

/** Case-insensitive substring over the label, its detail, and the section it
 *  sits in, so "mcp" finds the servers row and "model" finds the whole group. */
export function filter(sections: MenuSection[], query: string): MenuSection[] {
  const q = query.trim().toLowerCase();
  if (!q) return sections.filter((s) => s.items.length > 0);
  return sections
    .map((section) => {
      const inTitle = section.title?.toLowerCase().includes(q);
      const items = section.items.filter(
        (item) =>
          inTitle ||
          item.label.toLowerCase().includes(q) ||
          (item.kind === "action" && item.detail?.toLowerCase().includes(q))
      );
      return { ...section, items, note: undefined };
    })
    .filter((section) => section.items.length > 0);
}
