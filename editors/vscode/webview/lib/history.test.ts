import { describe, expect, it } from "vitest";
import { filterSessions, relativeTime } from "./history";
import type { SessionSummary } from "../../src/protocol";

const NOW = Date.parse("2026-08-07T12:00:00Z");
const ago = (ms: number) => new Date(NOW - ms).toISOString();
const HOUR = 3_600_000;
const DAY = 24 * HOUR;

const session = (over: Partial<SessionSummary> = {}): SessionSummary => ({
  id: "01J",
  created_at: ago(HOUR),
  model: "anthropic/claude-opus-4",
  turns: 3,
  title: "Fix sandbox seccomp filter",
  ...over,
});

describe("relativeTime", () => {
  it("coarsens as the session gets older", () => {
    expect(relativeTime(ago(30_000), NOW)).toBe("now");
    expect(relativeTime(ago(5 * 60_000), NOW)).toBe("5m");
    expect(relativeTime(ago(3 * HOUR), NOW)).toBe("3h");
    expect(relativeTime(ago(2 * DAY), NOW)).toBe("2d");
  });

  it("falls back to a date past a week", () => {
    expect(relativeTime(ago(40 * DAY), NOW)).toMatch(/\w+ \d+/);
  });

  it("is empty for an unparseable timestamp", () => {
    expect(relativeTime("not a date", NOW)).toBe("");
  });
});

describe("filterSessions", () => {
  it("keeps the incoming order", () => {
    const rows = filterSessions([
      session({ id: "a", created_at: ago(HOUR) }),
      session({ id: "b", created_at: ago(30 * HOUR) }),
      session({ id: "c", created_at: ago(60 * DAY) }),
    ]);
    expect(rows.map((s) => s.id)).toEqual(["a", "b", "c"]);
  });

  it("filters on title, model, and id", () => {
    const sessions = [
      session({ id: "aaa", title: "Fix sandbox seccomp filter" }),
      session({ id: "bbb", title: "Rename chat sessions", model: "openai/gpt-5" }),
    ];
    expect(filterSessions(sessions, "seccomp")).toHaveLength(1);
    expect(filterSessions(sessions, "gpt-5")[0].id).toBe("bbb");
    expect(filterSessions(sessions, "aaa")[0].id).toBe("aaa");
    expect(filterSessions(sessions, "nothing here")).toEqual([]);
  });

  it("ignores case and surrounding space in the query", () => {
    expect(filterSessions([session()], "  SECCOMP ")).toHaveLength(1);
  });
});
