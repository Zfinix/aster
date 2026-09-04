import { useEffect } from "react";
import type { McpServer } from "../../src/protocol";
import { post } from "../lib/host";
import { redactSecrets } from "../lib/redact";

/**
 * The `/mcp` control panel: every configured server with its state, where
 * picking one flips `disabled` in whichever config file declares it. It stays
 * open across toggles, since turning two servers off is one errand.
 */
export function McpPicker({
  servers,
  onToggle,
}: {
  servers: McpServer[];
  onToggle: (name: string, disabled: boolean) => void;
}) {
  // The config can change under us between openings, so the list is re-read
  // rather than cached from whenever the panel last loaded.
  useEffect(() => {
    post({ type: "listMcp" });
  }, []);

  return (
    <div className="picker" role="dialog" aria-label="MCP servers">
      <div className="picker-head">MCP servers</div>
      {servers.length === 0 && (
        <div className="picker-empty">
          No servers configured. Add them to .mcp.json or `mcp:` in aster.yaml, or run `aster mcp
          import`.
        </div>
      )}
      {servers.map((server) => (
        <button
          key={server.name}
          className="picker-row"
          data-selected={!server.disabled}
          onClick={() => onToggle(server.name, !server.disabled)}
        >
          <span className="picker-mark">{server.disabled ? "◻" : "◼"}</span>
          <span className="picker-body">
            <span className="picker-label">{server.name}</span>
            <span className="picker-detail">
              {server.disabled ? "disabled" : "enabled"} · {describe(server)}
            </span>
          </span>
        </button>
      ))}
    </div>
  );
}

function describe(server: McpServer): string {
  if (server.url) return redactSecrets(server.url);
  const command = [server.command, ...server.args].join(" ").trim();
  return command ? redactSecrets(command) : (server.transport ?? "unconfigured");
}
