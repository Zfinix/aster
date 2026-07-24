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

export type Turn =
  | { id: string; role: "user"; text: string }
  | { id: string; role: "assistant"; text: string; pending?: boolean; error?: boolean }
  | { id: string; role: "review"; data: ReviewData };

export interface Conversation {
  id: string;
  title: string;
  repoName: string;
  repoPath: string;
  whenLabel: string;
  turns: Turn[];
}

/** The 7/7-recall models from docs/BENCHMARKS.md, best first. */
export const RESEARCH_MODELS = [
  "google/gemini-3.1-flash-lite",
  "anthropic/claude-sonnet-5",
  "amazon/nova-lite-v1",
  "microsoft/phi-4",
  "qwen/qwen3-next-80b-a3b-instruct",
];

export const DEFAULT_MODEL = RESEARCH_MODELS[0];

/** Humanize one slug token by rule, not by lookup:
 *  "3.1" -> "3.1", "v1" -> "v1", "80b"/"a3b" -> "80B"/"A3B",
 *  "gpt"/"glm" -> "GPT"/"GLM" (no vowels = acronym), "qwen3" -> "Qwen3". */
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
