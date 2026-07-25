import { highlight } from "sugar-high";

// Highlighting the same source repeatedly (every typing-animation frame, every
// diff re-render) is wasteful, so memo by source string.
const cache = new Map<string, string>();

export function highlightCached(code: string): string {
  let html = cache.get(code);
  if (html === undefined) {
    html = highlight(code);
    cache.set(code, html);
  }
  return html;
}
