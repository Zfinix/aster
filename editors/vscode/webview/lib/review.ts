import type { Finding, StreamEvent } from "../../src/types";
import { parseDiffFiles, type ReviewData } from "./thread";

/** Where one issue stands after the reader acted on it. No entry means open. */
export type FixState =
  | { status: "fixing" }
  | { status: "fixed"; patch?: string }
  | { status: "failed"; reason: string }
  | { status: "blocked"; reason: string; patch?: string }
  | { status: "dismissed" };

/** A queued "Fix all": `keys` is the batch it started with, `queue` what is
 *  still waiting. Fixes run one at a time so every row gets its own result. */
export interface FixAll {
  keys: string[];
  queue: string[];
}

export interface Tally {
  open: number;
  fixed: number;
  dismissed: number;
  failed: number;
}

export const findingKey = (f: Pick<Finding, "file_path" | "line" | "title">) =>
  `${f.file_path}:${f.line}:${f.title}`;

export const fixOf = (data: ReviewData, finding: Finding): FixState | undefined =>
  data.fixes?.[findingKey(finding)];

export function applyReviewEvent(data: ReviewData, event: StreamEvent): ReviewData {
  switch (event.type) {
    case "phase":
      return { ...data, phase: event.name };
    case "hypothesized":
      return { ...data, candidates: event.count };
    case "verifying":
      return {
        ...data,
        verify: { index: event.index, total: event.total, title: event.title },
      };
    case "diff":
      return { ...data, files: parseDiffFiles(event.content) };
    case "finding": {
      const { type: _type, ...finding } = event;
      return { ...data, findings: [...data.findings, finding] };
    }
    case "refuted": {
      const { type: _type, ...ruledOut } = event;
      return { ...data, refuted: [...data.refuted, ruledOut] };
    }
    case "done":
      return { ...data, summary: event.summary, usage: event.usage };
    case "token":
      return data;
  }
}

export function setFix(data: ReviewData, key: string, fix: FixState | null): ReviewData {
  const fixes = { ...data.fixes };
  if (fix) {
    fixes[key] = fix;
  } else {
    delete fixes[key];
  }
  return { ...data, fixes };
}

/** Fold a host's `fixResult` into the review that asked for it. Only an issue
 *  marked fixing takes it, so a stale or foreign reply changes nothing. */
export function applyFixResult(
  data: ReviewData,
  finding: Finding,
  status: string,
  reason?: string,
  patch?: string
): ReviewData {
  const key = findingKey(finding);
  if (data.fixes?.[key]?.status !== "fixing") return data;
  const said = reason?.trim() || "Aster couldn't change this file.";
  switch (status) {
    case "applied":
      return setFix(data, key, { status: "fixed", patch });
    case "blocked":
      return setFix(data, key, { status: "blocked", reason: said, patch });
    default:
      return setFix(data, key, { status: "failed", reason: said });
  }
}

export function startFixAll(data: ReviewData): ReviewData {
  const keys = data.findings
    .filter((f) => !fixOf(data, f) || fixOf(data, f)?.status === "failed")
    .map(findingKey);
  return keys.length === 0 ? data : { ...data, fixAll: { keys, queue: keys } };
}

/** The next issue a running "Fix all" should send, marked fixing. `null` while
 *  one is still in flight or when the queue is empty. */
export function takeNextFix(data: ReviewData): { data: ReviewData; finding: Finding } | null {
  const run = data.fixAll;
  if (!run || run.queue.length === 0) return null;
  if (Object.values(data.fixes ?? {}).some((fix) => fix.status === "fixing")) return null;
  const [key, ...queue] = run.queue;
  const finding = data.findings.find((f) => findingKey(f) === key);
  const next = { ...data, fixAll: { ...run, queue } };
  if (!finding || fixOf(data, finding)?.status === "dismissed") {
    return takeNextFix(next);
  }
  return { data: setFix(next, key, { status: "fixing" }), finding };
}

export const fixAllRunning = (data: ReviewData) =>
  !!data.fixAll &&
  data.fixAll.keys.some((key) => {
    const status = data.fixes?.[key]?.status;
    return status === "fixing" || data.fixAll?.queue.includes(key);
  });

export function tally(data: ReviewData, keys?: string[]): Tally {
  const counts: Tally = { open: 0, fixed: 0, dismissed: 0, failed: 0 };
  const only = keys && new Set(keys);
  for (const finding of data.findings) {
    const key = findingKey(finding);
    if (only && !only.has(key)) continue;
    const status = data.fixes?.[key]?.status;
    if (status === "fixed") counts.fixed++;
    else if (status === "dismissed") counts.dismissed++;
    else if (status === "failed" || status === "blocked") counts.failed++;
    else counts.open++;
  }
  return counts;
}
