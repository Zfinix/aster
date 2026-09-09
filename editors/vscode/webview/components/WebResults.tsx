import type { WebResult } from "../lib/tools";

import { Link } from "./Link";

/** What a web search found, one row per hit. The raw payload is a JSON array
 *  the model reads; a reader wants the title as the link, the URL dimmed
 *  beneath, and the snippet filling in the story. */
export function WebResults({ results }: { results: WebResult[] }) {
  return (
    <ul className="web-results">
      {results.map((r, i) => (
        <li key={`${r.url}-${i}`} className="web-result">
          <Link url={r.url}>{r.title}</Link>
          <span className="web-result-url">{r.url}</span>
          {r.snippet && <span className="web-result-snippet">{r.snippet}</span>}
        </li>
      ))}
    </ul>
  );
}