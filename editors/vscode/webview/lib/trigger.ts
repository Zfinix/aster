export interface Trigger {
  /** Text between the `@` and the caret. */
  query: string;
  /** Index of the `@` in the input. */
  start: number;
}

/**
 * Detect an active `@` mention at the caret. Commands are not a trigger: they
 * live in the command menu, which `/` opens instead of typing into the box.
 */
export function triggerAt(text: string, caret: number): Trigger | null {
  const at = /(^|\s)@([^\s]*)$/.exec(text.slice(0, caret));
  return at ? { query: at[2], start: caret - at[2].length - 1 } : null;
}

/** Replace the trigger run with `value`, leaving a trailing space. */
export function applyTrigger(text: string, trigger: Trigger, caret: number, value: string): string {
  return `${text.slice(0, trigger.start)}${value} ${text.slice(caret)}`;
}
