import { Finding, StreamEvent, UsageSummary } from "./types";

/** Mirrors `aster_policy::Mode`; see crates/aster-policy/src/decision.rs.
 * Uses the canonical CLI names (`plan`, `manual`, `auto`, `edit`, `yolo`); the
 * old `ask`/`deny` aliases are migrated in `panel.ts`. */
export type PermissionMode = "plan" | "manual" | "auto" | "edit" | "yolo";

/** Mirrors `aster_ai::Effort`; see crates/aster-ai/src/effort.rs. */
export type Effort = "off" | "low" | "medium" | "high";

export interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

export type ReviewSource =
  | { kind: "working" }
  | { kind: "range"; value: string }
  | { kind: "pr"; value: string };

/** One line of `aster chat --stream` output. */
export type ChatStreamEvent =
  /** One streamed content delta, appended as it arrives. */
  | { type: "token"; content: string }
  /** A whole content block, sent only when the endpoint streamed no deltas. */
  | { type: "text"; content: string }
  | { type: "tool_call"; id: string; name: string; arguments: string }
  | { type: "tool_result"; id: string; name: string; result: string; error: boolean }
  /** Live progress for one sub-agent in an `agent` tool call. */
  | {
      type: "agent_status";
      call_id: string;
      agent: string;
      task?: string;
      status: "running" | "done" | "error";
      report?: string;
      error?: string;
      done: number;
      total: number;
    }
  /** `plan` means approving promotes the mode, so the next turn must not
   *  relaunch in `plan`; `action` is a one-off edit or command. */
  | {
      type: "approval_request";
      kind?: "plan" | "action";
      preview: string;
      scope?: string | null;
    }
  /** `ask_user`: the turn blocks until a `{"choice"}` line goes back. */
  | { type: "question"; header: string; question: string; options: string[] }
  | { type: "done"; reply: string; edits: string[]; usage?: UsageSummary }
  /** Something the harness did to the turn, e.g. stopping at the round cap. */
  | { type: "notice"; message: string }
  /** A queued user message the running turn absorbed at a round boundary. */
  | { type: "injected"; content: string }
  | { type: "error"; message: string };

export interface SessionSummary {
  id: string;
  created_at: string;
  model: string | null;
  turns: number;
  title: string;
}

/** One replayed message from a saved transcript. */
export interface TranscriptTurn {
  role: "user" | "assistant";
  content: string;
}

/** One skill the session can see. The panel's own actions are not sent over the
 *  wire: which picker a menu row opens is the webview's business. */
export interface SkillCommand {
  name: string;
  detail: string;
  /** Set when a plugin contributed it rather than a skills root. */
  plugin?: string;
}

/** One configured MCP server, from `aster mcp list --no-connect --json`. */
export interface McpServer {
  name: string;
  disabled: boolean;
  transport: string | null;
  command: string;
  args: string[];
  url: string;
}

/** One endpoint from `aster models --providers --json`. */
export interface Provider {
  name: string;
  base_url: string;
  example_model: string;
  current: boolean;
  /** Env vars that may hold this endpoint's own key, before the shared one. */
  key_env: string[];
}

/** A label/value pair in an info card, e.g. one `/status` row. */
export interface InfoRow {
  label: string;
  value: string;
}

/** Messages the webview sends to the extension host. */
export type ToHost =
  | { type: "ready" }
  | {
      type: "chat";
      id: string;
      messages: ChatMessage[];
      model: string | null;
      permissionMode: PermissionMode;
      /** `null` leaves the level to ASTER_EFFORT and `aster.yaml`. */
      effort: Effort | null;
      session: string | null;
    }
  | { type: "approval"; allow: boolean; always?: boolean }
  /** Answer to an open `question`; `null` declines to choose. */
  | { type: "answer"; choice: string | null }
  /** A message typed while a turn runs, offered to the CLI mid-turn. */
  | { type: "inject"; text: string }
  | { type: "cancelChat" }
  | { type: "review"; id: string; source: ReviewSource }
  | { type: "cancelReview" }
  | { type: "openFinding"; finding: Finding }
  | { type: "openFile"; path: string }
  /** Open a code block's contents as a scratch editor tab. */
  /** `title` names the scratch tab; keep it short, it is the whole label. */
  | { type: "openUntitled"; content: string; lang?: string; title?: string }
  | { type: "setPermissionMode"; mode: PermissionMode }
  | { type: "setModel"; model: string }
  | { type: "setEffort"; effort: Effort | null }
  /** Fuzzy workspace file search backing @ mentions. */
  | { type: "searchFiles"; query: string; requestId: string }
  /** Run one of the extension's own commands, e.g. from a slash command. */
  | { type: "runCommand"; command: string }
  | { type: "fixFinding"; finding: Finding }
  | { type: "fixAllFindings"; findings: Finding[] }
  | { type: "listSessions" }
  | { type: "loadSession"; id: string }
  | { type: "fetchModels" }
  /** `/status`, `/memory`, `/diff`: each answers with one `info` card. */
  | { type: "info"; id: string; topic: "status" | "memory" | "diff" }
  /** Files dropped on the composer, as `file://` URIs; answered with a mention. */
  | { type: "dropFiles"; uris: string[] }
  | { type: "listMcp" }
  | { type: "toggleMcp"; name: string; disabled: boolean }
  | { type: "listProviders" }
  /** Repoint the endpoint and adopt one of its models, as the TUI's `/provider` does. */
  | { type: "setProvider"; baseUrl: string; model: string }
  | { type: "compact"; id: string; messages: ChatMessage[] };

/** Messages the extension host sends to the webview. */
export type ToWebview =
  | {
      type: "init";
      workspaceRoot: string | null;
      repoName: string | null;
      branch: string | null;
      model: string | null;
      /** Everything the picker can offer before the endpoint is asked. */
      models: string[];
      /** The vetted subset, shown first; see `MODELS` in panel.ts. */
      recommended: string[];
      permissionMode: PermissionMode;
      effort: Effort | null;
      binaryOk: boolean;
      skills: SkillCommand[];
    }
  | { type: "chatEvent"; id: string; event: ChatStreamEvent }
  | { type: "chatError"; id: string; message: string }
  /** Authoritative run state, broadcast to every surface. */
  | { type: "runState"; review: boolean; chat: boolean }
  | { type: "sessions"; sessions: SessionSummary[] }
  | { type: "sessionLoaded"; id: string; turns: TranscriptTurn[] }
  /** Start a fresh conversation, from the command palette or a keybinding. */
  | { type: "newConversation" }
  /** Drop an editor selection into the composer as a mention. */
  | { type: "insertMention"; text: string }
  /** Open the command menu, from the palette or a keybinding. */
  | { type: "openCommandMenu" }
  | { type: "fileResults"; requestId: string; paths: string[] }
  | { type: "reviewStarted"; id: string; source: ReviewSource }
  | { type: "reviewEvent"; id: string; event: StreamEvent }
  | { type: "reviewDone"; id: string }
  | { type: "reviewError"; id: string; message: string }
  | { type: "fixResult"; finding: Finding; status: string; reason?: string; patch?: string }
  | { type: "fixAllResult"; results: { finding: Finding; status: string; reason?: string }[] }
  | { type: "log"; line: string }
  /** The endpoint's catalog, or why it could not be read. */
  | { type: "modelsLoaded"; models: string[]; error?: string }
  /** The answer to an `info` request: rows, a body, or the reason there is neither. */
  | {
      type: "infoCard";
      id: string;
      title: string;
      rows?: InfoRow[];
      body?: string;
      lang?: string;
      note?: string;
      error?: boolean;
    }
  | { type: "mcpServers"; servers: McpServer[] }
  | { type: "providers"; providers: Provider[] }
  /** The provider switch landed; `models` is the new endpoint's catalog. */
  | { type: "providerChanged"; provider: string; model: string; models: string[] }
  | {
      type: "compacted";
      id: string;
      summary: string;
      folded: number;
      messages: TranscriptTurn[];
    };
