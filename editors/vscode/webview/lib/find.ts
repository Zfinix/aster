const ALL = "aster-find";
const CURRENT = "aster-find-current";

/** Case-insensitive, non-overlapping match starts of `query` in `text`. */
export function matchOffsets(text: string, query: string): number[] {
  if (!query) return [];
  let hay = text.toLowerCase();
  let needle = query.toLowerCase();
  // A case fold that changes the length would misplace every offset after it.
  if (hay.length !== text.length || needle.length !== query.length) {
    hay = text;
    needle = query;
  }
  const out: number[] = [];
  for (let at = hay.indexOf(needle); at >= 0; at = hay.indexOf(needle, at + needle.length)) {
    out.push(at);
  }
  return out;
}

/** Every match under `root`, one range per hit, in document order. */
export function collectMatches(root: Node, query: string): Range[] {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const ranges: Range[] = [];
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    for (const at of matchOffsets(node.textContent ?? "", query)) {
      const range = document.createRange();
      range.setStart(node, at);
      range.setEnd(node, at + query.length);
      ranges.push(range);
    }
  }
  return ranges;
}

function registry(): HighlightRegistry | undefined {
  return "highlights" in CSS && typeof Highlight === "function" ? CSS.highlights : undefined;
}

/** Paint the matches and bring the current one into view. Without the
 *  highlight API the current match is selected instead, so it still shows. */
export function showMatches(ranges: Range[], current: number): void {
  const highlights = registry();
  const now = ranges[current];
  if (highlights) {
    highlights.set(ALL, new Highlight(...ranges.filter((_, i) => i !== current)));
    highlights.set(CURRENT, new Highlight(...(now ? [now] : [])));
  } else {
    const selection = window.getSelection();
    selection?.removeAllRanges();
    if (now) selection?.addRange(now);
  }
  now?.startContainer.parentElement?.scrollIntoView({ block: "center" });
}

export function clearMatches(): void {
  const highlights = registry();
  if (highlights) {
    highlights.delete(ALL);
    highlights.delete(CURRENT);
  } else {
    window.getSelection()?.removeAllRanges();
  }
}
