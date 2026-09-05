import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { DiffFile as DiffFileData, Finding } from "../lib/types";
import { fileKey } from "../lib/diff";
import { matchFile, findingKey } from "../lib/match";
import { DiffFile } from "./DiffFile";
import { ChevronIcon, XIcon } from "./icons";

const PANEL_MIN = 420;
const PANEL_STORE = "aster.diffPanelWidth";

function clampWidth(px: number) {
  const max = Math.max(PANEL_MIN, window.innerWidth - 480);
  return Math.min(max, Math.max(PANEL_MIN, px));
}

function usePanelWidth() {
  const [width, setWidth] = useState<number>(() => {
    const saved = Number(localStorage.getItem(PANEL_STORE));
    return saved ? clampWidth(saved) : clampWidth(Math.round(window.innerWidth * 0.42));
  });
  const [resizing, setResizing] = useState(false);
  const frame = useRef(0);

  const onResizeStart = useCallback((e: React.PointerEvent) => {
    e.preventDefault();
    setResizing(true);
    const move = (ev: PointerEvent) => {
      cancelAnimationFrame(frame.current);
      frame.current = requestAnimationFrame(() => setWidth(clampWidth(window.innerWidth - ev.clientX)));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      setResizing(false);
      setWidth((w) => {
        localStorage.setItem(PANEL_STORE, String(w));
        return w;
      });
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }, []);

  useEffect(() => {
    const onResize = () => setWidth((w) => clampWidth(w));
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  return { width, onResizeStart, resizing };
}

export function DiffPanel({
  files,
  findings,
  focus,
  onReverify,
  onApplyFix,
  onFixBrief,
  onClose,
}: {
  files: DiffFileData[];
  findings: Finding[];
  focus: { key: string; nonce: number } | null;
  onReverify: () => void;
  onApplyFix: (finding: Finding) => Promise<boolean>;
  onFixBrief: () => void;
  onClose: () => void;
}) {
  const adds = files.reduce((n, f) => n + f.additions, 0);
  const dels = files.reduce((n, f) => n + f.deletions, 0);
  const { width, onResizeStart, resizing } = usePanelWidth();

  const keys = useMemo(() => files.map(fileKey), [files]);
  const [closed, setClosed] = useState<Set<string> | null>(null);
  const closedSet = closed ?? new Set(keys.slice(1));
  const allCollapsed = keys.length > 0 && keys.every((k) => closedSet.has(k));

  const toggleFile = (key: string) => {
    const next = new Set(closedSet);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    setClosed(next);
  };
  const toggleAll = () => setClosed(new Set(allCollapsed ? [] : keys));

  const bodyRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!focus) return;
    const target = findings.find((f) => findingKey(f) === focus.key);
    if (!target) return;
    const fileIdx = files.findIndex((f) => matchFile(target, fileKey(f)));
    if (fileIdx >= 0) {
      setClosed((prev) => {
        const next = new Set(prev ?? new Set(keys.slice(1)));
        next.delete(keys[fileIdx]);
        return next;
      });
    }
    const raf = requestAnimationFrame(() => {
      const el = bodyRef.current?.querySelector<HTMLElement>(`[data-fkey="${CSS.escape(focus.key)}"]`);
      if (!el) return;
      el.scrollIntoView({ behavior: "smooth", block: "center" });
      el.dataset.flash = "false";
      void el.offsetWidth;
      el.dataset.flash = "true";
    });
    return () => cancelAnimationFrame(raf);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus?.nonce]);

  return (
    <aside className="diff-panel" data-resizing={resizing} style={{ width }}>
      <div
        className="diff-resizer"
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize diff panel"
        onPointerDown={onResizeStart}
      />
      <header className="diff-head" data-tauri-drag-region>
        <button
          type="button"
          className="diff-head-title"
          aria-expanded={!allCollapsed}
          title={allCollapsed ? "Expand all files" : "Collapse all files"}
          onClick={toggleAll}
        >
          <ChevronIcon open={!allCollapsed} />
          Changes
          <span className="diff-stats">
            <span className="diff-add">+{adds}</span>
            <span className="diff-del">−{dels}</span>
          </span>
        </button>
        <span className="grow" data-tauri-drag-region />
        <button type="button" className="btn" onClick={onReverify}>
          Review again
        </button>
        <button type="button" className="ghost icon-action" aria-label="Close diff" title="Close" onClick={onClose}>
          <XIcon />
        </button>
      </header>
      <div className="diff-body" ref={bodyRef}>
        {files.map((file, i) => (
          <DiffFile
            key={keys[i]}
            file={file}
            findings={findings.filter((x) => matchFile(x, fileKey(file)))}
            collapsed={closedSet.has(keys[i])}
            onToggle={() => toggleFile(keys[i])}
            onApplyFix={onApplyFix}
          />
        ))}
        {findings.length > 0 && (
          <div className="diff-brief">
            <span>
              {findings.length} finding{findings.length > 1 ? "s" : ""} ready to hand off
            </span>
            <button type="button" className="btn-primary" onClick={onFixBrief}>
              Copy fix brief
            </button>
          </div>
        )}
      </div>
    </aside>
  );
}
