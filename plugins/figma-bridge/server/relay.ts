/**
 * figma-bridge relay daemon.
 *
 * Owns ws://127.0.0.1:4395. The Figma plugin connects as /plugin; MCP server
 * instances connect as /server and forward tool calls to the plugin through
 * here. Runs as a detached daemon so several aster sessions can share one
 * plugin connection and the MCP server never hits EADDRINUSE.
 *
 * Start with: bun server/relay.ts (or let server/index.ts spawn it)
 */

const PORT = 4395;
const HOST = "127.0.0.1";

let pluginSocket: any = null;

type Client = { ws: any };
const clients = new Set<Client>();

type Pending = {
  resolve: (value: unknown) => void;
  reject: (err: Error) => void;
  timer: ReturnType<typeof setTimeout>;
};

// Calls the relay itself forwards on behalf of clients (ping).
const pending = new Map<number, Pending>();
let nextCallId = 1;

function rejectAllPending(err: Error) {
  for (const [, p] of pending) {
    clearTimeout(p.timer);
    p.reject(err);
  }
  pending.clear();
}

function send(ws: any, msg: unknown) {
  try {
    ws.send(JSON.stringify(msg));
  } catch {}
}

Bun.serve({
  port: PORT,
  hostname: HOST,
  fetch(req, srv) {
    const url = new URL(req.url);
    if (url.pathname !== "/plugin" && url.pathname !== "/server") {
      return new Response("figma-bridge relay\n", { status: 200 });
    }
    const ok = srv.upgrade(req, { data: { role: url.pathname === "/plugin" ? "plugin" : "server" } });
    if (!ok) return new Response("upgrade failed", { status: 400 });
    return undefined;
  },
  websocket: {
    open(ws: any) {
      const role = ws.data.role;
      if (role === "plugin") {
        // One plugin at a time: a new one replaces the old.
        if (pluginSocket && pluginSocket !== ws) {
          try {
            pluginSocket.close();
          } catch {}
        }
        pluginSocket = ws;
      } else {
        clients.add({ ws });
      }
    },
    async message(ws: any, raw: string | ArrayBuffer) {
      const role = ws.data.role;
      let msg: any;
      try {
        msg = JSON.parse(String(raw));
      } catch {
        return;
      }

      if (role === "plugin") {
        // Result of a forwarded call: {id, ok, result?, error?}
        const entry = pending.get(msg.id);
        if (!entry) return;
        pending.delete(msg.id);
        clearTimeout(entry.timer);
        if (msg.ok) entry.resolve(msg.result);
        else entry.reject(new Error(msg.error ?? "unknown plugin error"));
        return;
      }

      // Server client: {cid, method, params} -> forward to plugin
      if (typeof msg.cid !== "number" || typeof msg.method !== "string") return;
      if (!pluginSocket || pluginSocket.readyState !== 1) {
        send(ws, {
          cid: msg.cid,
          ok: false,
          error:
            "Figma is not connected. Open the Figma desktop app, open the file, then run the figma-bridge plugin from Plugins > Development.",
        });
        return;
      }
      const id = nextCallId++;
      const timer = setTimeout(() => {
        pending.delete(id);
        send(ws, { cid: msg.cid, ok: false, error: `Figma call ${msg.method} timed out after 30s` });
      }, 30_000);
      pending.set(id, {
        resolve: (result) => send(ws, { cid: msg.cid, ok: true, result }),
        reject: (err) => send(ws, { cid: msg.cid, ok: false, error: err.message }),
        timer,
      });
      send(pluginSocket, { id, method: msg.method, params: msg.params });
    },
    close(ws: any) {
      if (ws.data.role === "plugin") {
        if (pluginSocket === ws) {
          pluginSocket = null;
          rejectAllPending(new Error("Figma disconnected mid-call. Re-run the plugin and retry."));
        }
      } else {
        for (const c of clients) if (c.ws === ws) clients.delete(c);
      }
    },
  },
});
