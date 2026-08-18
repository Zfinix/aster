import { useEffect, useMemo, useRef, useState } from "react";
import { useDismiss } from "../lib/dismiss";
import { modelShort } from "../lib/model";

/** Unfiltered, an endpoint like OpenRouter lists hundreds; the search box is
 *  right there, and the note says how many are behind it. */
const SHOWN = 20;

interface Row {
  id: string;
  label: string;
  detail: string;
}

/**
 * Model picker: the endpoint's own catalog, searchable, with the vetted models
 * first and a checkmark on the active one. The catalog is re-read each time
 * this opens, so a provider switch or a newly released model shows up without
 * anyone having to know its id.
 */
export function ModelPicker({
  model,
  models,
  recommended,
  loading,
  error,
  onSelect,
  onRefresh,
  onClose,
}: {
  model: string | null;
  models: string[];
  recommended: string[];
  loading: boolean;
  error?: string;
  onSelect: (model: string) => void;
  onRefresh: () => void;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  useDismiss(ref, onClose);

  useEffect(() => {
    inputRef.current?.focus();
    onRefresh();
  }, []);

  useEffect(() => setActive(0), [query]);

  const { top, rest, hidden } = useMemo(() => {
    const q = query.trim().toLowerCase();
    // The label is humanized, so the exact id is what the second line is for.
    const rows = models.map((id) => ({ id, label: modelShort(id), detail: id }));
    // Matching the id as well as the readable name means "sonnet", "anthropic",
    // and "claude-sonnet-5" all land on the same row.
    const hits = q
      ? rows.filter((r) => r.id.toLowerCase().includes(q) || r.label.toLowerCase().includes(q))
      : rows;

    const isTop = (r: Row) => recommended.includes(r.id);
    const top = hits.filter(isTop);
    const all = hits.filter((r) => !isTop(r));
    return { top, rest: q ? all : all.slice(0, SHOWN), hidden: q ? 0 : Math.max(0, all.length - SHOWN) };
  }, [models, recommended, query]);

  // `Default` is a row like any other, so arrow keys reach it.
  const flat: (Row | null)[] = [null, ...top, ...rest];
  const typed = query.trim();
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

        <div className="cmd-section">{row(null, "default")}</div>

        {top.length > 0 && (
          <div className="cmd-section">
            <div className="cmd-section-title">Recommended</div>
            {top.map((r) => row(r, r.id))}
          </div>
        )}

        {rest.length > 0 && (
          <div className="cmd-section">
            <div className="cmd-section-title">{query ? "Matches" : "Available"}</div>
            {rest.map((r) => row(r, r.id))}
            {hidden > 0 && <div className="cmd-note">{hidden} more; type to search</div>}
          </div>
        )}

        {custom && <div className="cmd-section">{row(custom, "custom")}</div>}

        {options.length === 1 && !loading && (
          <div className="cmd-empty">No model matches. Type a full id to use it anyway.</div>
        )}
      </div>
    </div>
  );
}
