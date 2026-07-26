import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Finding, ReviewOpts, StartupInfo, StreamEvent } from "./types";

export function startupInfo(): Promise<StartupInfo> {
  return invoke<StartupInfo>("startup_info");
}

export function pickRepo(): Promise<string | null> {
  return invoke<string | null>("pick_repo");
}

export function pickDiff(): Promise<string | null> {
  return invoke<string | null>("pick_diff");
}

export interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

export interface ChatReply {
  reply: string;
  /** Repo-relative paths the agent edited during this turn. */
  edits: string[];
}

/** A conversational reply from Aster (not a review). Runs `aster chat` with
 *  the repo as cwd so provider resolution matches review exactly. The agent
 *  can read/search the repo; with allowEdits it can change it. */
export function chat(
  messages: ChatMessage[],
  repoPath: string | null,
  model: string | null,
  allowEdits: boolean,
  session: string | null,
): Promise<ChatReply> {
  return invoke<ChatReply>("chat", {
    messages,
    repoPath,
    model,
    allowEdits,
    session,
  });
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

interface RunHandlers {
  onEvent: (event: StreamEvent) => void;
  onLog: (line: string) => void;
  onError: (message: string) => void;
  onDone: () => void;
}

/**
 * Kick off a streaming review. Wires the backend's NDJSON events, stderr log
 * lines, and terminal error into the caller's handlers, and returns an unlisten
 * that must be called when the run is torn down.
 */
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
