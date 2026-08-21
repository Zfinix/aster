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
  /** The round's thinking, absent when the model reasons under encryption. */
  | { type: "reasoning"; content: string; tokens?: number; duration_ms?: number }
  /** One live fragment of streamed thinking, with the running token estimate. */
  | { type: "reasoning_delta"; content: string; tokens: number }
  /** Thinking finished: the final token estimate and how long the round took. */
  | { type: "reasoning_done"; tokens: number; duration_ms: number }
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
      /** Plan approvals carry the plan as clean markdown for document rendering. */
      markdown?: string | null;
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

/** One replayed message from a saved transcript. An assistant turn carries the
 *  round's thinking and tool calls too, so reopening a session rebuilds the same
 *  blocks it showed live instead of a bare wall of text. */
export interface TranscriptTurn {
  role: "user" | "assistant";
  content: string;
  reasoning?: TranscriptReasoning;
  toolCalls?: TranscriptToolCall[];
}

export interface TranscriptReasoning {
  text: string;
  tokens?: number;
  durationMs?: number;
}

/** A recorded call plus the result its matching `tool` event carried. */
export interface TranscriptToolCall {
  id: string;
  name: string;
  arguments: string;
  result?: string;
  error?: boolean;
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

/** A file pasted into the composer: the clipboard carries bytes and a name, never
 *  the path it came from. */
export interface PastedFile {
  name: string;
  /** Base64 contents, absent when the file is too big to carry through. */
  data?: string;
  size: number;
}

/** A label/value pair in an info card, e.g. one `/status` row. */
export interface InfoRow {
  label: string;
  value: string;
}

/** Mirrors `config::Kind`; decides which control a settings row wears. */
export type ConfigKind = "text" | "bool" | "number" | "list" | "choice";

/** Mirrors `config::Unit`, for suffixing a number with what it counts. */
export type ConfigUnit = "none" | "seconds" | "chars" | "bytes" | "tokens" | "percent";

/** Which file a write lands in, spelled as the CLI's own flags. */
export type ConfigScope = "global" | "local";

export type ConfigValue = string | number | boolean | string[] | null;

/** One row of `aster config list --json`. */
export interface ConfigKey {
  key: string;
  label: string;
  group: string;
  kind: ConfigKind;
  /** The options a `choice` key accepts; empty for every other kind. */
  choices: string[];
  unit: ConfigUnit;
  /** What the next turn resolves to, null when nothing sets it. */
  value: ConfigValue;
  /** The resolved value already rendered, units and all. */
  display: string;
  default: string;
  /** `env ASTER_MODEL`, a config file's path, or `default`. */
  source: string;
  /** The variable outranking a file that also sets this key. */
  shadowed: string | null;
  /** What each file sets on its own, so a row can edit one scope at a time. */
  scopes: Record<ConfigScope, ConfigValue>;
  env: string[];
  help: string;
}

/** Which config files this directory reads, from `aster config path --json`. */
export interface ConfigPaths {
  global: string;
  global_exists: boolean;
  project: string | null;
  project_exists: boolean;
  /** Where a workspace write would go when the repo has no config yet. */
  project_default: string;
}

/** The extension's own settings, which live in VS Code rather than aster.yaml. */
export interface EditorSettings {
  binaryPath: string;
  minConfidence: number | null;
  publishDiagnostics: boolean;
  extraArgs: string[];
}

export interface SettingsSnapshot {
  keys: ConfigKey[];
  paths: ConfigPaths | null;
  editor: EditorSettings;
  servers: McpServer[];
  /** The endpoint's catalog, so a model key offers a list instead of a blank
   *  box. Still free text: ids are open-ended and a catalog can fail to load. */
  models: string[];
  providers: Provider[];
  workspaceRoot: string | null;
  binaryOk: boolean;
  /** Why the config could not be read, when that is the whole story. */
  error?: string;
}

/** Messages the settings webview sends to the extension host. */
export type SettingsToHost =
  | { type: "ready" }
  | { type: "setKey"; key: string; value: Exclude<ConfigValue, null>; scope: ConfigScope }
  | { type: "unsetKey"; key: string; scope: ConfigScope }
  | { type: "setEditor"; key: keyof EditorSettings; value: ConfigValue }
  | { type: "toggleMcp"; name: string; disabled: boolean }
  | { type: "openConfigFile"; scope: ConfigScope }
  | { type: "reload" };

/** Messages the extension host sends to the settings webview. */
export type SettingsToWebview =
  | { type: "settings"; snapshot: SettingsSnapshot }
  /** A write failed; `key` anchors the message to the row that caused it. */
  | { type: "settingsError"; key?: string; message: string };

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
  /** Follow a link the agent printed: a preview URL, a doc, a rendered file.
   *  Goes through the host so remote workspaces forward the port. */
  | { type: "openExternal"; url: string }
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
  | { type: "deleteSession"; id: string }
  /** Rename a saved session; the name is appended to its transcript. */
  | { type: "renameSession"; id: string; title: string }
  | { type: "fetchModels" }
  /** `/status`, `/memory`, `/diff`: each answers with one `info` card. */
  | { type: "info"; id: string; topic: "status" | "memory" | "diff" }
  /** Pick files from disk, for the composer's `+`; answered with a mention. */
  | { type: "attachFiles" }
  /** Files dropped on the composer, as `file://` URIs; answered with a mention. */
  | { type: "dropFiles"; uris: string[] }
  /** Files pasted into the composer; also answered with a mention. */
  | { type: "pasteFiles"; files: PastedFile[] }
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
      /** Ids picked before, most recent first. */
      recent: string[];
      /** History size (chars) the CLI auto-compacts above; 0 when unknown. */
      contextBudget: number;
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
  /** A page's answer to `openUntitled`: no editor to open a tab in, so the
   *  snippet comes back and is shown over the thread. */
  | { type: "scratch"; content: string; lang?: string; title?: string }
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
