import { useState, type ReactElement } from "react";
import { languageFromPath } from "../lib/highlight";
import { inEditor, post } from "../lib/host";
import type { ToolCall } from "../lib/thread";
import {
  describeTool,
  displayOutput,
  mcpMatches,
  mcpTarget,
  outputTitle,
  resultHint,
  toolInput,
  toolPath,
} from "../lib/tools";
import { Disclosure } from "../interior/disclosure";
import { Code } from "./Code";
import { CopyButton } from "./CopyButton";
import { McpMatches } from "./McpMatches";
import { ToolOutput } from "./ToolOutput";
import {
  AgentIcon,
  AlertIcon,
  BookIcon,
  BrainIcon,
  CheckAllIcon,
  ChevronIcon,
  CloudIcon,
  CompassIcon,
  ExternalIcon,
  FileIcon,
  FileSearchIcon,
  FlaskIcon,
  FolderIcon,
  GlobeIcon,
  ImageIcon,
  HistoryIcon,
  ListOrderedIcon,
  NetworkIcon,
  PencilIcon,
  PlugIcon,
  QuestionIcon,
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
  recall: <HistoryIcon />,
  read_skill: <BookIcon />,
  update_plan: <ListOrderedIcon />,
  ask_user: <QuestionIcon />,
  exit_plan_mode: <CheckAllIcon />,
  agent: <AgentIcon />,
  aster_mcp: <PlugIcon />,
};

/** A web call is a web call whichever server served it, so it wears the globe
 *  rather than the generic MCP plug. */
const MCP_ICONS: Record<string, ReactElement> = {
  screenshot: <ImageIcon />,
  sitemap: <NetworkIcon />,
  fetch_content: <CloudIcon />,
  extract: <CloudIcon />,
};

function toolIcon(name: string, target: string | undefined): ReactElement {
  if (target) {
    const [server, action = ""] = target.split("/");
    if (MCP_ICONS[action]) return MCP_ICONS[action];
    if (/^web/.test(server)) return <GlobeIcon />;
  }
  return ICONS[name] ?? <FileIcon />;
}

/** Commands show their output without being asked: what ran and what came back
 *  is the transcript's story. */
const OPEN_BY_DEFAULT = new Set(["run_command", "run_tests"]);

/** One step, collapsed to its header until asked: eighteen reads stay a list
 *  rather than a wall. Failures open themselves, since that is the one case the
 *  reader was always going to expand. `nested` is a step inside a folded run. */
export function ToolCallRow({ call, nested }: { call: ToolCall; nested?: boolean }) {
  const running = call.result === undefined && !call.stopped;
  const output = displayOutput(call);
  const input = toolInput(call);
  const [expanded, setExpanded] = useState(OPEN_BY_DEFAULT.has(call.name));
  const { verb, detail, code } = describeTool(call);
  const matches = mcpMatches(call);
  const path = toolPath(call);
  const card = Boolean(output || input);
  const open = card && (expanded || call.error === true);

  /** Every step in a run shares one icon and one verb, and the run's header
   *  already wears both. What is left is the argument that tells them apart,
   *  which takes the row's weight now that nothing leads it. */
  const lead = nested ? undefined : verb;
  const body = nested && !detail ? verb : detail;

  /** A path opens the real file; anything else opens its output as a scratch
   *  tab, which is where find, folding, and highlighting live. A page has no
   *  tab for that, so there the output opens over the thread. */
  const openLabel = path
    ? `Open ${path}`
    : inEditor
      ? "Open output in an editor tab"
      : "Open the output over the thread";
  const openInEditor = () => {
    if (window.getSelection()?.isCollapsed === false) return;
    if (path) {
      post({ type: "openFile", path });
    } else if (output) {
      post({ type: "openUntitled", content: output, title: outputTitle(call) });
    }
  };

  return (
    <div className="tool" data-error={call.error === true} data-running={running}>
      <button
        className="tool-row"
        onClick={() => setExpanded(!expanded)}
        disabled={!card}
        aria-expanded={card ? open : undefined}
        title={card ? (open ? "Hide details" : "Show details") : undefined}
      >
        {!nested && (
          <span className="tool-icon">
            {call.error ? <AlertIcon /> : toolIcon(call.name, mcpTarget(call))}
          </span>
        )}
        <span className="tool-label" data-oneline={Boolean(input)} data-lead={!lead}>
          {lead && <span className="tool-verb">{lead}</span>}
          {/* A real space, not just flex gap: copied text glues the spans. */}
          {body && (
            <span className="tool-detail" data-code={code === true}>
              {lead ? " " : ""}
              {body}
            </span>
          )}
        </span>
        <span className="tool-hint">{running ? "running…" : resultHint(call)}</span>
        <span className="tool-chevron">{card && <ChevronIcon open={open} />}</span>
      </button>

      <Disclosure open={open}>
        <div className="tool-card">
          {input && (
            <div className="tool-cell tool-cell-in">
              <span className="tool-cell-label">in</span>
              <span className="tool-cell-body">
                <pre className="tool-output tool-input">
                  <code>
                    <Code code={input} lang="shellscript" />
                  </code>
                </pre>
              </span>
              <span className="tool-cell-copy">
                <CopyButton text={input} label="Copy command" />
              </span>
            </div>
          )}
          {output && (
            <div className="tool-cell tool-cell-out">
              <span className="tool-cell-label">out</span>
              <span
                className="tool-cell-body"
                role="button"
                tabIndex={0}
                onClick={openInEditor}
                onKeyDown={(e) => {
                  if (e.key !== "Enter" && e.key !== " ") return;
                  e.preventDefault();
                  openInEditor();
                }}
                title={openLabel}
              >
                {matches ? (
                  <McpMatches matches={matches} />
                ) : (
                  <ToolOutput output={output} lang={languageFromPath(path)} />
                )}
              </span>
              <button
                className="icon-btn tool-cell-open"
                onClick={openInEditor}
                title={openLabel}
                aria-label={openLabel}
              >
                <ExternalIcon />
              </button>
            </div>
          )}
        </div>
      </Disclosure>
    </div>
  );
}
