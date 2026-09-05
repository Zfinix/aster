export type Severity = "critical" | "high" | "medium" | "low" | "info";

export interface Finding {
  file_path: string;
  line: number;
  start_line?: number | null;
  side?: string | null;
  severity: Severity;
  category: string;
  title: string;
  description: string;
  suggestion: string;
  code_snippet?: string | null;
  confidence?: number | null;
}

export interface Usage {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  requests: number;
  estimated_cost_usd?: number | null;
  estimated: boolean;
}

export type StreamEvent =
  | { type: "diff"; content: string; repo_name: string; base_branch: string }
  | { type: "phase"; name: string }
  | { type: "token"; stage: string; delta: string }
  | { type: "hypothesized"; count: number }
  | { type: "verifying"; index: number; total: number; title: string }
  | ({ type: "finding" } & Finding)
  | { type: "refuted"; title: string; reason: string }
  | { type: "done"; summary: string; total: number; usage: Usage };

/** Mirrors `aster_policy::Mode`; see crates/aster-policy/src/decision.rs. */
export type PermissionMode = "plan" | "manual" | "auto" | "edit" | "yolo";

export type Effort = "off" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra";

export interface Provider {
  name: string;
  base_url: string;
  example_model: string;
  current: boolean;
  key_env: string[];
}

/** One line of `aster chat --stream` output. */
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
  | {
      type: "approval_request";
      kind?: "plan" | "action";
      preview: string;
      markdown?: string;
    }
  | { type: "done"; reply: string; edits: string[]; usage?: Usage }
  | { type: "title"; title: string }
  | { type: "notice"; message: string }
  | { type: "error"; message: string };

export type LineKind = "add" | "del" | "ctx" | "hunk";

export interface DiffLine {
  kind: LineKind;
  oldNo: number | null;
  newNo: number | null;
  text: string;
}

export interface DiffFile {
  oldPath: string;
  newPath: string;
  status: "added" | "deleted" | "modified" | "renamed";
  lines: DiffLine[];
  additions: number;
  deletions: number;
}

export type SourceKind = "working" | "range" | "pr" | "diff";

export interface ReviewOpts {
  repoPath: string;
  sourceKind: SourceKind;
  sourceValue: string | null;
  minConfidence: number;
  noIndex: boolean;
  model: string | null;
  apiKey: string | null;
  analyzers: string[];
}

export interface StartupInfo {
  defaultRepo: string | null;
  binPath: string | null;
}
