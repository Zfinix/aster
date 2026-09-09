import type { MemoryBlock, MemoryProject } from "../../src/protocol";
import { relativeTime } from "./history";

/** The name project memory goes by in the UI: the file itself. */
export const PROJECT = "ASTER.md";

export function filterBlocks(blocks: MemoryBlock[], query: string): MemoryBlock[] {
  const needle = query.trim().toLowerCase();
  if (!needle) {
    return blocks;
  }
  return blocks.filter((b) => `${b.name} ${b.description}`.toLowerCase().includes(needle));
}

/** When a block was last written, falling back to when it was first saved. */
export function blockWhen(block: MemoryBlock): string {
  const stamp = block.updated_at ?? block.created_at;
  return stamp ? relativeTime(stamp) : "";
}

/** Project memory is a bullet list, so its bullets are the facts in it. */
export function countFacts(project: MemoryProject | null): number {
  if (!project) {
    return 0;
  }
  return project.text.split("\n").filter((line) => line.trimStart().startsWith("-")).length;
}

export function plural(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? "" : "s"}`;
}
