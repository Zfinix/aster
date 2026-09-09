/**
 * Realtime demo: the CLI is the driver.
 *
 * Serves index.html and, per browser connection, spawns a real `aster acp`
 * child speaking the same newline-delimited JSON-RPC the VS Code extension
 * speaks (see editors/vscode/src/chatRunner.ts). Every raw frame in either
 * direction is forwarded to the page, so the graph lights up from actual
 * traffic rather than a script.
 *
 * Run: bun demo/cli-drives-ui/server.ts   (aster must be on PATH)
 */
import { fileURLToPath } from "node:url";

const REPO = fileURLToPath(new URL("../../", import.meta.url));
const PORT = Number(process.env.PORT ?? 4170);

type Ws = { send: (s: string) => void; data?: Session };

class Session {
  private proc: ReturnType<typeof Bun.spawn> | undefined;
  private nextId = 0;
  private pending = new Map<number, (v: unknown) => void>();
  private sessionId = "";
  private ws: Ws;

  constructor(ws: Ws) {
    this.ws = ws;
  }

  async start() {
    this.proc = Bun.spawn(["aster", "acp"], {
      cwd: REPO,
      stdin: "pipe",
      stdout: "pipe",
      stderr: "pipe",
    });
    this.frame("ui→cli", "spawn aster acp (cwd: repo root)");
    this.proc.stderr && readLines(this.proc.stderr, (l) => this.frame("cli stderr", l));
    readLines(this.proc.stdout, (line) => this.onLine(line));
    await this.call("initialize", {
      protocolVersion: 1,
      clientCapabilities: { fs: { readTextFile: false, writeTextFile: false } },
      clientInfo: { name: "aster-demo", title: "Aster", version: "0.0.0" },
    });
    const created = (await this.call("session/new", {
      cwd: REPO,
      mcpServers: [],
    })) as { sessionId?: string };
    this.sessionId = created?.sessionId ?? "";
    this.frame("ui→cli", `session/new → ${this.sessionId.slice(0, 8)}…`);
  }

  sendPrompt(text: string) {
    if (!this.proc || !this.sessionId) return;
    this.frame("ui→cli", `session/prompt "${text.slice(0, 60)}"`);
    this.call("session/prompt", {
      sessionId: this.sessionId,
      prompt: [{ type: "text", text }],
    }).then((result) => {
      const stop = (result as { stopReason?: string })?.stopReason;
      this.emit({ type: "done", stopReason: stop ?? null });
    });
  }

  replyPermission(id: number, optionId: string | null) {
    this.frame("ui→cli", `permission reply → ${optionId ?? "cancelled"}`);
    this.write({
      jsonrpc: "2.0",
      id,
      result: {
        outcome: optionId
          ? { outcome: "selected", optionId }
          : { outcome: "cancelled" },
      },
    });
  }

  stop() {
    this.proc?.kill();
  }

  private write(msg: unknown) {
    this.proc?.stdin.write(`${JSON.stringify(msg)}\n`);
  }

  private call(method: string, params: unknown): Promise<unknown> {
    const id = ++this.nextId;
    return new Promise((resolve, reject) => {
      this.pending.set(id, resolve);
      this.write({ jsonrpc: "2.0", id, method, params });
      setTimeout(() => {
        if (this.pending.delete(id)) reject(new Error(`${method} timed out`));
      }, 30_000);
    });
  }

  private onLine(line: string) {
    let msg: {
      id?: number;
      method?: string;
      params?: any;
      result?: unknown;
    };
    try {
      msg = JSON.parse(line);
    } catch {
      return;
    }
    if (msg.method && msg.id !== undefined) {
      if (msg.method === "session/request_permission") {
        const call = msg.params?.toolCall ?? {};
        this.emit({
          type: "permission",
          id: msg.id,
          title: call.title ?? "Approve this action",
          options: (msg.params?.options ?? []).map(
            (o: { optionId: string; name: string }) => o
          ),
        });
        return;
      }
      this.write({
        jsonrpc: "2.0",
        id: msg.id,
        error: { code: -32601, message: `${msg.method} is not supported` },
      });
      return;
    }
    if (msg.id !== undefined) {
      this.frame("cli→ui", `response #${msg.id}`);
      this.pending.get(msg.id ?? 0)?.(msg.result);
      this.pending.delete(msg.id ?? 0);
      return;
    }
    if (msg.method === "session/update") {
      const update = msg.params?.update ?? {};
      this.frame("cli→ui", `session/update · ${update.sessionUpdate ?? "?"}`);
      this.emit({ type: "update", update });
    } else {
      this.frame("cli→ui", `${msg.method ?? "?"}`);
    }
  }

  private frame(dir: string, line: string) {
    this.emit({ type: "frame", dir, line });
  }

  private emit(event: unknown) {
    this.ws.send(JSON.stringify(event));
  }
}

async function readLines(
  stream: ReadableStream<Uint8Array>,
  onLine: (line: string) => void
) {
  const dec = new TextDecoder();
  let buf = "";
  for await (const chunk of stream) {
    buf += dec.decode(chunk, { stream: true });
    let i;
    while ((i = buf.indexOf("\n")) !== -1) {
      const line = buf.slice(0, i).trim();
      buf = buf.slice(i + 1);
      if (line) onLine(line);
    }
  }
}

const server = Bun.serve({
  port: PORT,
  fetch(req, srv) {
    const url = new URL(req.url);
    if (url.pathname === "/ws") {
      if (srv.upgrade(req, { data: undefined })) return;
      return new Response("upgrade failed", { status: 400 });
    }
    if (url.pathname === "/") {
      return new Response(Bun.file(new URL("./index.html", import.meta.url)));
    }
    return new Response("not found", { status: 404 });
  },
  websocket: {
    open(ws) {
      const session = new Session(ws as unknown as Ws);
      ws.data = session;
      session.start().catch((err) => {
        (ws as unknown as Ws).send(
          JSON.stringify({ type: "error", message: String(err) })
        );
      });
    },
    message(ws, raw) {
      const session = (ws as unknown as Ws).data;
      if (!session) return;
      let msg: any;
      try {
        msg = JSON.parse(String(raw));
      } catch {
        return;
      }
      if (msg.type === "prompt") session.sendPrompt(String(msg.text ?? ""));
      if (msg.type === "permission_reply")
        session.replyPermission(Number(msg.id), msg.optionId ?? null);
    },
    close(ws) {
      (ws as unknown as Ws).data?.stop();
    },
  },
});

console.log(`demo on http://localhost:${server.port}`);