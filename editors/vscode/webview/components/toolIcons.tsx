import type { ReactElement } from "react";
import {
  AgentIcon,
  BookIcon,
  BrainIcon,
  CheckAllIcon,
  CloudIcon,
  CompassIcon,
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

/** The face a tool wears wherever it is listed: the step row, and the tally a
 *  sub-agent card shows for what it did. */
export function toolIcon(name: string, target?: string): ReactElement {
  if (target) {
    const [server, action = ""] = target.split("/");
    if (MCP_ICONS[action]) return MCP_ICONS[action];
    if (/^web/.test(server)) return <GlobeIcon />;
  }
  return ICONS[name] ?? <FileIcon />;
}

/** Whether a tool changes the workspace, which the tally colours differently:
 *  a batch that only read is not the same as one that wrote. */
export function writesFiles(name: string): boolean {
  return name === "edit_file";
}
