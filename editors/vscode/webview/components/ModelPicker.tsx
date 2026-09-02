import { useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { useDismiss } from "../lib/dismiss";
import { modelShort } from "../lib/model";

interface Row {
  id: string;
  label: string;
  detail: string;
}

/**
 * Model picker: the vetted coding models, with the endpoint's whole catalog one
 * search away. Listing hundreds of models nobody would pick is not a menu, so
 * the rest of the catalog appears only once there is something to match on. The
 * catalog is re-read each time this opens, so a provider switch or a newly
 * released model shows up without anyone having to know its id.
 */
export function ModelPicker({
  model,
  models,
  recommended,
  recent,
  loading,
  error,
  onSelect,
  onRefresh,
  onClose,
  boundary,
}: {
  model: string | null;
  models: string[];
  recommended: string[];
  recent: string[];
  loading: boolean;
  error?: string;
  onSelect: (model: string) => void;
  onRefresh: () => void;
  onClose: () => void;
  /** What counts as "inside" for a click: the pane this picker shares with the
   *  effort and provider rail, when it has one. */
  boundary?: RefObject<HTMLElement | null>;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  useDismiss(boundary ?? ref, onClose);

  useEffect(() => {
    inputRef.current?.focus();
    onRefresh();
  }, []);

  useEffect(() => setActive(0), [query]);

  const typed = query.trim();

  const { top, rest, recents } = useMemo(() => {
    const q = typed.toLowerCase();
    // The label is humanized, so the exact id is what the second line is for.
    const toRow = (id: string) => ({ id, label: modelShort(id), detail: id });
    const matches = (r: Row) =>
      !q || r.id.toLowerCase().includes(q) || r.label.toLowerCase().includes(q);

    const top = recommended.filter((id) => models.includes(id)).map(toRow).filter(matches);
    // Picked before, minus what Recommended already shows, so no id appears twice.
    const recents = recent
      .filter((id) => models.includes(id) && !recommended.includes(id))
      .map(toRow)
      .filter(matches);
    // Idle, the catalog is only the menu when nothing vetted survived it, which
    // is what a switch to an endpoint with its own ids leaves behind.
    const rest = q
      ? models
          .filter((id) => !recommended.includes(id) && !recent.includes(id))
          .map(toRow)
          .filter(matches)
      : top.length || recents.length
        ? []
        : models.map(toRow);
    return { top, rest, recents };
  }, [models, recommended, recent, typed]);

  // `Default` is a row like any other, so arrow keys reach it — but it answers
  // no search, so a query drops it and Enter lands on the first real match.
  const flat: (Row | null)[] = typed ? [...recents, ...top, ...rest] : [null, ...recents, ...top, ...rest];
  const exact = models.some((m) => m.toLowerCase() === typed.toLowerCase());
  const custom = typed && !exact ? typed : null;
  const options = custom ? [...flat, custom] : flat;

  const choose = (option: (typeof options)[number]) => {
    onSelect(typeof option === "string" ? option : (option?.id ?? ""));
    onClose();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const by = e.key === "ArrowDown" ? 1 : -1;
      setActive((i) => (i + by + options.length) % options.length);
      return;
    }
    if (e.key === "Enter" && options[active] !== undefined) {
      e.preventDefault();
      choose(options[active]);
    }
  };

  // Rows are numbered as they render, so the arrow keys walk what the eye sees
  // rather than a list assembled somewhere else.
  let index = -1;
  const row = (option: Row | string | null, key: string) => {
    index += 1;
    const at = index;
    const custom = typeof option === "string";
    const id = custom ? option : (option?.id ?? "");
    const checked = !custom && id === (model ?? "");
    return (
      <button
        key={key}
        className="picker-row"
        role="menuitemradio"
        aria-checked={checked}
        data-active={at === active}
        title={id || undefined}
        onMouseEnter={() => setActive(at)}
        onMouseDown={(e) => {
          e.preventDefault();
          choose(option);
        }}
      >
        <span className="picker-body">
          <span className="picker-label">
            {custom ? `Use “${option}”` : option ? option.label : "Default"}
          </span>
          {(custom || !option) && (
            <span className="picker-detail">
              {custom
                ? "An id this endpoint did not list; it is remembered once used"
                : "Whatever aster.yaml configures"}
            </span>
          )}
        </span>
        {checked && <span className="picker-check">✓</span>}
      </button>
    );
  };

  return (
    <div className="cmd" ref={ref} role="dialog" aria-label="Model">
      <input
        ref={inputRef}
        className="cmd-filter"
        placeholder={loading ? "Reading the catalog…" : "Search models, or type an id…"}
        spellCheck={false}
        value={query}
        onChange={(e) => setQuery(e.currentTarget.value)}
        onKeyDown={onKeyDown}
      />

      <div className="cmd-list" role="listbox">
        {error && <div className="cmd-note">{error}</div>}

        {!typed && <div className="cmd-section">{row(null, "default")}</div>}

        {recents.length > 0 && (
          <div className="cmd-section">
            <div className="cmd-section-title">Recent</div>
            {recents.map((r) => row(r, `recent:${r.id}`))}
          </div>
        )}

        {top.length > 0 && (
          <div className="cmd-section">
            <div className="cmd-section-title">{typed ? "Recommended" : "Best for coding"}</div>
            {top.map((r) => row(r, r.id))}
          </div>
        )}

        {rest.length > 0 && (
          <div className="cmd-section">
            <div className="cmd-section-title">{typed ? "Matches" : "Available"}</div>
            {rest.map((r) => row(r, r.id))}
          </div>
        )}

        {custom && <div className="cmd-section">{row(custom, "custom")}</div>}
      </div>
    </div>
  );
}
