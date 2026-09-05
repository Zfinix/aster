import { useState, type ReactElement, type ReactNode } from "react";
import type { ToolStep } from "../lib/session";
import { ToolOutput } from "./ToolOutput";
import {
  AgentIcon,
  BookIcon,
  BrainIcon,
  ChevronIcon,
  CompassIcon,
  FileIcon,
  FileSearchIcon,
  FlaskIcon,
  FolderIcon,
  HistoryIcon,
  PencilIcon,
  SearchIcon,
  TerminalIcon,
} from "./icons";

const ICONS: Record<string, ReactElement> = {
  read_file: <FileIcon />,
  list_files: <FolderIcon />,
  find_files: <FileSearchIcon />,
  search_files: <SearchIcon />,
  edit_file: <PencilIcon />,
  run_command: <TerminalIcon />,
  run_tests: <FlaskIcon />,
  explore: <CompassIcon />,
  remember: <BrainIcon />,
  forget: <BrainIcon />,
  recall: <HistoryIcon />,
  read_skill: <BookIcon />,
  agent: <AgentIcon />,
};

const OPEN_BY_DEFAULT = new Set(["run_command", "run_tests"]);

function hintFor(output: string): string {
  const n = output.split("\n").filter((l) => l.length).length;
  return `${n} line${n === 1 ? "" : "s"}`;
}

/** One step, collapsed to its header until asked. The label's first word is
 *  the verb and carries the row; the rest is what it acted on. */
export function ToolRow({
  step,
  running,
  icon,
  verb,
  detail,
  hint,
  card,
}: {
  step: ToolStep;
  running: boolean;
  icon?: ReactElement;
  verb?: string;
  detail?: string;
  hint?: string;
  card?: ReactNode;
}) {
  const [expanded, setExpanded] = useState(OPEN_BY_DEFAULT.has(step.name));
  const output = step.output?.trim() ?? "";
  const body = card ?? (output ? <ToolOutput name={step.name} output={output} /> : null);
  const hasCard = body != null;

  const space = step.label.indexOf(" ");
  const lead = verb ?? (space === -1 ? step.label : step.label.slice(0, space));
  const rest = detail ?? (space === -1 ? "" : step.label.slice(space + 1));

  return (
    <div className="tool" data-running={running} data-error={false}>
      <button
        type="button"
        className="tool-row"
        onClick={() => setExpanded((o) => !o)}
        disabled={!hasCard}
        aria-expanded={hasCard ? expanded : undefined}
      >
        <span className="tool-icon">{icon ?? ICONS[step.name] ?? <FileIcon />}</span>
        <span className="tool-label">
          <span className="tool-verb">{lead}</span>
          {rest && <span className="tool-detail"> {rest}</span>}
        </span>
        <span className="tool-hint">{running ? "running…" : (hint ?? (output ? hintFor(output) : ""))}</span>
        <span className="tool-chevron">{hasCard && <ChevronIcon open={expanded} />}</span>
      </button>
      {hasCard && expanded && (
        <div className="tool-card">
          <div className="tool-cell">
            <span className="tool-cell-label">out</span>
            <div className="tool-cell-body">{body}</div>
          </div>
        </div>
      )}
    </div>
  );
}
