import { useEffect, useRef, useState } from "react";
import { Button, Input } from "@heroui/react";
import type { ReviewOpts, SourceKind } from "../lib/types";
import { SOURCE_LABELS } from "../lib/session";
import { AttachIcon } from "./icons";

const SOURCE_OPTS = (Object.keys(SOURCE_LABELS) as SourceKind[]).map((k) => ({
  value: k,
  label: SOURCE_LABELS[k],
}));

/** The "+" popover: attach a diff, pick the repository, pick the source.
    Collapses what used to be three separate pills into one menu. */
export function PlusMenu({
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
      <Button
        className="ghost-icon"
        style={{ width: 28, height: 28 }}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label="Review setup"
        onPress={() => (open ? close() : setOpen(true))}
      >
        <AttachIcon />
      </Button>
      {open && (
        <div className={`dd ${direction}`} role="menu" style={{ minWidth: 230 }}>
          <Input
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
                <Button key={h.label} onPress={pick(h.act)}>
                  <span>{h.label}</span>
                </Button>
              ))
            ) : (
              <div className="dd-empty">No matches</div>
            )
          ) : view === "root" ? (
            <>
              <Button onPress={pick(onAttach)}>
                <span>Attach a diff file…</span>
              </Button>
              <Button onPress={() => setView("repo")}>
                <span>Repository</span>
                <small>{repoLabel} ›</small>
              </Button>
              <Button onPress={() => setView("source")}>
                <span>Source</span>
                <small>{SOURCE_LABELS[opts.sourceKind]} ›</small>
              </Button>
            </>
          ) : view === "repo" ? (
            <>
              <Button className="dd-back" onPress={() => setView("root")}>
                <span>‹ Back</span>
              </Button>
              {repoOptions.map((r) => (
                <Button
                  key={r.value}
                  data-active={r.value === opts.repoPath}
                  onPress={pick(() => onRepo(r.value))}
                >
                  <span title={r.value}>{r.label}</span>
                </Button>
              ))}
              <Button onPress={pick(() => onRepo("__browse__"))}>
                <span>Browse…</span>
              </Button>
            </>
          ) : (
            <>
              <Button className="dd-back" onPress={() => setView("root")}>
                <span>‹ Back</span>
              </Button>
              {SOURCE_OPTS.map((o) => (
                <Button
                  key={o.value}
                  data-active={o.value === opts.sourceKind}
                  onPress={pick(() => onSource(o.value))}
                >
                  <span>{o.label}</span>
                </Button>
              ))}
            </>
          )}
        </div>
      )}
    </div>
  );
}
