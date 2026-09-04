import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ChatStreamEvent,
  Finding,
  PermissionMode,
  ReviewOpts,
  StartupInfo,
  StreamEvent,
} from "./types";

export function startupInfo(): Promise<StartupInfo> {
  return invoke<StartupInfo>("startup_info");
}

export function pickRepo(): Promise<string | null> {
  return invoke<string | null>("pick_repo");
}

export function pickDiff(): Promise<string | null> {
  return invoke<string | null>("pick_diff");
}

export function listRepoFiles(repoPath: string): Promise<string[]> {
  return invoke<string[]>("list_repo_files", { repoPath });
}

export function openExternal(url: string): Promise<void> {
  return invoke<void>("open_external", { url });
}

export interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

export interface ChatHandlers {
  onEvent: (event: ChatStreamEvent) => void;
  onLog: (line: string) => void;
  onError: (message: string) => void;
  onExit: (code: number | null, error: string | null) => void;
}

/** Kick off a streaming chat turn (`aster chat --stream`). The backend owns the
 *  child; NDJSON arrives as `aster://chat-event`, exit as `aster://chat-exit`.
 *  Returns an unlisten that must be called when the turn is torn down. */
export async function runChat(
  messages: ChatMessage[],
  repoPath: string | null,
  model: string | null,
  permissionMode: PermissionMode | null,
  session: string | null,
  handlers: ChatHandlers,
): Promise<UnlistenFn> {
  const unlisteners: UnlistenFn[] = [];

  unlisteners.push(
    await listen<string>("aster://chat-event", (e) => {
      try {
        handlers.onEvent(JSON.parse(e.payload) as ChatStreamEvent);
      } catch {
        // A malformed line is not worth killing the stream over.
      }
    }),
  );
  unlisteners.push(
    await listen<string>("aster://log", (e) => handlers.onLog(e.payload)),
  );
  unlisteners.push(
    await listen<{ code: number | null; error: string | null }>(
      "aster://chat-exit",
      (e) => handlers.onExit(e.payload.code, e.payload.error),
    ),
  );

  const unlistenAll: UnlistenFn = () => unlisteners.forEach((u) => u());

  invoke("chat", { messages, repoPath, model, permissionMode, session })
    .catch((err: unknown) => handlers.onError(String(err)));

  return unlistenAll;
}

/** Answer a pending `approval_request`. */
export function answerApproval(allow: boolean): Promise<void> {
  return invoke("answer_approval", { allow });
}

/** Stop the running chat turn. */
export function cancelChat(): Promise<void> {
  return invoke("cancel_chat");
}

export interface SessionSummary {
  id: string;
  created_at: string;
  model: string | null;
  turns: number;
  title: string;
}

/** List saved chat sessions for a repo, newest first. */
export function listSessions(
  repoPath: string | null,
): Promise<SessionSummary[]> {
  return invoke<SessionSummary[]>("list_sessions", { repoPath });
}

/** Load a session's full transcript by id. */
export function showSession(
  id: string,
  repoPath: string | null,
): Promise<{ id: string; events: unknown[] }> {
  return invoke("show_session", { id, repoPath });
}

/** Delete a saved session by id. */
export function deleteSession(
  id: string,
  repoPath: string | null,
): Promise<{ ok: boolean; id: string }> {
  return invoke("delete_session", { id, repoPath });
}

export interface MemoryList {
  dir: string;
  blocks: { name: string; description: string }[];
}

/** List durable memory: project facts plus named blocks. */
export function memoryList(): Promise<MemoryList> {
  return invoke<MemoryList>("memory_list");
}

/** Save a durable fact. With a title it writes a named block; otherwise it
 *  appends to project memory (ASTER.md). */
export function memoryAdd(
  text: string,
  title?: string,
): Promise<{ ok: boolean; kind: string; path: string | null }> {
  return invoke("memory_add", { text, title: title ?? null });
}

export interface FixResult {
  file_path: string;
  line: number;
  title: string;
  status: "applied" | "preview" | "cannot_fix" | "error";
  reason?: string;
  patch?: string;
}

/** Ask the fix engine to patch one finding in the working tree. */
export function applyFix(
  finding: Finding,
  repoPath: string | null,
  model: string | null,
): Promise<FixResult> {
  return invoke<FixResult>("apply_fix", { finding, repoPath, model });
}

export interface AuthStatus {
  hasKey: boolean;
  baseUrl: string | null;
  model: string | null;
}

export function authStatus(): Promise<AuthStatus> {
  return invoke<AuthStatus>("auth_status");
}

/** Persist provider settings. Pass a single space as `apiKey` to clear it;
 *  omit (undefined) to leave the stored key untouched. */
export function saveProvider(fields: {
  apiKey?: string;
  baseUrl?: string | null;
  model?: string | null;
}): Promise<void> {
  return invoke("save_provider", {
    apiKey: fields.apiKey,
    baseUrl: fields.baseUrl ?? null,
    model: fields.model ?? null,
  });
}

/** Save the model where every surface reads it: aster.yaml, via the CLI. */
export function persistModel(model: string): Promise<void> {
  return invoke("set_model", { model });
}

/** A resolved `aster config` value: YAML scalars keep their type, lists stay
 *  arrays, and unset keys resolve to null. */
export type ConfigValue = string | number | boolean | string[] | null;

export interface ConfigEntry {
  key: string;
  value: ConfigValue;
  source: string;
  default?: ConfigValue;
  kind?: string;
}

/** Every `aster config` key with its resolved value. Backs the settings UI. */
export async function configList(
  repoPath?: string | null,
): Promise<ConfigEntry[]> {
  const res = await invoke<{ ok: boolean; keys: ConfigEntry[] }>("config_list", {
    repoPath: repoPath ?? null,
  });
  return res.keys ?? [];
}

/** Write one `aster.yaml` value. Empty clears the key back to its default. */
export function configSet(
  key: string,
  value: string,
  opts?: { repoPath?: string | null; global?: boolean },
): Promise<unknown> {
  if (!value.trim()) {
    return invoke("config_unset", {
      key,
      repoPath: opts?.repoPath ?? null,
      global: opts?.global ?? null,
    });
  }
  return invoke("config_set", {
    key,
    value,
    repoPath: opts?.repoPath ?? null,
    global: opts?.global ?? null,
  });
}

interface RunHandlers {
  onEvent: (event: StreamEvent) => void;
  onLog: (line: string) => void;
  onError: (message: string) => void;
  onDone: () => void;
}

/** Kick off a streaming review. Wires the backend's NDJSON events, stderr log
 *  lines, and terminal error into the caller's handlers, and returns an unlisten
 *  that must be called when the run is torn down. */
export async function runReview(
  opts: ReviewOpts,
  handlers: RunHandlers,
): Promise<UnlistenFn> {
  const unlisteners: UnlistenFn[] = [];

  unlisteners.push(
    await listen<string>("aster://event", (e) => {
      try {
        handlers.onEvent(JSON.parse(e.payload) as StreamEvent);
      } catch {
        // A malformed line is not worth killing the stream over.
      }
    }),
  );
  unlisteners.push(
    await listen<string>("aster://log", (e) => handlers.onLog(e.payload)),
  );

  const unlistenAll: UnlistenFn = () => unlisteners.forEach((u) => u());

  invoke("run_review", { opts })
    .catch((err: unknown) => handlers.onError(String(err)))
    .finally(() => handlers.onDone());

  return unlistenAll;
}
