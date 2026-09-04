import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useListNav } from "../lib/listnav";
import { ChoiceSlider } from "./ChoiceSlider";

/** One step of an inline dial, e.g. an effort level. */
export interface MenuOption {
  value: string;
  label: string;
}

export type MenuItem =
  | {
      kind: "action";
      id: string;
      label: string;
      detail?: string;
      hint?: string;
      icon?: ReactNode;
      slash?: string;
      takesArg?: boolean;
      keepOpen?: boolean;
      run: (rest: string) => void;
    }
  | {
      kind: "choice";
      id: string;
      label: string;
      value: string;
      options: MenuOption[];
      icon?: ReactNode;
      onSelect: (value: string) => void;
    }
  | {
      kind: "toggle";
      id: string;
      label: string;
      on: boolean;
      icon?: ReactNode;
      onToggle: (on: boolean) => void;
    };

export interface MenuSection {
  title?: string;
  items: MenuItem[];
  limit?: number;
}

const KEYS = ["ArrowDown", "ArrowUp", "ArrowLeft", "ArrowRight", "Enter", "Tab"];

/** The panel's command surface: everything it can do, ranked against a query.
 *  Either the composer drives the filter, because the query is a `/name` being
 *  typed there, or the menu owns an input of its own. */
export function CommandMenu({
  sections,
  query,
  onRun,
}: {
  sections: MenuSection[];
  query: string | null;
  onRun: (item: MenuItem, complete: boolean) => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [typed, setTyped] = useState("");
  const driven = query !== null;
  const text = driven ? query : typed;

  useEffect(() => {
    if (!driven) inputRef.current?.focus();
  }, [driven]);

  const filtered = useMemo(() => rank(sections, text), [sections, text]);
  const flat = useMemo(() => filtered.flatMap((s) => s.items), [filtered]);

  const { active, setActive, leave, onKey: navKey, seat } = useListNav<HTMLDivElement>({
    count: flat.length,
    resetOn: text,
    tabCompletes: true,
    onPick: (index, complete) => {
      const item = flat[index];
      if (item.kind === "choice") step(item, 1);
      else if (item.kind === "toggle") item.onToggle(!item.on);
      else onRun(item, complete);
    },
  });
  const current = flat[active];

  const step = (item: Extract<MenuItem, { kind: "choice" }>, by: number) => {
    const at = item.options.findIndex((o) => o.value === item.value);
    const next = item.options[(at + by + item.options.length) % item.options.length];
    item.onSelect(next.value);
  };

  const onKey = (e: KeyboardEvent | React.KeyboardEvent) => {
    if ((e.key === "ArrowRight" || e.key === "ArrowLeft") && current?.kind === "choice") {
      e.preventDefault();
      step(current, e.key === "ArrowRight" ? 1 : -1);
      return;
    }
    if ((e.key === "ArrowRight" || e.key === "ArrowLeft") && current?.kind === "toggle") {
      e.preventDefault();
      current.onToggle(e.key === "ArrowRight");
      return;
    }
    navKey(e);
  };

  // Driven from the composer, the keyboard never leaves it: the menu reads the
  // keys it owns off the document before the textarea acts on them.
  const keyRef = useRef(onKey);
  keyRef.current = onKey;
  useEffect(() => {
    if (!driven) return;
    const handler = (e: KeyboardEvent) => {
      if (KEYS.includes(e.key)) keyRef.current(e);
    };
    document.addEventListener("keydown", handler, true);
    return () => document.removeEventListener("keydown", handler, true);
  }, [driven]);

  let index = -1;

  return (
    <div className="cmd" role="dialog" aria-label="Commands">
      {!driven && (
        <input
          ref={inputRef}
          className="cmd-filter"
          placeholder="Filter actions…"
          spellCheck={false}
          value={typed}
          onChange={(e) => setTyped(e.currentTarget.value)}
          onKeyDown={onKey}
        />
      )}

      <div className="cmd-list" role="listbox" onMouseLeave={leave}>
        {flat.length === 0 && <div className="cmd-empty">No matching commands.</div>}
        {filtered.map((section) => (
          <div key={section.title ?? "top"} className="cmd-section">
            {section.title && <div className="cmd-section-title">{section.title}</div>}
            {section.items.map((item) => {
              index += 1;
              const at = index;
              return item.kind === "choice" ? (
                <div
                  key={item.id}
                  ref={seat(at)}
                  className="cmd-row cmd-row-choice"
                  data-active={at === active}
                  onMouseEnter={() => setActive(at)}
                >
                  <span className="cmd-icon">{item.icon}</span>
                  <span className="cmd-body">
                    <span className="cmd-label">{item.label}</span>
                    <span className="cmd-detail">
                      ({item.options.find((o) => o.value === item.value)?.label ?? item.value})
                    </span>
                  </span>
                  <ChoiceSlider
                    label={item.label}
                    options={item.options}
                    value={item.value}
                    onSelect={item.onSelect}
                  />
                </div>
              ) : item.kind === "toggle" ? (
                <div
                  key={item.id}
                  ref={seat(at)}
                  className="cmd-row cmd-row-choice"
                  data-active={at === active}
                  onMouseEnter={() => setActive(at)}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    item.onToggle(!item.on);
                  }}
                >
                  <span className="cmd-icon">{item.icon}</span>
                  <span className="cmd-label">{item.label}</span>
                  <span className="switch" role="switch" aria-checked={item.on} aria-label={item.label}>
                    <span className="switch-knob" />
                  </span>
                </div>
              ) : (
                <div
                  key={item.id}
                  ref={seat(at)}
                  className="cmd-row"
                  role="option"
                  aria-selected={at === active}
                  data-active={at === active}
                  onMouseEnter={() => setActive(at)}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    onRun(item, false);
                  }}
                >
                  <span className="cmd-icon">{item.icon}</span>
                  <span className="cmd-body">
                    <span className="cmd-label">{item.label}</span>
                    {item.detail && <span className="cmd-detail">{item.detail}</span>}
                  </span>
                  {item.hint && <span className="cmd-hint">{item.hint}</span>}
                </div>
              );
            })}
          </div>
        ))}
      </div>
    </div>
  );
}

/** Rank every row against the query and drop the ones that miss. A name beats a
 *  description, and a section floats to wherever its best row landed, so `/mod`
 *  puts the model rows on top without hiding a skill that also says "model". */
export function rank(sections: MenuSection[], query: string): MenuSection[] {
  const q = query.trim().replace(/^\//, "").toLowerCase();
  if (!q) return sections.filter((section) => section.items.length > 0).map(capped);

  return sections
    .map((section) => {
      const scored = section.items
        .map((item) => ({ item, score: score(item, section.title, q) }))
        .filter((row): row is { item: MenuItem; score: number } => row.score !== null)
        .sort((a, b) => a.score - b.score);
      return {
        title: section.title,
        items: scored.map((row) => row.item),
        best: scored[0]?.score ?? Infinity,
      };
    })
    .filter((section) => section.items.length > 0)
    .sort((a, b) => a.best - b.best)
    .map(({ title, items }) => ({ title, items }));
}

function capped(section: MenuSection): MenuSection {
  const limit = section.limit;
  if (!limit || section.items.length <= limit) return section;
  return { ...section, items: section.items.slice(0, limit) };
}

function score(item: MenuItem, title: string | undefined, q: string): number | null {
  const names = [item.label, item.kind === "action" ? item.slash : undefined, item.id]
    .filter((name): name is string => Boolean(name))
    .map((name) => name.replace(/^\//, "").toLowerCase());
  const detail = item.kind === "action" ? item.detail?.toLowerCase() : undefined;

  if (names.some((name) => name === q)) return 0;
  if (names.some((name) => name.startsWith(q))) return 1;
  if (names.some((name) => name.includes(q))) return 2;
  if (title?.toLowerCase().includes(q)) return 3;
  if (detail?.includes(q)) return 4;
  // Initials and dropped letters, so `wlc` still finds `write-like-chizi`. One
  // letter would match nearly everything, so it takes two.
  if (q.length > 1 && names.some((name) => subsequence(name, q))) return 5;
  return null;
}

function subsequence(name: string, q: string): boolean {
  let at = 0;
  for (const char of name) {
    if (char === q[at]) at += 1;
    if (at === q.length) return true;
  }
  return false;
}
