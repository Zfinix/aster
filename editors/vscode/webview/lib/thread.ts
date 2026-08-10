import type { Finding, UsageSummary } from "../../src/types";
import type { ChatMessage, InfoRow } from "../../src/protocol";

export interface ToolCall {
  id: string;
  name: string;
  arguments: string;
  result?: string;
  error?: boolean;
}

export interface ReviewData {
  status: "running" | "done" | "error" | "stopped";
  phase: string;
  findings: Finding[];
  refuted: { title: string; reason: string }[];
  summary: string;
  usage?: UsageSummary;
  errorMsg?: string;
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
}

/**
 * One chronological slice of a turn. Text and tool groups are kept in arrival
 * order so steps render under the thought that produced them, the way the CLI
 * prints them, instead of piling up above the whole reply.
 */
export type TurnBlock =
  | { kind: "text"; id: string; text: string }
  | { kind: "tools"; id: string; calls: ToolCall[] }
  /** Live sub-agent swarm for one `agent` tool call. */
  | { kind: "agents"; id: string; callId: string; tasks: AgentTaskState[] }
  /** A user message the turn absorbed mid-run, shown where it landed. */
  | { kind: "injected"; id: string; text: string };

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

/** An open `ask_user` prompt: a headline, the question, and the offered answers. */
export interface Question {
  header: string;
  question: string;
  options: string[];
}

export interface AssistantTurn {
  id: string;
  role: "assistant";
  /** The whole reply, flattened: chat history, copy, and error display use it. */
  text: string;
  blocks: TurnBlock[];
  /** Set while the CLI blocks waiting for an approval reply. */
  approval?: string;
  /** Set while `ask_user` blocks waiting for a choice. */
  question?: Question;
  /** True once any token delta arrived, so `text` is already the full answer. */
  streamed?: boolean;
  pending?: boolean;
  error?: boolean;
  /** Kept apart from `text` so a failure never erases the output that preceded it. */
  errorMsg?: string;
  /** Cancelled by the user, or abandoned because the host went idle mid-turn. */
  stopped?: boolean;
  edits?: string[];
  usage?: UsageSummary;
}

/**
 * A local answer that never went to the model: `/status`, `/memory`, `/diff`,
 * and what a compaction folded away. It sits in the thread where it was asked
 * for, and stays out of the history the next turn sends.
 */
export interface InfoTurn {
  id: string;
  role: "info";
  title: string;
  rows?: InfoRow[];
  body?: string;
  lang?: string;
  note?: string;
  error?: boolean;
  pending?: boolean;
}

export type Turn =
  | { id: string; role: "user"; text: string }
  | AssistantTurn
  | InfoTurn
  | { id: string; role: "review"; data: ReviewData };

export function emptyReview(): ReviewData {
  return { status: "running", phase: "Starting", findings: [], refuted: [], summary: "" };
}

let blocks = 0;
const blockId = () => `b${++blocks}`;

/**
 * Webview state persisted before turns kept a block timeline has `text` and a
 * flat `tools` list instead. Fold it into one text block so a panel restored
 * across an extension update still renders.
 */
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

/**
 * `separator` joins the chunk onto whatever came before in the flattened text.
 * A chunk that opens a new block never carries it, so the block itself never
 * starts with blank lines.
 */
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

export function appendInjected(turn: AssistantTurn, text: string): AssistantTurn {
  return { ...turn, blocks: [...turn.blocks, { kind: "injected", id: blockId(), text }] };
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
  const j = block.tasks.findIndex((t) => t.agent === st.agent);
  const tasks = j === -1 ? [...block.tasks, st] : block.tasks.map((t, k) => (k === j ? st : t));
  return {
    ...turn,
    blocks: [...turn.blocks.slice(0, i), { ...block, tasks }, ...turn.blocks.slice(i + 1)],
  };
}

/**
 * Close out anything still spinning. The host owns the run state, so whenever it
 * reports idle every unfinished turn is over, whether it was cancelled, failed,
 * or the CLI died without a terminal event.
 */
export function stopUnfinished(turns: Turn[]): Turn[] {
  return turns.map((turn) => {
    if (turn.role === "assistant" && turn.pending) {
      return {
        ...turn,
        pending: false,
        approval: undefined,
        question: undefined,
        stopped: true,
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

/** Turns older than this are dropped from what a turn sends; `/compact` folds
 *  them into a summary instead, and passes `Infinity` to see all of them. */
const HISTORY_LIMIT = 12;

/**
 * The chat history sent to `aster chat`. Review turns are flattened into an
 * assistant message describing their findings, so follow-up questions like
 * "why is finding 2 critical?" have the findings in context.
 */
export function buildMessages(turns: Turn[], limit = HISTORY_LIMIT): ChatMessage[] {
  const messages: ChatMessage[] = [];
  for (const turn of turns) {
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
  return limit === Infinity ? messages : messages.slice(-limit);
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
