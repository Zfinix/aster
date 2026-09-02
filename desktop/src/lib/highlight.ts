import { highlight } from "sugar-high";

const cache = new Map<string, string>();

export function highlightCached(code: string): string {
  let html = cache.get(code);
  if (html === undefined) {
    html = highlight(code);
    cache.set(code, html);
  }
  return html;
}
