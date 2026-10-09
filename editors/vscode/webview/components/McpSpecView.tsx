import type { McpSpec } from "../lib/tools";

/** What a tool inspection found: what the tool does and the inputs it takes.
 *  The raw payload is a JSON schema the model reads; the row header already
 *  names the tool, so this shows only the summary and an aligned input list. */
export function McpSpecView({ spec }: { spec: McpSpec }) {
  return (
    <div className="mcp-spec">
      {spec.about && <p className="mcp-spec-about">{spec.about}</p>}
      {spec.params.length > 0 && (
        <dl className="mcp-params">
          {spec.params.map((param) => (
            <div key={param.name} className="mcp-param">
              <dt className="mcp-param-name">{param.name}</dt>
              <dd className="mcp-param-detail">
                <span className="mcp-param-type">{param.type}</span>
                {param.required && <span className="mcp-param-required">required</span>}
                {param.description && (
                  <span className="mcp-param-about">{param.description}</span>
                )}
              </dd>
            </div>
          ))}
        </dl>
      )}
    </div>
  );
}
