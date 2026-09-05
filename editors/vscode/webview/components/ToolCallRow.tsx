import { useState, type ReactElement } from "react";
import { languageFromPath } from "../lib/highlight";
import { inEditor, post } from "../lib/host";
import { openFilePreview } from "../lib/filePreview";
import type { ToolCall } from "../lib/thread";
import {
  arg,
  describeTool,
  displayOutput,
  mcpMatches,
  mcpTarget,
  numberArg,
  outputTitle,
  rendersAsMarkdown,
  resultHint,
  toolInput,
  toolPath,
} from "../lib/tools";
import { Disclosure } from "../interior/disclosure";
import { Code } from "./Code";
import { CopyButton } from "./CopyButton";
import { Markdown } from "./Markdown";
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
  const prose = rendersAsMarkdown(call);
  const card = Boolean(output || input);
  const open = card && (expanded || call.error === true);

  const lead = nested ? undefined : verb;
  const body = nested && !detail ? verb : detail;

  const openLabel = path
    ? `Open ${path}`
    : inEditor
      ? "Open output in an editor tab"
      : "Open the output over the thread";
  const openInEditor = () => {
    if (window.getSelection()?.isCollapsed === false) return;
    if (path) {
      // An edit lands on the line its search text matched; a read on the
      // first line of the range it asked for.
      if (inEditor && call.name === "edit_file") {
        post({ type: "openFile", path, needle: arg(call, "search") });
      } else {
        openFilePreview(path, numberArg(call, "start_line"));
      }
    } else if (output) {
      post({ type: "openUntitled", content: output, title: outputTitle(call), doc: prose });
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
                ) : prose ? (
                  <Markdown text={output} />
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
