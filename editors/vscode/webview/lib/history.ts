import type { SessionSummary } from "../../src/protocol";

export interface SessionGroup {
  label: string;
  sessions: SessionSummary[];
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/**
 * How long ago a session started, at the coarsest useful precision. Anything
 * older than a week is a date, since "37 days ago" is not how people look for
 * a conversation.
 */
export function relativeTime(iso: string, now = Date.now()): string {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) {
    return "";
  }
  const ago = now - then;
  if (ago < MINUTE) return "just now";
  if (ago < HOUR) return `${Math.floor(ago / MINUTE)}m ago`;
  if (ago < DAY) return `${Math.floor(ago / HOUR)}h ago`;
  if (ago < 7 * DAY) return `${Math.floor(ago / DAY)}d ago`;
  return new Date(then).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });
}

/** Which heading a session files under, from oldest boundary to newest. */
function bucket(iso: string, now: number): string {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) {
    return "Earlier";
  }
  const startOfToday = new Date(now).setHours(0, 0, 0, 0);
  if (then >= startOfToday) return "Today";
  if (then >= startOfToday - DAY) return "Yesterday";
  if (then >= startOfToday - 7 * DAY) return "This week";
  if (then >= startOfToday - 30 * DAY) return "This month";
  return "Earlier";
}

function matches(session: SessionSummary, needle: string): boolean {
  return [session.title, session.model ?? "", session.id]
    .join(" ")
    .toLowerCase()
    .includes(needle);
}

/**
 * Filter by query, then group by age. Input order is preserved inside each
 * group, so the CLI's newest-first ordering carries through.
 */
export function groupSessions(
  sessions: SessionSummary[],
  query = "",
  now = Date.now()
): SessionGroup[] {
  const needle = query.trim().toLowerCase();
  const matched = needle ? sessions.filter((s) => matches(s, needle)) : sessions;

  const groups: SessionGroup[] = [];
  for (const session of matched) {
    const label = bucket(session.created_at, now);
    const last = groups[groups.length - 1];
    if (last?.label === label) {
      last.sessions.push(session);
    } else {
      groups.push({ label, sessions: [session] });
    }
  }
  return groups;
}
