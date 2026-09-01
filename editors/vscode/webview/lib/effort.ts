import type { Effort } from "../../src/protocol";

/** The ladder in menu order; "" is whatever aster.yaml configures. */
export const EFFORTS: (Effort | "")[] = ["", "off", "low", "medium", "high"];

/** "medium" -> "Med", so the chip and the menu row stay one short word. */
export function effortShort(effort: string | null): string {
  if (!effort) return "Default";
  return effort === "medium" ? "Med" : effort.charAt(0).toUpperCase() + effort.slice(1);
}

/** The same ladder as the command menu's choice control wants it. */
export const EFFORT_OPTIONS = EFFORTS.map((value) => ({
  value,
  label: effortShort(value || null),
}));
