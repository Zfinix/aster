import type { SessionSummary } from "../../src/protocol";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** How long ago a session started, short enough to sit at the end of a row.
 *  Anything older than a week is a date, since "37d" is not how people look
 *  for a conversation. */
export function relativeTime(iso: string, now = Date.now()): string {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) {
    return "";
  }
  const ago = now - then;
  if (ago < MINUTE) return "now";
  if (ago < HOUR) return `${Math.floor(ago / MINUTE)}m`;
  if (ago < DAY) return `${Math.floor(ago / HOUR)}h`;
  if (ago < 7 * DAY) return `${Math.floor(ago / DAY)}d`;
  return new Date(then).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });
}

function matches(session: SessionSummary, needle: string): boolean {
  return [session.title, session.model ?? "", session.id]
    .join(" ")
    .toLowerCase()
    .includes(needle);
}

/** The sessions a query leaves, in the CLI's own newest-first order. */
export function filterSessions(sessions: SessionSummary[], query = ""): SessionSummary[] {
  const needle = query.trim().toLowerCase();
  return needle ? sessions.filter((s) => matches(s, needle)) : sessions;
}
