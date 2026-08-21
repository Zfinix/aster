import { useEffect, useState } from "react";

const MOD = typeof navigator !== "undefined" && /Mac/.test(navigator.userAgent) ? "cmd" : "ctrl";

/** Openers for the empty panel. The panel is a review tool and a chat, so the
 *  line that greets you should not insist it is only one of them. */
export const OPENERS = [
  "What should we review next?",
  "What are we looking at?",
  "What's on the branch?",
  "Where should I start?",
  "What needs a second read?",
  "What are we building?",
  "What changed?",
  "What do you want to dig into?",
  "What's the task?",
  "Ready when you are.",
];

/** One-line reminders of what the panel can do; none of it is on screen until
 *  you go looking, so the empty state is where it gets said. Anything bracketed
 *  is drawn as a chip: a key to press or a name to type. */
export const TIPS = [
  "[@] mentions a file from this repo",
  "[/review] reads the working tree",
  "[/review-pr] reviews a GitHub PR",
  "[/diff] shows uncommitted changes",
  "[/compact] folds earlier turns down",
  "[/model] switches models mid-chat",
  "[/effort] sets the reasoning budget",
  "[/provider] switches the endpoint",
  "[/memory] lists what Aster remembers",
  "[/status] shows model and token usage",
  "[/mcp] turns MCP servers on and off",
  "[/mode] sets what the agent may do",
  "Skills show up as their own [/name]",
  "[alt] [a] mentions the editor selection",
  `[${MOD}] [alt] [k] opens the command menu`,
  `[${MOD}] [alt] [n] starts a conversation`,
  `[${MOD}] [alt] [r] reopens the last one`,
  "[shift] [enter] adds a newline",
  "Drop a file or paste a screenshot in",
  "Fix hands a finding back for a patch",
];

export interface Part {
  text: string;
  /** Drawn as a chip rather than prose. */
  chip: boolean;
}

/** Splits a tip on its `[bracketed]` runs, keeping the prose between them. */
export function parts(tip: string): Part[] {
  const out: Part[] = [];
  let at = 0;
  for (const match of tip.matchAll(/\[([^\]]+)\]/g)) {
    const start = match.index ?? 0;
    if (start > at) out.push({ text: tip.slice(at, start), chip: false });
    out.push({ text: match[1], chip: true });
    at = start + match[0].length;
  }
  if (at < tip.length) out.push({ text: tip.slice(at), chip: false });
  return out;
}

/** Wraps both ways, so a step past either end lands back in the list. */
export function itemAt<T>(items: readonly T[], index: number): T {
  return items[((index % items.length) + items.length) % items.length];
}

/**
 * Holds a line from `items`, stepping on after `everyMs` when that is set. The
 * start is random so launches differ; the order after it is not, so a rotation
 * never repeats itself before the list is out.
 */
export function useRotation(items: readonly string[], everyMs = 0): string {
  const [at, setAt] = useState(() => Math.floor(Math.random() * items.length));
  useEffect(() => {
    if (!everyMs || stillness()) return;
    const timer = setInterval(() => setAt((i) => i + 1), everyMs);
    return () => clearInterval(timer);
  }, [everyMs]);
  return itemAt(items, at);
}

const stillness = () =>
  typeof window !== "undefined" &&
  (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false);
