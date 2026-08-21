import type { McpServer } from "../../src/protocol";
import { redactSecrets } from "../lib/redact";
import { Toggle } from "./controls/Toggle";

/** The servers themselves, which `aster mcp` owns rather than `aster.yaml`, so
 *  they sit below the MCP keys instead of among them. */
export function McpSection({
  servers,
  onToggle,
}: {
  servers: McpServer[];
  onToggle: (name: string, disabled: boolean) => void;
}) {
  return (
    <>
      <h2 className="set-group-title">Servers</h2>
      <div className="set-card">
        {servers.length === 0 ? (
          <div className="set-row">
            <div className="set-row-text">
              <p className="set-row-help">
                No MCP servers configured. <code>aster mcp import</code> copies them from another
                coding tool.
              </p>
            </div>
          </div>
        ) : (
          servers.map((server) => (
            <div key={server.name} className="set-row">
              <div className="set-row-text">
                <div className="set-row-head">
                  <span className="set-row-label">{server.name}</span>
                </div>
                <p className="set-row-key mono">{describe(server)}</p>
              </div>
              <div className="set-row-control">
                <Toggle
                  checked={!server.disabled}
                  label={`Enable ${server.name}`}
                  onChange={(enabled) => onToggle(server.name, !enabled)}
                />
              </div>
            </div>
          ))
        )}
      </div>
    </>
  );
}

function describe(server: McpServer): string {
  if (server.url) return redactSecrets(server.url);
  return redactSecrets([server.command, ...server.args].filter(Boolean).join(" "));
}
