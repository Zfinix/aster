import { describe, expect, it } from "vitest";
import { groupSessions, relativeTime } from "./history";
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
    expect(relativeTime(ago(30_000), NOW)).toBe("just now");
    expect(relativeTime(ago(5 * 60_000), NOW)).toBe("5m ago");
    expect(relativeTime(ago(3 * HOUR), NOW)).toBe("3h ago");
    expect(relativeTime(ago(2 * DAY), NOW)).toBe("2d ago");
  });

  it("falls back to a date past a week", () => {
    expect(relativeTime(ago(40 * DAY), NOW)).toMatch(/\w+ \d+/);
  });

  it("is empty for an unparseable timestamp", () => {
    expect(relativeTime("not a date", NOW)).toBe("");
  });
});

describe("groupSessions", () => {
  it("buckets by age and keeps the incoming order", () => {
    const groups = groupSessions(
      [
        session({ id: "a", created_at: ago(HOUR) }),
        session({ id: "b", created_at: ago(2 * HOUR) }),
        session({ id: "c", created_at: ago(30 * HOUR) }),
        session({ id: "d", created_at: ago(60 * DAY) }),
      ],
      "",
      NOW
    );
    expect(groups.map((g) => g.label)).toEqual(["Today", "Yesterday", "Earlier"]);
    expect(groups[0].sessions.map((s) => s.id)).toEqual(["a", "b"]);
  });

  it("never repeats a heading for one run of sessions", () => {
    const groups = groupSessions(
      [
        session({ id: "a", created_at: ago(HOUR) }),
        session({ id: "b", created_at: ago(2 * HOUR) }),
        session({ id: "c", created_at: ago(3 * HOUR) }),
      ],
      "",
      NOW
    );
    expect(groups).toHaveLength(1);
  });

  it("filters on title, model, and id", () => {
    const sessions = [
      session({ id: "aaa", title: "Fix sandbox seccomp filter" }),
      session({ id: "bbb", title: "Rename chat sessions", model: "openai/gpt-5" }),
    ];
    expect(groupSessions(sessions, "seccomp", NOW)[0].sessions).toHaveLength(1);
    expect(groupSessions(sessions, "gpt-5", NOW)[0].sessions[0].id).toBe("bbb");
    expect(groupSessions(sessions, "aaa", NOW)[0].sessions[0].id).toBe("aaa");
    expect(groupSessions(sessions, "nothing here", NOW)).toEqual([]);
  });

  it("ignores case and surrounding space in the query", () => {
    const groups = groupSessions([session()], "  SECCOMP ", NOW);
    expect(groups[0].sessions).toHaveLength(1);
  });
});
