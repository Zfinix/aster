import { runCli } from "./asterCli";
import { SessionSummary, TranscriptTurn } from "./protocol";

interface RawEvent {
  type: string;
  role?: string;
  content?: string;
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

/**
 * A session's transcript, flattened to user/assistant turns. Tool and session
 * envelope events are dropped: the UI only replays the conversation.
 */
export async function loadSession(cwd: string, id: string): Promise<TranscriptTurn[]> {
  const { stdout, code } = await runCli(["sessions", "show", id, "--json"], cwd);
  if (code !== 0) {
    throw new Error(`could not load session ${id}`);
  }
  const parsed = JSON.parse(stdout) as { events?: RawEvent[] };
  return (parsed.events ?? [])
    .filter(
      (e): e is RawEvent & { role: string; content: string } =>
        e.type === "message" &&
        (e.role === "user" || e.role === "assistant") &&
        typeof e.content === "string" &&
        e.content.trim().length > 0
    )
    .map((e) => ({ role: e.role as "user" | "assistant", content: e.content }));
}
