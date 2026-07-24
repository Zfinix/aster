import { useEffect, useRef, useState } from "react";
import type { ReviewOpts, SourceKind } from "../lib/types";
import { SOURCE_LABELS, modelShort } from "../lib/session";
import { AttachIcon, RepoIcon, SendIcon } from "./icons";

export type ComposerBinding = Omit<Props, "variant">;

interface Props {
  variant: "home" | "foot";
  prompt: string;
  setPrompt: (s: string) => void;
  onAsk: () => void;
  onReview: () => void;
  busy: boolean;
  reviewing: boolean;
  canReview: boolean;
  opts: ReviewOpts;
  repoName: string;
  repoOptions: { value: string; label: string; hint?: string }[];
  onRepo: (value: string) => void;
  onSource: (kind: SourceKind) => void;
  model: string;
  models: string[];
  onModel: (value: string) => void;
  onAddModel: (value: string) => void;
  onAttach: () => void;
}

const SOURCE_OPTS = (Object.keys(SOURCE_LABELS) as SourceKind[]).map((k) => ({
  value: k,
  label: SOURCE_LABELS[k],
}));

export function Composer(props: Props) {
  const {
    variant,
    prompt,
    setPrompt,
    onAsk,
    onReview,
    busy,
    reviewing,
    canReview,
    opts,
    repoName,
    repoOptions,
    onRepo,
    onSource,
    model,
    models,
    onModel,
    onAddModel,
    onAttach,
  } = props;
  const ref = useRef<HTMLTextAreaElement>(null);

  const canAsk = !!prompt.trim() && !busy;

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 140)}px`;
  }, [prompt]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey && canAsk) {
      e.preventDefault();
      onAsk();
    }
  };

  const plusMenu = (
    <PlusMenu
      opts={opts}
      repoOptions={repoOptions}
      onRepo={onRepo}
      onSource={onSource}
      onAttach={onAttach}
      direction="up"
    />
  );

  const modelPill = (
    <ModelMenu
      model={model}
      models={models}
      onModel={onModel}
      onAddModel={onAddModel}
      direction="up"
    />
  );

  return (
    <div className={variant === "home" ? "home-composer" : ""}>
      <div className="composer">
        <textarea
          ref={ref}
          value={prompt}
          rows={1}
          spellCheck={false}
          placeholder={
            variant === "home"
              ? "Ask Aster anything (Enter), or set a focus and hit Review"
              : "Ask a follow-up (Enter), or run another review"
          }
          aria-label="Message Aster"
          onChange={(e) => setPrompt(e.target.value)}
          onKeyDown={onKeyDown}
        />

        {variant === "home" ? (
          <div className="composer-row">
            {plusMenu}
            {modelPill}
            <span className="spacer" />
            <button className="btn btn-ghost" disabled={!canAsk} onClick={onAsk}>
              Ask
            </button>
            <button
              className="btn btn-primary"
              disabled={!canReview || busy}
              onClick={onReview}
            >
              {reviewing ? "Reviewing" : "Review"}
            </button>
          </div>
        ) : (
          <div className="composer-row">
            {plusMenu}
            {modelPill}
            <span className="spacer" />
            <button
              className="btn btn-ghost"
              disabled={!canReview || busy}
              onClick={onReview}
            >
              {reviewing ? "Reviewing" : "Review"}
            </button>
            <button
              className="send"
              aria-label="Send message"
              disabled={!canAsk}
              onClick={onAsk}
            >
              <SendIcon />
            </button>
          </div>
        )}
      </div>

      {variant === "foot" && (
        <div className="under-row">
          <span className="u-item">
            <RepoIcon />
            {repoName || "no repo"}
          </span>
          <span className="u-item mono">{SOURCE_LABELS[opts.sourceKind]}</span>
          <span className="grow" />
          <span className="u-item mono">
            confidence ≥ {opts.minConfidence.toFixed(2)}
          </span>
        </div>
      )}
    </div>
  );
}

/** Model picker: a name-only list of vetted models, a checkmark on the current
    one, and a "+" row to type any custom model id. */
function ModelMenu({
  model,
  models,
  onModel,
  onAddModel,
  direction = "up",
}: {
  model: string;
  models: string[];
  onModel: (value: string) => void;
  onAddModel: (value: string) => void;
  direction?: "up" | "down";
}) {
  const [open, setOpen] = useState(false);
  const [adding, setAdding] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (!wrapRef.current?.contains(e.target as Node)) {
        setOpen(false);
        setAdding(false);
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setOpen(false);
        setAdding(false);
      }
    };
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  useEffect(() => {
    if (adding) inputRef.current?.focus();
  }, [adding]);

  const commitAdd = () => {
    const value = inputRef.current?.value.trim();
    if (value) onAddModel(value);
    setAdding(false);
    setOpen(false);
  };

  return (
    <div className="dd-wrap" ref={wrapRef}>
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="pill model-pill">
          <span>{modelShort(model)}</span>
          <span className="chev">▾</span>
        </span>
      </button>
      {open && (
        <div className={`dd ${direction}`} role="menu">
          {models.map((m) => (
            <button
              key={m}
              type="button"
              role="menuitemradio"
              aria-checked={m === model}
              data-active={m === model}
              title={m}
              onClick={() => {
                onModel(m);
                setOpen(false);
              }}
            >
              <span>{modelShort(m)}</span>
            </button>
          ))}
          {adding ? (
            <input
              ref={inputRef}
              className="dd-input"
              spellCheck={false}
              placeholder="provider/model-name"
              onKeyDown={(e) => {
                if (e.key === "Enter") commitAdd();
              }}
              onBlur={commitAdd}
            />
          ) : (
            <button
              type="button"
              className="dd-add"
              onClick={(e) => {
                e.stopPropagation();
                setAdding(true);
              }}
            >
              <span>+ Add model</span>
            </button>
          )}
        </div>
      )}
    </div>
  );
}

/** The "+" popover: attach a diff, pick the repository, pick the source.
    Collapses what used to be three separate pills into one menu. */
function PlusMenu({
  opts,
  repoOptions,
  onRepo,
  onSource,
  onAttach,
  direction = "up",
}: {
  opts: ReviewOpts;
  repoOptions: { value: string; label: string }[];
  onRepo: (value: string) => void;
  onSource: (kind: SourceKind) => void;
  onAttach: () => void;
  direction?: "up" | "down";
}) {
  const [open, setOpen] = useState(false);
  const [view, setView] = useState<"root" | "repo" | "source">("root");
  const [query, setQuery] = useState("");
  const wrapRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  const close = () => {
    setOpen(false);
    setView("root");
    setQuery("");
  };

  useEffect(() => {
    if (!open) return;
    searchRef.current?.focus();
    const onDoc = (e: MouseEvent) => {
      if (!wrapRef.current?.contains(e.target as Node)) close();
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const pick = (fn: () => void) => () => {
    fn();
    close();
  };

  const repoLabel =
    repoOptions.find((r) => r.value === opts.repoPath)?.label || "none";

  const q = query.trim().toLowerCase();
  const hits = q
    ? [
        { label: "Attach a diff file…", act: onAttach },
        ...repoOptions.map((r) => ({
          label: r.label,
          act: () => onRepo(r.value),
        })),
        ...SOURCE_OPTS.map((o) => ({
          label: o.label,
          act: () => onSource(o.value),
        })),
      ].filter((h) => h.label.toLowerCase().includes(q))
    : [];

  return (
    <div className="dd-wrap" ref={wrapRef}>
      <button
        type="button"
        className="ghost-icon"
        style={{ width: 28, height: 28 }}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label="Review setup"
        onClick={() => (open ? close() : setOpen(true))}
      >
        <AttachIcon />
      </button>
      {open && (
        <div className={`dd ${direction}`} role="menu" style={{ minWidth: 230 }}>
          <input
            ref={searchRef}
            className="dd-search"
            spellCheck={false}
            placeholder="Search…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && hits.length) {
                hits[0].act();
                close();
              }
            }}
          />

          {q ? (
            hits.length ? (
              hits.map((h) => (
                <button key={h.label} type="button" onClick={pick(h.act)}>
                  <span>{h.label}</span>
                </button>
              ))
            ) : (
              <div className="dd-empty">No matches</div>
            )
          ) : view === "root" ? (
            <>
              <button type="button" onClick={pick(onAttach)}>
                <span>Attach a diff file…</span>
              </button>
              <button type="button" onClick={() => setView("repo")}>
                <span>Repository</span>
                <small>{repoLabel} ›</small>
              </button>
              <button type="button" onClick={() => setView("source")}>
                <span>Source</span>
                <small>{SOURCE_LABELS[opts.sourceKind]} ›</small>
              </button>
            </>
          ) : view === "repo" ? (
            <>
              <button type="button" className="dd-back" onClick={() => setView("root")}>
                <span>‹ Back</span>
              </button>
              {repoOptions.map((r) => (
                <button
                  key={r.value}
                  type="button"
                  data-active={r.value === opts.repoPath}
                  title={r.value}
                  onClick={pick(() => onRepo(r.value))}
                >
                  <span>{r.label}</span>
                </button>
              ))}
              <button type="button" onClick={pick(() => onRepo("__browse__"))}>
                <span>Browse…</span>
              </button>
            </>
          ) : (
            <>
              <button type="button" className="dd-back" onClick={() => setView("root")}>
                <span>‹ Back</span>
              </button>
              {SOURCE_OPTS.map((o) => (
                <button
                  key={o.value}
                  type="button"
                  data-active={o.value === opts.sourceKind}
                  onClick={pick(() => onSource(o.value))}
                >
                  <span>{o.label}</span>
                </button>
              ))}
            </>
          )}
        </div>
      )}
    </div>
  );
}
