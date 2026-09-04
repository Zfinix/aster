import type { DiffFile, Finding, SourceKind, Usage } from "./types";

export interface Refuted {
  title: string;
  reason: string;
}

/** The artifacts of a single code review, carried by a `review` turn. */
export interface ReviewData {
  status: "running" | "done" | "error";
  phase: string;
  baseBranch: string;
  files: DiffFile[];
  findings: Finding[];
  refuted: Refuted[];
  summary: string;
  usage: Usage | null;
  errorMsg?: string;
}

export function emptyReview(): ReviewData {
  return {
    status: "running",
    phase: "",
    baseBranch: "",
    files: [],
    findings: [],
    refuted: [],
    summary: "",
    usage: null,
  };
}

/** One tool call the chat agent made during a turn, derived from the session
 *  transcript. `output` is the tool's result text, shown when a step expands. */
export interface ToolStep {
  id: string;
  name: string;
  label: string;
  output?: string;
}

export type Turn =
  | { id: string; role: "user"; text: string; ts?: number }
  | {
      id: string;
      role: "assistant";
      text: string;
      ts?: number;
      pending?: boolean;
      error?: boolean;
      steps?: ToolStep[];
      reasoning?: string;
      reasoningTokens?: number;
      reasoningDurationMs?: number;
      reasoningDone?: boolean;
      agents?: AgentRun[];
      usage?: Usage | null;
      stopped?: boolean;
    }
  | { id: string; role: "review"; data: ReviewData; ts?: number };

/** One sub-agent inside an `agent` tool call, fed by `agent_status` events. */
export interface AgentRun {
  callId: string;
  agent: string;
  task?: string;
  status: "running" | "done" | "error";
  report?: string;
  error?: string;
  done: number;
  total: number;
}

/** Insert or replace a sub-agent's row, keyed by call id + agent name. */
export function upsertAgent(
  agents: AgentRun[] | undefined,
  ev: {
    call_id: string;
    agent: string;
    task?: string;
    status: "running" | "done" | "error";
    report?: string;
    error?: string;
    done: number;
    total: number;
  },
): AgentRun[] {
  const row: AgentRun = {
    callId: ev.call_id,
    agent: ev.agent,
    task: ev.task,
    status: ev.status,
    report: ev.report,
    error: ev.error,
    done: ev.done,
    total: ev.total,
  };
  const i = (agents ?? []).findIndex((r) => r.callId === ev.call_id && r.agent === ev.agent);
  return i === -1 ? [...(agents ?? []), row] : agents!.map((r, k) => (k === i ? row : r));
}

/** "just now", "5 minutes ago", "2 hours ago", "3 days ago". */
export function timeAgo(ts: number): string {
  const s = Math.max(0, (Date.now() - ts) / 1000);
  if (s < 60) return "just now";
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} minute${m === 1 ? "" : "s"} ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h} hour${h === 1 ? "" : "s"} ago`;
  const d = Math.floor(h / 24);
  return `${d} day${d === 1 ? "" : "s"} ago`;
}

/** Friendly one-line label for a tool call, à la Claude's activity rows. */
export function stepLabel(name: string, args: Record<string, unknown>): string {
  const s = (v: unknown) => (typeof v === "string" ? v : "");
  switch (name) {
    case "read_file":
      return `Read ${s(args.path) || "file"}`;
    case "list_files":
      return `Listed ${s(args.dir) || "project root"}`;
    case "search_files":
      return `Searched “${s(args.query)}”`;
    case "find_files":
      return `Found files matching ${s(args.pattern)}`;
    case "run_command":
      return s(args.description) || `Ran ${s(args.command) || "a command"}`;
    case "edit_file":
      return `Edited ${s(args.path) || "file"}`;
    case "open_preview":
      return s(args.description) || `Opened ${s(args.target)}`;
    case "remember":
      return "Saved to memory";
    case "recall":
      return `Recalled ${s(args.name)}`;
    case "forget":
      return `Forgot ${s(args.name)}`;
    case "read_skill":
      return `Read skill ${s(args.name)}`;
    case "agent": {
      const tasks = Array.isArray(args.tasks)
        ? (args.tasks as { agent?: unknown }[])
        : [];
      const names = tasks
        .map((t) => (typeof t.agent === "string" ? t.agent : ""))
        .filter(Boolean);
      const unique = [...new Set(names)];
      if (names.length === 0) return "agent";
      if (names.length === 1) return `agent: ${unique[0]}`;
      if (unique.length === 1) return `agent ×${names.length}: ${unique[0]}`;
      return `agent ×${names.length}: ${unique.join(", ")}`;
    }
    default:
      return name.replace(/_/g, " ");
  }
}

interface RawEvent {
  type?: string;
  role?: string;
  content?: string;
  ts?: string;
  tool_call_id?: string;
  tool_calls?: { id: string; function: { name: string; arguments: string } }[];
  reasoning?: { text?: string; tokens?: number; duration_ms?: number };
}

function toStep(
  tc: { id: string; function: { name: string; arguments: string } },
  results: Map<string, string>,
): ToolStep {
  let args: Record<string, unknown> = {};
  try {
    args = JSON.parse(tc.function.arguments || "{}");
  } catch {
    args = {};
  }
  return {
    id: tc.id,
    name: tc.function.name,
    label: stepLabel(tc.function.name, args),
    output: results.get(tc.id),
  };
}

/** Rebuild a conversation from a saved transcript. Every assistant round
 *  between two user messages folds into one turn, the way it was shown live:
 *  steps accumulate, thinking joins with a blank line, commentary joins the reply. */
export function turnsFromEvents(events: unknown[], idPrefix = "h"): Turn[] {
  const evs = (events as RawEvent[]).filter((e) => e?.type === "message");
  // Results arrive as their own `tool` events after the call that produced them.
  const results = new Map<string, string>();
  for (const e of evs) {
    if (e.role === "tool" && e.tool_call_id) {
      results.set(e.tool_call_id, e.content ?? "");
    }
  }

  const turns: Turn[] = [];
  let n = 0;
  const nextId = () => `${idPrefix}${++n}`;
  const stamp = (e: RawEvent) => {
    const ms = e.ts ? Date.parse(e.ts) : NaN;
    return Number.isNaN(ms) ? undefined : ms;
  };
  // The assistant turn currently absorbing rounds; a user message closes it.
  let open: Extract<Turn, { role: "assistant" }> | null = null;

  for (const e of evs) {
    if (e.role === "user") {
      const text = e.content ?? "";
      if (!text.trim()) continue;
      open = null;
      turns.push({ id: nextId(), role: "user", text, ts: stamp(e) });
      continue;
    }
    // Harness steering is recorded as `system`, and tool results were indexed
    // above; neither is a turn.
    if (e.role !== "assistant") continue;

    if (!open) {
      open = { id: nextId(), role: "assistant", text: "", ts: stamp(e) };
      turns.push(open);
    }
    const text = (e.content ?? "").trim();
    if (text) {
      open.text = open.text ? `${open.text}\n\n${text}` : text;
    }
    const thinking = e.reasoning?.text?.trim();
    if (thinking) {
      open.reasoning = open.reasoning ? `${open.reasoning}\n\n${thinking}` : thinking;
      // The live panel shows the newest round's count over the joined text, so
      // a reopened turn has to read the same rather than a total.
      open.reasoningTokens = e.reasoning?.tokens;
      open.reasoningDurationMs = e.reasoning?.duration_ms;
      open.reasoningDone = true;
    }
    if (e.tool_calls?.length) {
      open.steps = [...(open.steps ?? []), ...e.tool_calls.map((tc) => toStep(tc, results))];
    }
  }

  // A turn that ran tools and never answered (stopped, or the cap tripped) is
  // still worth showing; one that carries nothing at all is not.
  return turns.filter(
    (t) => t.role !== "assistant" || t.text || t.steps?.length || t.reasoning,
  );
}

/** Extract the tool calls of the most recent turn from a session transcript:
 *  every assistant tool call after the last user message, paired with its
 *  result by tool_call_id. */
export function stepsFromEvents(events: unknown[]): ToolStep[] {
  const evs = events as RawEvent[];
  let lastUser = -1;
  evs.forEach((e, i) => {
    if (e?.type === "message" && e.role === "user") lastUser = i;
  });

  const results = new Map<string, string>();
  for (let i = lastUser + 1; i < evs.length; i++) {
    const e = evs[i];
    if (e?.type === "message" && e.role === "tool" && e.tool_call_id) {
      results.set(e.tool_call_id, e.content ?? "");
    }
  }

  const steps: ToolStep[] = [];
  for (let i = lastUser + 1; i < evs.length; i++) {
    const e = evs[i];
    if (e?.type === "message" && e.role === "assistant" && e.tool_calls?.length) {
      steps.push(...e.tool_calls.map((tc) => toStep(tc, results)));
    }
  }
  return steps;
}

/** A compact summary line for a collapsed activity panel, e.g.
 *  "Read 2 files · searched 1". */
export function summarizeSteps(steps: ToolStep[]): string {
  if (steps.length === 1) return steps[0].label;

  const count = (name: string) => steps.filter((s) => s.name === name).length;
  const reads = count("read_file");
  const lists = count("list_files");
  const searches = count("search_files");
  const edits = count("edit_file");
  const other = steps.length - reads - lists - searches - edits;

  const parts: string[] = [];
  if (edits) parts.push(`edited ${edits} file${edits > 1 ? "s" : ""}`);
  if (reads) parts.push(`read ${reads} file${reads > 1 ? "s" : ""}`);
  if (lists) parts.push(`listed ${lists} dir${lists > 1 ? "s" : ""}`);
  if (searches) parts.push(`searched ${searches}`);
  if (other) parts.push(`${other} more`);

  const s = parts.join(" · ") || `${steps.length} steps`;
  return s.charAt(0).toUpperCase() + s.slice(1);
}

export interface Conversation {
  id: string;
  title: string;
  repoName: string;
  repoPath: string;
  whenLabel: string;
  turns: Turn[];
  sessionId?: string;
  renamed?: boolean;
}

export const RESEARCH_MODELS = [
  "google/gemini-3.1-flash-lite",
  "anthropic/claude-sonnet-5",
  "amazon/nova-lite-v1",
  "microsoft/phi-4",
  "qwen/qwen3-next-80b-a3b-instruct",
];

export const DEFAULT_MODEL = RESEARCH_MODELS[0];

function caseToken(w: string): string {
  if (/^[0-9.]+$/.test(w)) return w;
  if (/^v\d+$/i.test(w)) return w.toLowerCase();
  if (/\d/.test(w) && w.length <= 3) return w.toUpperCase();
  if (/^[a-z]+$/i.test(w) && !/[aeiouy]/i.test(w)) return w.toUpperCase();
  return w.charAt(0).toUpperCase() + w.slice(1);
}

/** "google/gemini-3.1-flash-lite" -> "Gemini 3.1 Flash Lite" */
export function modelShort(id: string): string {
  const slug = id.split("/").pop() || id;
  return slug.split("-").map(caseToken).join(" ");
}

/** "google/gemini-3.1-flash-lite" -> "google" */
export function modelProvider(id: string): string {
  return id.includes("/") ? id.split("/")[0] : "";
}

export const SOURCE_LABELS: Record<SourceKind, string> = {
  working: "working tree",
  range: "main..HEAD",
  pr: "pull request",
  diff: "diff file",
};

export const SOURCE_DEFAULT_VALUE: Record<SourceKind, string | null> = {
  working: null,
  range: "main..HEAD",
  pr: "",
  diff: "",
};

/** A concise, repo-free title for a review row, derived from its source.
 *  The repo is already shown by the folder group, so we don't repeat it. */
export function defaultReviewTitle(kind: SourceKind, value: string | null): string {
  switch (kind) {
    case "working":
      return "Working tree";
    case "range":
      return value || "main..HEAD";
    case "pr":
      return value ? `PR #${value.replace(/^#/, "")}` : "Pull request";
    case "diff":
      return value ? value.split("/").pop() || "Diff" : "Diff";
  }
}

export function repoNameOf(path: string): string {
  const parts = path.replace(/\/+$/, "").split("/");
  return parts[parts.length - 1] || path;
}

/** The latest review turn in a conversation, if any. */
export function latestReview(c: Conversation): ReviewData | null {
  for (let i = c.turns.length - 1; i >= 0; i--) {
    const t = c.turns[i];
    if (t.role === "review") return t.data;
  }
  return null;
}

let counter = 0;
export function nextId(): string {
  counter += 1;
  return `t${counter}`;
}
