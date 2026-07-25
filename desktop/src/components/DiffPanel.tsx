import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { DiffFile, Finding } from "../lib/types";
import { severityOf, SEV_LABEL } from "../lib/severity";
import { SEV_CLS, SEV_ICON } from "../lib/review-format";
import { fileKey } from "../lib/diff";
import { matchFile, findingKey } from "../lib/match";
import { highlightCached } from "../lib/highlight";
import { useToast } from "./chrome";
import { Mark } from "./Mark";
import { ExpandIcon } from "./icons";

const SIGN: Record<string, string> = { add: "+", del: "-", ctx: "", hunk: "" };

const PANEL_MIN = 420;
const PANEL_STORE = "rpanel-width";

function clampWidth(px: number) {
  const max = Math.max(PANEL_MIN, window.innerWidth - 360);
  return Math.min(max, Math.max(PANEL_MIN, px));
}

function usePanelWidth() {
  const [width, setWidth] = useState<number>(() => {
    const saved = Number(localStorage.getItem(PANEL_STORE));
    return saved ? clampWidth(saved) : Math.round(window.innerWidth * 0.46);
  });
  const [resizing, setResizing] = useState(false);
  const frame = useRef(0);

  const onResizeStart = useCallback((e: React.PointerEvent) => {
    e.preventDefault();
    setResizing(true);
    const move = (ev: PointerEvent) => {
      cancelAnimationFrame(frame.current);
      frame.current = requestAnimationFrame(() =>
        setWidth(clampWidth(window.innerWidth - ev.clientX)),
      );
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

function anchorFindings(file: DiffFile, findings: Finding[]) {
  const byRow = new Map<number, Finding[]>();
  const orphans: Finding[] = [];
  for (const f of findings) {
    const wantOld = (f.side ?? "right").toLowerCase() === "left";
    let idx = file.lines.findIndex((l) =>
      wantOld ? l.oldNo === f.line : l.newNo === f.line,
    );
    if (idx < 0)
      idx = file.lines.findIndex(
        (l) => l.newNo === f.line || l.oldNo === f.line,
      );
    if (idx < 0) orphans.push(f);
    else byRow.set(idx, [...(byRow.get(idx) ?? []), f]);
  }
  return { byRow, orphans };
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
  files: DiffFile[];
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

  // Collapse state for every file, lifted so the header can toggle them all.
  // null = the default layout: first file open, the rest collapsed.
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
      const el = bodyRef.current?.querySelector<HTMLElement>(
        `[data-fkey="${CSS.escape(focus.key)}"]`,
      );
      if (!el) return;
      el.scrollIntoView({ behavior: "smooth", block: "center" });
      el.classList.remove("flash");
      void el.offsetWidth;
      el.classList.add("flash");
    });
    return () => cancelAnimationFrame(raf);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus?.nonce]);

  return (
    <aside className={`rpanel ${resizing ? "resizing" : ""}`} style={{ width }}>
      <div
        className="rpanel-resizer"
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize diff panel"
        onPointerDown={onResizeStart}
      />
      <header className="r-head" data-tauri-drag-region>
        <button
          className={`r-title ${allCollapsed ? "collapsed" : ""}`}
          aria-expanded={!allCollapsed}
          title={allCollapsed ? "Expand all files" : "Collapse all files"}
          onClick={toggleAll}
        >
          <span className="f-chev">▾</span>
          All changes
          <span
            className="mono"
            style={{ fontSize: 11.5, color: "var(--faint)" }}
          >
            <span className="plus">+{adds}</span>{" "}
            <span className="minus">-{dels}</span>
          </span>
        </button>
        <span className="grow" data-tauri-drag-region />
        <button
          className="btn btn-line"
          style={{ fontSize: 12, padding: "5px 13px" }}
          onClick={onReverify}
        >
          Re-verify
        </button>
        <button
          className="ghost-icon"
          aria-label="Close diff panel"
          onClick={onClose}
        >
          <ExpandIcon />
        </button>
      </header>
      <div className="r-body" ref={bodyRef}>
        {files.map((file, i) => (
          <FileBlock
            key={keys[i]}
            file={file}
            findings={findings.filter((x) => matchFile(x, fileKey(file)))}
            collapsed={closedSet.has(keys[i])}
            onToggle={() => toggleFile(keys[i])}
            onApplyFix={onApplyFix}
          />
        ))}
        {findings.length > 0 && (
          <div className="r-brief">
            <span className="r-brief-label">
              {findings.length} finding{findings.length > 1 ? "s" : ""} ready to
              hand off
            </span>
            <button className="btn btn-primary" onClick={onFixBrief}>
              Copy fix brief
            </button>
          </div>
        )}
      </div>
    </aside>
  );
}

function FileBlock({
  file,
  findings,
  collapsed,
  onToggle,
  onApplyFix,
}: {
  file: DiffFile;
  findings: Finding[];
  collapsed: boolean;
  onToggle: () => void;
  onApplyFix: (finding: Finding) => Promise<boolean>;
}) {
  const { byRow, orphans } = useMemo(
    () => anchorFindings(file, findings),
    [file, findings],
  );

  return (
    <div className={`file ${collapsed ? "collapsed" : ""}`}>
      <button
        className="file-head"
        aria-expanded={!collapsed}
        onClick={onToggle}
      >
        <span className="f-chev">▾</span>
        <span className="fname">{file.newPath || file.oldPath}</span>
        {file.status === "added" && <span className="ftag">new file</span>}
        <span className="fstats">
          <span className="plus">+{file.additions}</span>
          <span className="minus">-{file.deletions}</span>
        </span>
      </button>
      <div className="file-body">
        <table className="difftable">
          <tbody>
            {file.lines.map((line, i) => {
              const rowFindings = byRow.get(i);
              if (line.kind === "hunk") {
                return (
                  <tr key={i} className="hunk">
                    <td colSpan={4}>{line.text || "@@"}</td>
                  </tr>
                );
              }
              return (
                <RowGroup
                  key={i}
                  line={line}
                  findings={rowFindings}
                  onApplyFix={onApplyFix}
                />
              );
            })}
            {orphans.map((f, i) => (
              <tr key={`orphan-${i}`} className="comment">
                <td colSpan={4}>
                  <InlineComment finding={f} onApplyFix={onApplyFix} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function RowGroup({
  line,
  findings,
  onApplyFix,
}: {
  line: import("../lib/types").DiffLine;
  findings?: Finding[];
  onApplyFix: (finding: Finding) => Promise<boolean>;
}) {
  return (
    <>
      <tr className={`${line.kind}${findings?.length ? " flag" : ""}`}>
        <td className="ln">{line.oldNo ?? ""}</td>
        <td className="ln">{line.newNo ?? ""}</td>
        <td className="sign">{SIGN[line.kind]}</td>
        <td
          className="code"
          dangerouslySetInnerHTML={{ __html: highlightCached(line.text || " ") }}
        />
      </tr>
      {findings?.map((f, i) => (
        <tr key={i} className="comment">
          <td colSpan={4}>
            <InlineComment finding={f} onApplyFix={onApplyFix} />
          </td>
        </tr>
      ))}
    </>
  );
}

function InlineComment({
  finding,
  onApplyFix,
}: {
  finding: Finding;
  onApplyFix: (finding: Finding) => Promise<boolean>;
}) {
  const toast = useToast();
  const [resolved, setResolved] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  const [fixing, setFixing] = useState(false);
  const sev = severityOf(finding.severity);
  const conf =
    finding.confidence != null ? finding.confidence.toFixed(2) : null;

  const copySuggestion = async () => {
    const text = finding.suggestion || finding.description;
    await navigator.clipboard.writeText(text);
    toast("Suggestion copied to clipboard");
  };

  const runFix = async () => {
    setFixing(true);
    try {
      if (await onApplyFix(finding)) setResolved(true);
    } catch (e) {
      toast(String(e));
    } finally {
      setFixing(false);
    }
  };

  if (resolved) {
    return (
      <div className="ic-resolved">
        <span className="rt">{finding.title}</span>
        <button className="btn btn-ghost" onClick={() => setResolved(false)}>
          Reopen
        </button>
      </div>
    );
  }

  return (
    <div
      className={`inline-comment ${collapsed ? "collapsed" : ""}`}
      data-fkey={findingKey(finding)}
    >
      <button
        className="ic-head"
        aria-expanded={!collapsed}
        title={collapsed ? "Expand finding" : "Collapse finding"}
        onClick={() => setCollapsed((c) => !c)}
      >
        <Mark px={1.3} />
        <span className="who">Aster</span>
        <span className={`sev ${SEV_CLS[sev]}`}>
          {(() => {
            const Icon = SEV_ICON[sev];
            return <Icon />;
          })()}
          {SEV_LABEL[sev]}
        </span>
        {collapsed && <span className="ic-head-title">{finding.title}</span>}
        <span className="ic-spacer" />
        {conf && <span className="conf">{conf}</span>}
        <span className="ic-chev">▾</span>
      </button>
      {!collapsed && (
        <div className="ic-body">
          <div className="ic-title">{finding.title}</div>
          <p className="ic-text">{finding.description}</p>
          {finding.suggestion && (
            <div className="ic-sugg">{finding.suggestion}</div>
          )}
          <div className="ic-actions">
            <button
              className="btn btn-success"
              disabled={fixing}
              onClick={runFix}
            >
              {fixing ? "Fixing…" : "Apply fix"}
            </button>
            <button className="btn btn-line" onClick={copySuggestion}>
              Copy fix
            </button>
            <button className="btn btn-line" onClick={() => setResolved(true)}>
              Resolve
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
