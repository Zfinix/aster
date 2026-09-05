import { useMemo } from "react";
import type { DiffFile as DiffFileData, Finding } from "../lib/types";
import { highlightCached } from "../lib/highlight";
import { FindingCard } from "./FindingCard";
import { ChevronIcon } from "./icons";

const SIGN: Record<string, string> = { add: "+", del: "-", ctx: "", hunk: "" };

function anchorFindings(file: DiffFileData, findings: Finding[]) {
  const byRow = new Map<number, Finding[]>();
  const orphans: Finding[] = [];
  for (const f of findings) {
    const wantOld = (f.side ?? "right").toLowerCase() === "left";
    let idx = file.lines.findIndex((l) => (wantOld ? l.oldNo === f.line : l.newNo === f.line));
    if (idx < 0) idx = file.lines.findIndex((l) => l.newNo === f.line || l.oldNo === f.line);
    if (idx < 0) orphans.push(f);
    else byRow.set(idx, [...(byRow.get(idx) ?? []), f]);
  }
  return { byRow, orphans };
}

/** One file of the diff: a header that folds it, and the rows with findings
 *  pinned under the lines they point at. */
export function DiffFile({
  file,
  findings,
  collapsed,
  onToggle,
  onApplyFix,
}: {
  file: DiffFileData;
  findings: Finding[];
  collapsed: boolean;
  onToggle: () => void;
  onApplyFix: (finding: Finding) => Promise<boolean>;
}) {
  const { byRow, orphans } = useMemo(() => anchorFindings(file, findings), [file, findings]);

  return (
    <div className="file" data-collapsed={collapsed}>
      <button type="button" className="file-head" aria-expanded={!collapsed} onClick={onToggle}>
        <ChevronIcon open={!collapsed} />
        <span className="file-name">{file.newPath || file.oldPath}</span>
        {file.status === "added" && <span className="file-tag">new</span>}
        {file.status === "deleted" && <span className="file-tag">deleted</span>}
        <span className="diff-stats">
          <span className="diff-add">+{file.additions}</span>
          <span className="diff-del">−{file.deletions}</span>
        </span>
      </button>
      <div className="file-body">
        <table className="difftable">
          <tbody>
            {file.lines.map((line, i) => {
              if (line.kind === "hunk") {
                return (
                  <tr key={i} className="hunk">
                    <td colSpan={4}>{line.text || "@@"}</td>
                  </tr>
                );
              }
              const rowFindings = byRow.get(i);
              return (
                <DiffRows key={i} line={line} findings={rowFindings} onApplyFix={onApplyFix} />
              );
            })}
            {orphans.map((f, i) => (
              <tr key={`orphan-${i}`} className="comment">
                <td colSpan={4}>
                  <FindingCard finding={f} onApplyFix={onApplyFix} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function DiffRows({
  line,
  findings,
  onApplyFix,
}: {
  line: DiffFileData["lines"][number];
  findings?: Finding[];
  onApplyFix: (finding: Finding) => Promise<boolean>;
}) {
  return (
    <>
      <tr className={`${line.kind}${findings?.length ? " flag" : ""}`}>
        <td className="ln">{line.oldNo ?? ""}</td>
        <td className="ln">{line.newNo ?? ""}</td>
        <td className="sign">{SIGN[line.kind]}</td>
        <td className="code" dangerouslySetInnerHTML={{ __html: highlightCached(line.text || " ") }} />
      </tr>
      {findings?.map((f, i) => (
        <tr key={i} className="comment">
          <td colSpan={4}>
            <FindingCard finding={f} onApplyFix={onApplyFix} />
          </td>
        </tr>
      ))}
    </>
  );
}
