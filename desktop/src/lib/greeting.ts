import { useEffect, useState } from "react";

export const REVIEW_OPENERS = [
  "What should we review next?",
  "What are we looking at?",
  "What's on the branch?",
  "What needs a second read?",
  "What changed?",
  "Where should I start?",
  "Anything blocking merge?",
  "Is this ready for production?",
  "Are there breaking changes?",
  "Have tests been updated?",
  "What was the feedback last time?",
  "Any refactoring worth calling out?",
  "Where are the biggest risks?",
  "Did anything surprise you?",
  "What would you do differently?",
  "Is the commit message clear?",
  "Who should approve this?",
  "How does this impact users?",
  "Were performance considerations made?",
  "Do we need to update documentation?",
  "How will we verify this in staging?",
  "Does this follow our conventions?",
  "Any TODOs we should address?",
  "What is the main goal of this diff?",
  "Is there legacy code touched here?",
  "What assumptions were made?",
  "What's the testing strategy?",
  "Are there edge cases to consider?",
  "Any third-party dependencies introduced?",
  "Can we simplify anything further?",
  "Are error states handled properly?",
];

export const CHAT_OPENERS = [
  "What are we building?",
  "What's the task?",
  "What do you want to dig into?",
  "Where are we picking up?",
  "What are we working on?",
  "Ready when you are.",
  "What’s on your mind today?",
  "What problem are we solving?",
  "What's our top priority?",
  "Any blockers I can help with?",
  "What’s the latest update?",
  "Anything unclear from the ticket?",
  "Where should we focus first?",
  "Is there context I should know?",
  "What’s the deadline?",
  "What are your thoughts on the approach?",
  "Should we pair on this part?",
  "How are you feeling about the project?",
  "Any decisions made recently?",
  "What needs breaking down?",
  "Can we ship a first version today?",
  "Anything you'd like to demo?",
  "What’s the next smallest step?",
  "Have you tried running it locally?",
  "Should we create a checklist?",
  "Is there a design to refer to?",
  "Are there open questions on this?",
  "Where do we start?",
  "Can I review anything for you?",
  "What experiments can we run?",
];
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

/** Holds a line from `items`, stepping on after `everyMs` when that is set. The
 *  start is random so launches differ; the order after it is not, so a rotation
 *  never repeats itself before the list is out. */
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
