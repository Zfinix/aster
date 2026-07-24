import { useMemo, useState } from "react";
import type { DiffFile, Finding, Severity } from "../lib/types";
import { severityOf, SEV_LABEL } from "../lib/severity";
import { fileKey } from "../lib/diff";
import { matchFile } from "../lib/match";
import { useToast } from "./chrome";
import { Mark } from "./Mark";
import { ExpandIcon } from "./icons";

const SEV_CLS: Record<Severity, string> = {
  critical: "crit",
  high: "high",
  medium: "med",
  low: "low",
  info: "info",
};

const SIGN: Record<string, string> = { add: "+", del: "-", ctx: "", hunk: "" };

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
  onReverify,
  onApplyFix,
  onClose,
}: {
  files: DiffFile[];
  findings: Finding[];
  onReverify: () => void;
  onApplyFix: (finding: Finding) => Promise<boolean>;
  onClose: () => void;
}) {
  const adds = files.reduce((n, f) => n + f.additions, 0);
  const dels = files.reduce((n, f) => n + f.deletions, 0);

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

  return (
    <aside className="rpanel">
      <header className="r-head">
        <button
          className={`r-title ${allCollapsed ? "collapsed" : ""}`}
          aria-expanded={!allCollapsed}
          title={allCollapsed ? "Expand all files" : "Collapse all files"}
          onClick={toggleAll}
        >
          <span className="f-chev">▾</span>
          All changes
          <span className="mono" style={{ fontSize: 11.5, color: "var(--faint)" }}>
            <span className="plus">+{adds}</span> <span className="minus">-{dels}</span>
          </span>
        </button>
        <span className="grow" />
        <button
          className="btn btn-line"
          style={{ fontSize: 12, padding: "5px 13px" }}
          onClick={onReverify}
        >
          Re-verify
        </button>
        <button className="ghost-icon" aria-label="Close diff panel" onClick={onClose}>
          <ExpandIcon />
        </button>
      </header>
      <div className="r-body">
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
      <button className="file-head" aria-expanded={!collapsed} onClick={onToggle}>
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
                <RowGroup key={i} line={line} findings={rowFindings} onApplyFix={onApplyFix} />
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
        <td className="code">{line.text || " "}</td>
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
    <div className="inline-comment">
      <div className="ic-head">
        <Mark px={1.3} />
        <span className="who">Aster</span>
        <span className={`sev ${SEV_CLS[sev]}`}>{SEV_LABEL[sev]}</span>
        {conf && <span className="conf">{conf}</span>}
      </div>
      <div className="ic-body">
        <div className="ic-title">{finding.title}</div>
        <p className="ic-text">{finding.description}</p>
        {finding.suggestion && (
          <div className="ic-sugg">{finding.suggestion}</div>
        )}
        <div className="ic-actions">
          <button className="btn btn-line" disabled={fixing} onClick={runFix}>
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
    </div>
  );
}
