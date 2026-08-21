import { useEffect, useState } from "react";

/** Openers for a review: the home screen asks what to look at, not always in
 *  the same words. */
export const REVIEW_OPENERS = [
  "What should we review next?",
  "What are we looking at?",
  "What's on the branch?",
  "What needs a second read?",
  "What changed?",
  "Where should I start?",
];

/** Openers for a chat, which is the other half of the same box. */
export const CHAT_OPENERS = [
  "What are we building?",
  "What's the task?",
  "What do you want to dig into?",
  "Where are we picking up?",
  "What are we working on?",
  "Ready when you are.",
];

/** One-line reminders of what the app can do; most of it lives behind a menu,
 *  so the home screen is where it gets said. Anything bracketed is drawn as a
 *  chip: a key to press, or a control to go and find. */
export const TIPS = [
  "[@] picks a file from the repo",
  "The mode menu switches chat and review",
  "The plus menu attaches a diff file",
  "[shift] [enter] adds a newline",
  "Recent conversations live in the sidebar",
  "Settings runs [ast-grep] and [semgrep] too",
  "The confidence gate drops unsure findings",
  "The model menu takes a name of your own",
  "A base URL aims Aster at another endpoint",
  "A finding opens on click, then jumps to the diff",
  "Light and dark live in Settings",
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
  window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
