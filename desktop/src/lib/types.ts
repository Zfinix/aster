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
