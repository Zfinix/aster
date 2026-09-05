export interface Trigger {
  query: string;
  start: number;
  end: number;
}

/** `@` opens the file list, `/` the command menu; only one can hold the caret. */
export interface Triggers {
  mention: Trigger | null;
  command: Trigger | null;
}

const MENTION = /(?:^|\s)@[^\s]*/g;
const COMMAND = /(?:^|\s)\/[^\s/]*/g;

export function triggersAt(text: string, caret: number): Triggers {
  return {
    mention: tokenAt(text, caret, MENTION, "@"),
    command: tokenAt(text, caret, COMMAND, "/"),
  };
}

function tokenAt(text: string, caret: number, pattern: RegExp, sigil: string): Trigger | null {
  pattern.lastIndex = 0;
  for (const match of text.matchAll(pattern)) {
    if (match.index === undefined) continue;
    const start = text.indexOf(sigil, match.index);
    const end = match.index + match[0].length;
    if (caret >= start && caret <= end) {
      return { query: text.slice(start + 1, end), start, end };
    }
  }
  return null;
}

/** Replace the trigger's token with `value`, leaving one space after it. */
export function applyTrigger(text: string, trigger: Trigger, value: string): string {
  const rest = text.slice(trigger.end);
  return `${text.slice(0, trigger.start)}${value}${rest.startsWith(" ") ? "" : " "}${rest}`;
}

/** Take the token out, for a command that runs instead of completing. */
export function dropTrigger(text: string, trigger: Trigger): string {
  return `${text.slice(0, trigger.start)}${text.slice(trigger.end)}`;
}
