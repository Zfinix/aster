import { Finding, StreamEvent, UsageSummary } from "./types";

export type PermissionMode = "plan" | "manual" | "auto" | "edit" | "yolo";
export type Effort = "off" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra";

export interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

export type ReviewSource =
  | { kind: "working" }
  | { kind: "range"; value: string }
  | { kind: "pr"; value: string };

export interface SetupInfo {
  provider: string;
  base_url: string;
  login: "codex" | "openrouter" | "zai" | null;
  key_vars: string[];
}

export type ChatStreamEvent =
  | { type: "token"; content: string }
  | { type: "text"; content: string }
  | { type: "reasoning"; content: string; tokens?: number; duration_ms?: number }
  | { type: "reasoning_delta"; content: string; tokens: number }
  | { type: "reasoning_done"; tokens: number; duration_ms: number }
  | { type: "tool_call"; id: string; name: string; arguments: string }
  | { type: "tool_result"; id: string; name: string; result: string; error: boolean }
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
  | { type: "agent_activity"; call_id: string; agent: string; task?: string; line: string }
  | { type: "goal_set"; condition: string; max_turns: number }
  | {
      type: "goal_verdict";
      verdict: "met" | "not_yet" | "impossible";
      reason: string;
      turn: number;
      final: boolean;
    }
  | {
      type: "approval_request";
      kind?: "plan" | "action";
      preview: string;
      markdown?: string | null;
      scope?: string | null;
    }
  | { type: "question"; header: string; question: string; options: string[] }
  | {
      type: "compacted";
      summary: string;
      folded: number;
      messages: ChatMessage[];
    }
  | {
      type: "done";
      reply: string;
      edits: string[];
      usage?: UsageSummary;
      title?: string | null;
      context_budget?: number;
    }
  | { type: "title"; title: string }
  | { type: "notice"; message: string }
  | { type: "injected"; content: string }
  | { type: "error"; message: string; setup?: SetupInfo };

export interface SessionSummary {
  id: string;
  created_at: string;
  model: string | null;
  turns: number;
  title: string;
}

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

export interface TranscriptToolCall {
  id: string;
  name: string;
  arguments: string;
  result?: string;
  error?: boolean;
}

export interface SkillCommand {
  name: string;
  detail: string;
  plugin?: string;
}

export interface McpServer {
  name: string;
  disabled: boolean;
  transport: string | null;
  command: string;
  args: string[];
  url: string;
}

export interface Provider {
  name: string;
  base_url: string;
  example_model: string;
  current: boolean;
  key_env: string[];
}

/** How the panel gets an endpoint its credentials: a browser sign-in, a key
 *  pasted into the card, or nothing at all for a server on this machine. */
export type ConnectAuth =
  | { kind: "login"; target: string }
  | { kind: "key"; value: string }
  | { kind: "none" };

export interface PastedFile {
  name: string;
  data?: string;
  size: number;
}

export interface InfoRow {
  label: string;
  value: string;
}

export type ConfigKind = "text" | "bool" | "number" | "list" | "choice";
export type ConfigUnit = "none" | "seconds" | "chars" | "bytes" | "tokens" | "percent";
export type ConfigScope = "global" | "local";
export type ConfigValue = string | number | boolean | string[] | null;

export interface ConfigKey {
  key: string;
  label: string;
  group: string;
  kind: ConfigKind;
  choices: string[];
  unit: ConfigUnit;
  value: ConfigValue;
  display: string;
  default: string;
  source: string;
  shadowed: string | null;
  scopes: Record<ConfigScope, ConfigValue>;
  env: string[];
  help: string;
}

export interface ApiKey {
  var: string;
  provider: string;
  group: string;
  set: boolean;
  source: string;
  masked: string | null;
  help: string;
}

export interface EnvVar {
  var: string;
  group: string;
  kind: "text" | "bool" | "number" | "json";
  secret: boolean;
  set: boolean;
  source: string;
  masked: string | null;
  value: string | null;
  help: string;
}

export interface ConfigPaths {
  global: string;
  global_exists: boolean;
  project: string | null;
  project_exists: boolean;
  project_default: string;
}

export interface EditorSettings {
  binaryPath: string;
  minConfidence: number | null;
  publishDiagnostics: boolean;
  extraArgs: string[];
}

export interface SettingsSnapshot {
  keys: ConfigKey[];
  apiKeys: ApiKey[];
  envVars: EnvVar[];
  paths: ConfigPaths | null;
  editor: EditorSettings;
  servers: McpServer[];
  models: string[];
  providers: Provider[];
  workspaceRoot: string | null;
  binaryOk: boolean;
  error?: string;
}

export type SettingsToHost =
  | { type: "ready" }
  | { type: "setKey"; key: string; value: Exclude<ConfigValue, null>; scope: ConfigScope }
  | { type: "unsetKey"; key: string; scope: ConfigScope }
  | { type: "setApiKey"; var: string; value: string; scope: ConfigScope }
  | { type: "unsetApiKey"; var: string; scope: ConfigScope }
  | { type: "revealApiKey"; var: string }
  | { type: "setEnv"; var: string; value: string; scope: ConfigScope }
  | { type: "unsetEnv"; var: string; scope: ConfigScope }
  | { type: "revealEnv"; var: string }
  | { type: "setEditor"; key: keyof EditorSettings; value: ConfigValue }
  | { type: "toggleMcp"; name: string; disabled: boolean }
  | { type: "openConfigFile"; scope: ConfigScope }
  | { type: "reload" };

export type SettingsToWebview =
  | { type: "settings"; snapshot: SettingsSnapshot }
  | { type: "apiKeyValue"; var: string; value: string | null }
  | { type: "envValue"; var: string; value: string | null }
  | { type: "settingsError"; key?: string; message: string };

export type ToHost =
  | { type: "ready" }
  | {
      type: "chat";
      id: string;
      messages: ChatMessage[];
      model: string | null;
      permissionMode: PermissionMode;
      effort: Effort | null;
      session: string | null;
    }
  | { type: "approval"; allow: boolean; always?: boolean }
  | { type: "answer"; choice: string | null }
  | { type: "inject"; text: string }
  | { type: "cancelChat" }
  | { type: "review"; id: string; source: ReviewSource }
  | { type: "cancelReview" }
  | { type: "openFinding"; finding: Finding }
  | { type: "openFile"; path: string; needle?: string; line?: number }
  | { type: "readFile"; path: string; requestId: string }
  | { type: "openSettings" }
  | { type: "openExternal"; url: string }
  | { type: "openUntitled"; content: string; lang?: string; title?: string; doc?: boolean }
  | { type: "setPermissionMode"; mode: PermissionMode }
  | { type: "setModel"; model: string }
  | { type: "setEffort"; effort: Effort | null }
  | { type: "searchFiles"; query: string; requestId: string }
  | { type: "runCommand"; command: string }
  | { type: "fixFinding"; finding: Finding }
  | { type: "fixAllFindings"; findings: Finding[] }
  | { type: "listSessions" }
  | { type: "loadSession"; id: string }
  | { type: "deleteSession"; id: string }
  | { type: "renameSession"; id: string; title: string }
  | { type: "fetchModels" }
  | { type: "info"; id: string; topic: "status" | "memory" | "diff" | "mom" }
  | { type: "attachFiles" }
  | { type: "dropFiles"; uris: string[] }
  | { type: "pasteFiles"; files: PastedFile[] }
  | { type: "listMcp" }
  | { type: "toggleMcp"; name: string; disabled: boolean }
  | { type: "listProviders" }
  | { type: "setProvider"; baseUrl: string; model: string }
  | { type: "login"; target: string }
  | { type: "connect"; baseUrl: string; model: string; auth: ConnectAuth }
  | { type: "compact"; id: string; messages: ChatMessage[] }
  | { type: "dismissAnnouncements"; ids: string[] }
  | { type: "installCli" }
  | { type: "installCliTerminal" }
  | { type: "locateCli" };

export type ToWebview =
  | {
      type: "init";
      workspaceRoot: string | null;
      repoName: string | null;
      branch: string | null;
      model: string | null;
      models: string[];
      recommended: string[];
      recent: string[];
      contextBudget: number;
      permissionMode: PermissionMode;
      effort: Effort | null;
      binaryOk: boolean;
      skills: SkillCommand[];
      setup?: SetupInfo | null;
      announcements?: { id: string; text: string }[];
    }
  | { type: "chatEvent"; id: string; event: ChatStreamEvent }
  | { type: "chatError"; id: string; message: string }
  | { type: "loginOutput"; line: string }
  | { type: "loginDone"; ok: boolean; message: string }
  | { type: "connectDone"; ok: boolean; message: string }
  | { type: "installCliProgress"; message: string }
  | { type: "installCliDone"; ok: boolean; message: string }
  | {
      type: "runState";
      review: boolean;
      chat: boolean;
      id?: string;
      pending?: ChatStreamEvent;
    }
  | { type: "sessions"; sessions: SessionSummary[] }
  | { type: "sessionLoaded"; id: string; title: string | null; turns: TranscriptTurn[] }
  | { type: "newConversation" }
  | { type: "insertMention"; text: string; mentions?: string[] }
  | { type: "openCommandMenu" }
  | { type: "scratch"; content: string; lang?: string; title?: string; doc?: boolean }
  | { type: "fileResults"; requestId: string; paths: string[] }
  | {
      type: "filePreview";
      requestId: string;
      file: { path: string; lang?: string; content: string; truncated: boolean; image?: string; doc?: string; size?: number } | null;
    }
  | { type: "reviewStarted"; id: string; source: ReviewSource }
  | { type: "reviewEvent"; id: string; event: StreamEvent }
  | { type: "reviewDone"; id: string }
  | { type: "reviewError"; id: string; message: string }
  | { type: "fixResult"; finding: Finding; status: string; reason?: string; patch?: string }
  | { type: "fixAllResult"; results: { finding: Finding; status: string; reason?: string }[] }
  | { type: "log"; line: string }
  | { type: "modelsLoaded"; models: string[]; error?: string }
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
  | { type: "providerChanged"; provider: string; model: string; models: string[] }
  | {
      type: "compacted";
      id: string;
      summary: string;
      folded: number;
      messages: TranscriptTurn[];
    };
