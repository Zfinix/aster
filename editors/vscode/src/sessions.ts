import { runCli } from "./asterCli";
import {
  SessionSummary,
  TranscriptToolCall,
  TranscriptTurn,
} from "./protocol";

interface RawToolCall {
  id?: string;
  function?: { name?: string; arguments?: string };
}

interface RawEvent {
  type: string;
  role?: string;
  content?: string;
  tool_calls?: RawToolCall[];
  tool_call_id?: string;
  reasoning?: { text?: string; tokens?: number; duration_ms?: number };
}

/** Saved sessions for this repo, newest first. */
export async function listSessions(cwd: string): Promise<SessionSummary[]> {
  try {
    const { stdout, code } = await runCli(["sessions", "list", "--json"], cwd);
    if (code !== 0) {
      return [];
    }
    // The CLI already sorts newest first; reversing here put the oldest on top.
    return JSON.parse(stdout) as SessionSummary[];
  } catch {
    return [];
  }
}

/** Remove a saved session. Errors are the caller's to report. */
export async function deleteSession(cwd: string, id: string): Promise<void> {
  const { code, stderr } = await runCli(["sessions", "delete", id, "--json"], cwd);
  if (code !== 0) {
    throw new Error(stderr.trim() || `could not delete session ${id}`);
  }
}

/** Give a saved session a new name. */
export async function renameSession(cwd: string, id: string, title: string): Promise<void> {
  const { code, stderr } = await runCli(["sessions", "rename", id, title, "--json"], cwd);
  if (code !== 0) {
    throw new Error(stderr.trim() || `could not rename session ${id}`);
  }
}

/**
 * A session's transcript, rebuilt into the turns the thread renders. Assistant
 * turns keep their thinking and tool calls: replaying only `content` dropped
 * every reasoning block, every tool step, and any turn that was tool calls
 * alone, which is most of a working session.
 */
export async function loadSession(cwd: string, id: string): Promise<TranscriptTurn[]> {
  const { stdout, code } = await runCli(["sessions", "show", id, "--json"], cwd);
  if (code !== 0) {
    throw new Error(`could not load session ${id}`);
  }
  const parsed = JSON.parse(stdout) as { events?: RawEvent[] };
  const events = (parsed.events ?? []).filter((e) => e.type === "message");
  // Results arrive as their own `tool` events after the call, so index them
  // first and let each assistant turn pick up its own.
  const results = new Map<string, string>();
  for (const event of events) {
    if (event.role === "tool" && event.tool_call_id) {
      results.set(event.tool_call_id, event.content ?? "");
    }
  }

  const turns: TranscriptTurn[] = [];
  for (const event of events) {
    if (event.role === "user") {
      const content = event.content ?? "";
      // Harness steering is recorded as `system` and never lands here, but an
      // empty user turn is still nothing to draw.
      if (content.trim()) {
        turns.push({ role: "user", content });
      }
      continue;
    }
    if (event.role !== "assistant") {
      continue;
    }
    const toolCalls = (event.tool_calls ?? []).map(toTranscriptCall(results));
    const reasoning = event.reasoning?.text?.trim()
      ? {
          text: event.reasoning.text,
          tokens: event.reasoning.tokens,
          durationMs: event.reasoning.duration_ms,
        }
      : undefined;
    const content = event.content ?? "";
    // A round can be tool calls with no commentary, or thinking alone. Only a
    // turn carrying none of the three has nothing to show.
    if (!content.trim() && toolCalls.length === 0 && !reasoning) {
      continue;
    }
    turns.push({ role: "assistant", content, reasoning, toolCalls });
  }
  return turns;
}

function toTranscriptCall(results: Map<string, string>) {
  return (call: RawToolCall, index: number): TranscriptToolCall => {
    const id = call.id ?? `restored-${index}`;
    const result = results.get(id);
    return {
      id,
      name: call.function?.name ?? "unknown",
      arguments: call.function?.arguments ?? "{}",
      result,
      error: result?.startsWith("error: ") || undefined,
    };
  };
}
