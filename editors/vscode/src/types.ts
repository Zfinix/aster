export interface Finding {
  file_path: string;
  line: number;
  start_line?: number | null;
  side?: string | null;
  severity: "critical" | "high" | "medium" | "low" | "info" | string;
  category: string;
  title: string;
  description: string;
  suggestion: string;
  code_snippet?: string | null;
  confidence?: number | null;
}

export interface UsageSummary {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  requests: number;
  estimated_cost_usd?: number | null;
  estimated: boolean;
}

export type StreamEvent =
  | { type: "diff"; content: string; repo_name?: string; base_branch?: string }
  | { type: "phase"; name: string }
  | { type: "token"; stage: string; delta: string }
  | { type: "hypothesized"; count: number }
  | { type: "verifying"; index: number; total: number; title: string }
  | ({ type: "finding" } & Finding)
  | { type: "refuted"; title: string; reason: string }
  | { type: "done"; summary: string; total: number; usage?: UsageSummary };
