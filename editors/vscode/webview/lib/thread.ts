import type { Finding, UsageSummary } from "../../src/types";
import type { ChatMessage, InfoRow, SetupInfo } from "../../src/protocol";

export interface ToolCall {
  id: string;
  name: string;
  arguments: string;
  result?: string;
  error?: boolean;
  stopped?: boolean;
}

export interface ReviewData {
  status: "running" | "done" | "error" | "stopped";
  phase: string;
  candidates?: number;
  verify?: { index: number; total: number; title: string };
  findings: Finding[];
  refuted: { title: string; reason: string }[];
  summary: string;
  usage?: UsageSummary;
  errorMsg?: string;
  files: string[];
}

/** One sub-agent inside an `agent` tool call, fed by `agent_status` events. */
export interface AgentTaskState {
  callId: string;
  agent: string;
  task?: string;
  status: "running" | "done" | "error";
  report?: string;
  error?: string;
  done: number;
  total: number;
  log?: string[];
  startedAt?: number;
  endedAt?: number;
}

/** One chronological slice of a turn. Text and tool groups are kept in arrival
 *  order so steps render under the thought that produced them, the way the CLI
 *  prints them, instead of piling up above the whole reply. */
export type TurnBlock =
  | { kind: "text"; id: string; text: string }
  | { kind: "tools"; id: string; calls: ToolCall[] }
  | {
      kind: "reasoning";
      id: string;
      text: string;
      tokens?: number;
      durationMs?: number;
      done?: boolean;
    }
  | { kind: "agents"; id: string; callId: string; tasks: AgentTaskState[] }
  | { kind: "injected"; id: string; text: string }
  | {
      kind: "goal";
      id: string;
      condition: string;
      maxTurns: number;
      verdicts: GoalVerdictRow[];
      outcome?: GoalOutcome;
    };

/** One judge verdict on the goal, one row of the card's timeline. */
export interface GoalVerdictRow {
  verdict: "met" | "not_yet" | "impossible";
  reason: string;
  turn: number;
}

export type GoalOutcome = "met" | "impossible" | "exhausted" | "stopped";

export type PlanStepStatus = "pending" | "in_progress" | "done" | "skipped" | "blocked";

export interface PlanStep {
  label: string;
  status: PlanStepStatus;
}

const PLAN_STATUSES: PlanStepStatus[] = ["pending", "in_progress", "done", "skipped", "blocked"];

export function parsePlan(args: string): PlanStep[] | null {
  try {
    const parsed = JSON.parse(args) as { steps?: { label?: unknown; status?: unknown }[] };
    if (!Array.isArray(parsed.steps) || parsed.steps.length === 0) return null;
    return parsed.steps.map((step) => ({
      label: String(step.label ?? ""),
      status: PLAN_STATUSES.includes(step.status as PlanStepStatus)
        ? (step.status as PlanStepStatus)
        : "pending",
    }));
  } catch {
    return null;
  }
}

/** An open approval. `scope` is the directory or credential a "don't ask again"
 *  would be remembered against; without one there is nothing to remember. */
export interface ApprovalAsk {
  preview: string;
  markdown?: string | null;
  scope?: string | null;
  kind?: "plan" | "action";
}

/** An open `ask_user` prompt: a headline, the question, and the offered answers. */
export interface Question {
  header: string;
  question: string;
  options: string[];
}

export interface AssistantTurn {
  id: string;
  role: "assistant";
  text: string;
  blocks: TurnBlock[];
  approval?: ApprovalAsk;
  question?: Question;
  streamed?: boolean;
  pending?: boolean;
  error?: boolean;
  errorMsg?: string;
  setup?: SetupInfo;
  stopped?: boolean;
  edits?: string[];
  usage?: UsageSummary;
}

/** A local answer that never went to the model: `/status`, `/memory`, `/diff`. */
export interface InfoCardData {
  id: string;
  title: string;
  rows?: InfoRow[];
  body?: string;
  lang?: string;
  doc?: boolean;
  note?: string;
  error?: boolean;
  pending?: boolean;
}

/** The kind that belongs in the transcript, because it records something that
 *  happened to the conversation rather than answering a question about it. */
export interface InfoTurn extends InfoCardData {
  role: "info";
}

/** A seam in the list marking a model/provider switch, so messages either side
 *  of it read as from different models. `label` is the humanized new model. */
export interface DividerTurn {
  id: string;
  role: "divider";
  label: string;
}

/** The seam a compaction leaves. Everything above it stays on screen and stops
 *  being sent; `messages` is the folded history that goes in its place. */
export interface CompactionTurn {
  id: string;
  role: "compaction";
  summary: string;
  folded: number;
  messages: ChatMessage[];
}

export type Turn =
  | { id: string; role: "user"; text: string }
  | AssistantTurn
  | InfoTurn
  | { id: string; role: "review"; data: ReviewData }
  | DividerTurn
  | CompactionTurn;

export function emptyReview(): ReviewData {
  return { status: "running", phase: "Starting", findings: [], refuted: [], summary: "", files: [] };
}

/** File paths from a unified git diff, from `diff --git a/X b/Y` headers. The
 *  `b/` side names the file as it exists after the change, which is the path a
 *  finding's `file_path` points at. */
export function parseDiffFiles(diff: string): string[] {
  const files = new Set<string>();
  for (const line of diff.split("\n")) {
    const m = /^diff --git a\/(.*?) b\/(.*)$/.exec(line);
    if (!m) continue;
    const b = m[2];
    files.add(b === "/dev/null" ? m[1] : b);
  }
  return [...files];
}

let blocks = 0;
const blockId = () => `b${++blocks}`;

/** Webview state persisted before turns kept a block timeline has `text` and a
 *  flat `tools` list instead. Fold it into one text block so a panel restored
 *  across an extension update still renders. */
export function hydrate(turns: Turn[] | undefined): Turn[] {
  const restored = (turns ?? []).map((turn) => {
    if (turn.role !== "assistant" || Array.isArray(turn.blocks)) return turn;
    const legacy = turn as AssistantTurn & { tools?: ToolCall[] };
    return {
      ...turn,
      blocks: [
        ...(legacy.tools?.length ? [{ kind: "tools" as const, id: blockId(), calls: legacy.tools }] : []),
        ...(legacy.text ? [{ kind: "text" as const, id: blockId(), text: legacy.text }] : []),
      ],
    };
  });
  // Ids are minted per mount, so start past whatever came back.
  blocks = restored.reduce(
    (n, turn) => (turn.role === "assistant" ? n + turn.blocks.length : n),
    blocks
  );
  return restored;
}

export function newTurn(id: string): AssistantTurn {
  return { id, role: "assistant", text: "", blocks: [], pending: true };
}

/** `separator` joins the chunk onto whatever came before in the flattened text.
 *  A chunk that opens a new block never carries it, so the block itself never
 *  starts with blank lines. */
export function appendText(turn: AssistantTurn, chunk: string, separator = ""): AssistantTurn {
  if (!chunk) return turn;
  const last = turn.blocks[turn.blocks.length - 1];
  return {
    ...turn,
    text: turn.text ? turn.text + separator + chunk : chunk,
    blocks:
      last?.kind === "text"
        ? [...turn.blocks.slice(0, -1), { ...last, text: last.text + separator + chunk }]
        : [...turn.blocks, { kind: "text", id: blockId(), text: chunk }],
  };
}

export function appendReasoning(
  turn: AssistantTurn,
  text: string,
  tokens?: number,
  durationMs?: number,
): AssistantTurn {
  if (!text.trim()) return turn;
  return {
    ...turn,
    blocks: [...turn.blocks, { kind: "reasoning", id: blockId(), text, tokens, durationMs, done: true }],
  };
}

/** Append a live thinking fragment, growing the in-flight block's token count. */
export function appendReasoningDelta(turn: AssistantTurn, chunk: string, tokens: number): AssistantTurn {
  if (!chunk) return turn;
  const last = turn.blocks[turn.blocks.length - 1];
  if (last?.kind === "reasoning" && !last.done) {
    return {
      ...turn,
      blocks: [...turn.blocks.slice(0, -1), { ...last, text: last.text + chunk, tokens }],
    };
  }
  return {
    ...turn,
    blocks: [...turn.blocks, { kind: "reasoning", id: blockId(), text: chunk, tokens, done: false }],
  };
}

/** Close the in-flight reasoning block with the final token count and duration. */
export function finishReasoning(turn: AssistantTurn, tokens: number, durationMs: number): AssistantTurn {
  const last = turn.blocks[turn.blocks.length - 1];
  if (last?.kind === "reasoning") {
    return {
      ...turn,
      blocks: [...turn.blocks.slice(0, -1), { ...last, tokens, durationMs, done: true }],
    };
  }
  return turn;
}

export function appendInjected(turn: AssistantTurn, text: string): AssistantTurn {
  return { ...turn, blocks: [...turn.blocks, { kind: "injected", id: blockId(), text }] };
}

/** Mount the goal card as soon as the CLI announces the judged loop. */
export function setGoal(turn: AssistantTurn, condition: string, maxTurns: number): AssistantTurn {
  return {
    ...turn,
    blocks: [
      ...turn.blocks,
      { kind: "goal", id: blockId(), condition, maxTurns, verdicts: [] },
    ],
  };
}

/** Append one judge verdict to the open goal block; `final` closes the card. */
export function appendGoalVerdict(
  turn: AssistantTurn,
  row: GoalVerdictRow,
  final: boolean
): AssistantTurn {
  const outcome: GoalOutcome | undefined = !final
    ? undefined
    : row.verdict === "met"
      ? "met"
      : row.verdict === "impossible"
        ? "impossible"
        : "exhausted";
  return {
    ...turn,
    blocks: turn.blocks.map((block) =>
      block.kind === "goal" && block.outcome === undefined
        ? { ...block, verdicts: [...block.verdicts, row], outcome }
        : block
    ),
  };
}

/** Rebuild a saved assistant turn in the order the live events arrived: what it
 *  thought, said, then ran. A reopened session used to come back as one flat
 *  text block, losing every reasoning panel and tool step. */
export function restoreTurn(
  id: string,
  content: string,
  reasoning?: { text: string; tokens?: number; durationMs?: number },
  calls: ToolCall[] = []
): AssistantTurn {
  let turn = newTurn(id);
  if (reasoning) {
    turn = appendReasoning(turn, reasoning.text, reasoning.tokens, reasoning.durationMs);
  }
  turn = appendText(turn, content);
  for (const call of calls) {
    turn = appendCall(turn, call);
  }
  return { ...turn, pending: false };
}

/** Consecutive calls join one group; a call after text opens a new one. */
export function appendCall(turn: AssistantTurn, call: ToolCall): AssistantTurn {
  const last = turn.blocks[turn.blocks.length - 1];
  return {
    ...turn,
    blocks:
      last?.kind === "tools"
        ? [...turn.blocks.slice(0, -1), { ...last, calls: [...last.calls, call] }]
        : [...turn.blocks, { kind: "tools", id: blockId(), calls: [call] }],
  };
}

export function patchCall(
  turn: AssistantTurn,
  id: string,
  patch: Partial<ToolCall>
): AssistantTurn {
  return {
    ...turn,
    blocks: turn.blocks.map((block) =>
      block.kind === "tools" && block.calls.some((call) => call.id === id)
        ? {
            ...block,
            calls: block.calls.map((call) => (call.id === id ? { ...call, ...patch } : call)),
          }
        : block
    ),
  };
}

/** Upsert one sub-agent's state into the `agents` block for its call, creating
 *  the block on the first event so the swarm shows up as soon as it starts. */
export function upsertAgentState(turn: AssistantTurn, st: AgentTaskState): AssistantTurn {
  const i = turn.blocks.findIndex((b) => b.kind === "agents" && b.callId === st.callId);
  if (i === -1) {
    return {
      ...turn,
      blocks: [...turn.blocks, { kind: "agents", id: blockId(), callId: st.callId, tasks: [st] }],
    };
  }
  const block = turn.blocks[i];
  if (block.kind !== "agents") return turn;
  // A batch can run the same agent several times with different tasks, so the
  // task text is part of the identity.
  const j = block.tasks.findIndex((t) => t.agent === st.agent && t.task === st.task);
  const tasks =
    j === -1
      ? [...block.tasks, st]
      : block.tasks.map((t, k) =>
          k === j ? { ...st, log: t.log, startedAt: t.startedAt ?? st.startedAt } : t
        );
  return {
    ...turn,
    blocks: [...turn.blocks.slice(0, i), { ...block, tasks }, ...turn.blocks.slice(i + 1)],
  };
}

/** Append one live activity line to the matching sub-agent's rolling feed. */
export function appendAgentActivity(
  turn: AssistantTurn,
  callId: string,
  agent: string,
  task: string | undefined,
  line: string
): AssistantTurn {
  return {
    ...turn,
    blocks: turn.blocks.map((block) =>
      block.kind === "agents" && block.callId === callId
        ? {
            ...block,
            tasks: block.tasks.map((t) =>
              t.agent === agent && t.task === task
                ? { ...t, log: [...(t.log ?? []), line].slice(-50) }
                : t
            ),
          }
        : block
    ),
  };
}

/** Close out anything still spinning. The host owns the run state, so whenever it
 *  reports idle every unfinished turn is over, whether it was cancelled, failed,
 *  or the CLI died without a terminal event. */
export function stopUnfinished(turns: Turn[]): Turn[] {
  return turns.map((turn) => {
    if (turn.role === "assistant" && turn.pending) {
      return {
        ...turn,
        pending: false,
        approval: undefined,
        question: undefined,
        stopped: true,
        blocks: turn.blocks.map((block) =>
          block.kind === "tools"
            ? {
                ...block,
                calls: block.calls.map((call) =>
                  call.result === undefined ? { ...call, stopped: true } : call
                ),
              }
            : block.kind === "agents"
              ? {
                  ...block,
                  tasks: block.tasks.map((t) =>
                    t.status === "running" ? { ...t, status: "error" as const, error: "stopped" } : t
                  ),
                }
              : block.kind === "goal" && block.outcome === undefined
                ? { ...block, outcome: "stopped" as const }
                : block
        ),
      };
    }
    if (turn.role === "review" && turn.data.status === "running") {
      return { ...turn, data: { ...turn.data, status: "stopped" } };
    }
    if (turn.role === "info" && turn.pending) {
      return { ...turn, pending: false, note: turn.note ?? "Stopped", error: true };
    }
    return turn;
  });
}

const HISTORY_LIMIT = 12;

/** The chat history sent to `aster chat`, from the last compaction onwards.
 *  Review turns are flattened into an assistant message describing their
 *  findings, so "why is finding 2 critical?" has the findings in context. */
export function buildMessages(turns: Turn[], limit = HISTORY_LIMIT): ChatMessage[] {
  const seam = turns.reduce((at, turn, i) => (turn.role === "compaction" ? i : at), -1);
  const folded = seam === -1 ? [] : (turns[seam] as CompactionTurn).messages;
  const messages: ChatMessage[] = [];
  for (const turn of turns.slice(seam + 1)) {
    if (turn.role === "user") {
      messages.push({ role: "user", content: turn.text });
    } else if (turn.role === "assistant" && turn.text && !turn.pending && !turn.error) {
      // Mid-turn injections steered the reply, so they read best just before it.
      for (const block of turn.blocks) {
        if (block.kind === "injected") {
          messages.push({ role: "user", content: block.text });
        }
      }
      messages.push({ role: "assistant", content: turn.text });
    } else if (turn.role === "review") {
      messages.push({ role: "assistant", content: reviewContext(turn.data) });
    }
  }
  return [...folded, ...(limit === Infinity ? messages : messages.slice(-limit))];
}

function reviewContext(data: ReviewData): string {
  if (data.status === "error") {
    return `A code review was attempted but failed: ${data.errorMsg ?? "unknown error"}.`;
  }
  if (data.status === "running") {
    return "A code review is currently running.";
  }
  if (data.status === "stopped") {
    return "A code review was started but stopped before it finished.";
  }
  if (data.findings.length === 0) {
    return "I ran a code review and found no issues; the diff is clean.";
  }
  const lines = data.findings.slice(0, 25).map((f, i) => {
    const confidence = f.confidence != null ? ` [confidence ${f.confidence.toFixed(2)}]` : "";
    return `${i + 1}. ${f.severity.toUpperCase()} — ${f.title} (${f.file_path}:${f.line})${confidence}: ${f.description}`;
  });
  return `Findings from the code review I ran (${data.findings.length} total):\n${data.summary}\n${lines.join("\n")}`;
}
