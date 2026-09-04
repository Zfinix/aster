import type { Effort } from "../../src/protocol";

export const EFFORTS: (Effort | "")[] = ["", "off", "low", "medium", "high", "xhigh", "max", "ultra"];

/** "medium" -> "Med", so the chip and the menu row stay one short word. */
export function effortShort(effort: string | null): string {
  if (!effort) return "Default";
  const short: Record<string, string> = {
    off: "Off",
    low: "Low",
    medium: "Med",
    high: "High",
    xhigh: "XHigh",
    max: "Max",
    ultra: "Ultra",
  };
  return short[effort] ?? effort.charAt(0).toUpperCase() + effort.slice(1);
}

/** The same ladder as the command menu's choice control wants it. */
export const EFFORT_OPTIONS = EFFORTS.map((value) => ({
  value,
  label: effortShort(value || null),
}));
