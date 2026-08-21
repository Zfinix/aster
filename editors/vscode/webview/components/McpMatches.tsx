import type { McpMatch } from "../lib/tools";

/** What a tool search found, as one chip per tool. The raw payload is JSON the
 *  model reads; a reader wants to see which tools came back and from where. */
export function McpMatches({ matches }: { matches: McpMatch[] }) {
  return (
    <ul className="mcp-chips">
      {matches.map((match) => (
        <li key={match.id} className="mcp-chip" title={match.description || match.id}>
          <span className="mcp-chip-server">{match.server}</span>
          <span className="mcp-chip-name">{match.name}</span>
        </li>
      ))}
    </ul>
  );
}
