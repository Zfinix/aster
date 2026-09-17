/**
 * figma-bridge MCP server.
 *
 * stdio MCP server. Forwards tool calls over a WebSocket to the relay daemon
 * (server/relay.ts) on 127.0.0.1:4395, which routes them to the plugin running
 * inside Figma. Spawns the relay if it is not already running, so several
 * aster sessions can share one plugin connection without port conflicts.
 *
 * Run with: bun server/index.ts
 */

const RELAY_URL = "ws://127.0.0.1:4395/server";
const RELAY_PORT = 4395;
const CALL_TIMEOUT_MS = 30_000;
const MAX_RESULT_CHARS = 400_000;

// ---------------------------------------------------------------------------
// Relay client
// ---------------------------------------------------------------------------

type Pending = {
  resolve: (value: unknown) => void;
  reject: (err: Error) => void;
  timer: ReturnType<typeof setTimeout>;
};

let relay: any = null;
const pending = new Map<number, Pending>();
let nextCallId = 1;

function spawnRelay() {
  const script = new URL("./relay.ts", import.meta.url).pathname;
  try {
    const proc = Bun.spawn(["bun", script], {
      stdin: "ignore",
      stdout: "ignore",
      stderr: "ignore",
    });
    proc.unref();
  } catch {}
}

async function connectRelay(): Promise<void> {
  return new Promise((resolve, reject) => {
    let settled = false;
    const ws = new WebSocket(RELAY_URL);
    const timer = setTimeout(() => {
      if (!settled) {
        settled = true;
        try {
          ws.close();
        } catch {}
        reject(new Error("relay connect timed out"));
      }
    }, 2000);
    ws.onopen = () => {
      if (!settled) {
        settled = true;
        clearTimeout(timer);
        relay = ws;
        resolve();
      }
    };
    ws.onmessage = (event) => {
      let msg: { cid: number; ok: boolean; result?: unknown; error?: string };
      try {
        msg = JSON.parse(String(event.data));
      } catch {
        return;
      }
      const entry = pending.get(msg.cid);
      if (!entry) return;
      pending.delete(msg.cid);
      clearTimeout(entry.timer);
      if (msg.ok) entry.resolve(msg.result);
      else entry.reject(new Error(msg.error ?? "unknown plugin error"));
    };
    ws.onclose = () => {
      relay = null;
      rejectAllPending(new Error("Relay connection lost. Re-run the plugin and retry."));
      if (!settled) {
        settled = true;
        clearTimeout(timer);
        reject(new Error("relay closed before connect"));
      }
    };
    ws.onerror = () => {
      try {
        ws.close();
      } catch {}
    };
  });
}

async function ensureRelay() {
  if (relay && relay.readyState === 1) return;
  try {
    await connectRelay();
    return;
  } catch {}
  // Relay not up: spawn the daemon and wait for the port.
  spawnRelay();
  for (let attempt = 0; attempt < 10; attempt++) {
    await new Promise((r) => setTimeout(r, 300));
    try {
      await connectRelay();
      return;
    } catch {}
  }
  throw new Error(
    "Could not start the figma-bridge relay on 127.0.0.1:4395. Start it manually: bun server/relay.ts",
  );
}

function callPlugin(method: string, params: unknown): Promise<unknown> {
  return ensureRelay().then(() =>
    new Promise((resolve, reject) => {
      const cid = nextCallId++;
      const timer = setTimeout(() => {
        pending.delete(cid);
        reject(new Error(`Figma call ${method} timed out after ${CALL_TIMEOUT_MS / 1000}s`));
      }, CALL_TIMEOUT_MS);
      pending.set(cid, { resolve, reject, timer });
      relay.send(JSON.stringify({ cid, method, params }));
    }),
  );
}

function rejectAllPending(err: Error) {
  for (const [, p] of pending) {
    clearTimeout(p.timer);
    p.reject(err);
  }
  pending.clear();
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

type Content = { type: "text"; text: string } | { type: "image"; data: string; mimeType: string };

type ToolDef = {
  description: string;
  inputSchema: Record<string, unknown>;
  run: (args: any) => Promise<Content[]>;
};

function text(t: string): Content[] {
  const clipped =
    t.length > MAX_RESULT_CHARS ? t.slice(0, MAX_RESULT_CHARS) + "...[truncated]" : t;
  return [{ type: "text", text: clipped }];
}

const TOOLS: Record<string, ToolDef> = {
  execute: {
    description:
      "Run arbitrary Figma Plugin API code inside the Figma desktop app. `figma` is in scope. The value of the last expression is returned as JSON. Read with get_tree/get_node/get_screenshot first, then mutate. After mutating, call notify so the human sees a toast.",
    inputSchema: {
      type: "object",
      properties: { code: { type: "string", description: "Plugin API code to run" } },
      required: ["code"],
    },
    run: (args) =>
      callPlugin("execute", { code: String(args.code) }).then((r) =>
        text(JSON.stringify(r, null, 2)),
      ),
  },
  get_tree: {
    description:
      "Read the document node tree: ids, names, types, and layout. Cheaper than execute for orientation. Returns the subtree under node_id (default: document root), capped by max_depth.",
    inputSchema: {
      type: "object",
      properties: {
        node_id: { type: "string", description: "Subtree root; omit for the whole document" },
        max_depth: { type: "number", description: "Depth cap, default 6" },
        filter: { type: "string", description: "Regex; only node types matching it are expanded" },
      },
    },
    run: (args) =>
      callPlugin("get_tree", {
        nodeId: args.node_id,
        maxDepth: args.max_depth,
        filter: args.filter,
      }).then((r) => text(JSON.stringify(r, null, 2))),
  },
  get_node: {
    description:
      "Full detail for one node: fills, strokes, effects, text style, constraints, absolute transform, children ids.",
    inputSchema: {
      type: "object",
      properties: { node_id: { type: "string" } },
      required: ["node_id"],
    },
    run: (args) =>
      callPlugin("get_node", { nodeId: String(args.node_id) }).then((r) =>
        text(JSON.stringify(r, null, 2)),
      ),
  },
  get_screenshot: {
    description:
      "Render the canvas (or one node) to an image and return it. This is how you see your work.",
    inputSchema: {
      type: "object",
      properties: {
        viewport: { type: "boolean", description: "true: current viewport; false: whole page" },
        node_id: { type: "string", description: "Render this node instead" },
        format: { type: "string", enum: ["png", "jpg"] },
        scale: { type: "number", description: "Export scale, default 1" },
      },
    },
    run: (args) =>
      callPlugin("get_screenshot", {
        viewport: args.viewport,
        nodeId: args.node_id,
        format: args.format ?? "png",
        scale: args.scale ?? 1,
      }).then((r: any) => {
        const c: Content[] = [{ type: "image", data: r.data, mimeType: r.mimeType }];
        if (r.note) c.push({ type: "text", text: r.note });
        return c;
      }),
  },
  notify: {
    description:
      "Show a toast inside Figma (figma.notify) so the human sees what the agent did.",
    inputSchema: {
      type: "object",
      properties: { message: { type: "string" } },
      required: ["message"],
    },
    run: (args) => callPlugin("notify", { message: String(args.message) }).then(() => text("ok")),
  },
  create_frame: {
    description:
      "Create a frame. Parent defaults to the current page. Pass layoutMode (HORIZONTAL/VERTICAL/NONE) plus itemSpacing and padding for auto-layout.",
    inputSchema: {
      type: "object",
      properties: {
        parent_id: { type: "string" },
        name: { type: "string" },
        x: { type: "number" },
        y: { type: "number" },
        width: { type: "number" },
        height: { type: "number" },
        fills: { type: "array", description: "Figma Paint objects" },
        corner_radius: { type: "number" },
        layout_mode: { type: "string", enum: ["NONE", "HORIZONTAL", "VERTICAL"] },
        item_spacing: { type: "number" },
        padding_left: { type: "number" },
        padding_right: { type: "number" },
        padding_top: { type: "number" },
        padding_bottom: { type: "number" },
        primary_axis_align_items: { type: "string" },
        counter_axis_align_items: { type: "string" },
      },
    },
    run: (args) =>
      callPlugin("create_frame", {
        parentId: args.parent_id,
        name: args.name,
        x: args.x,
        y: args.y,
        width: args.width,
        height: args.height,
        fills: args.fills,
        cornerRadius: args.corner_radius,
        layoutMode: args.layout_mode,
        itemSpacing: args.item_spacing,
        paddingLeft: args.padding_left,
        paddingRight: args.padding_right,
        paddingTop: args.padding_top,
        paddingBottom: args.padding_bottom,
        primaryAxisAlignItems: args.primary_axis_align_items,
        counterAxisAlignItems: args.counter_axis_align_items,
      }).then((r) => text(JSON.stringify(r, null, 2))),
  },
  create_text: {
    description:
      "Create a text node. fontName is {family, style}; load the font first via execute if it is not Inter Regular.",
    inputSchema: {
      type: "object",
      properties: {
        parent_id: { type: "string" },
        name: { type: "string" },
        characters: { type: "string" },
        font_name: { type: "object", properties: { family: { type: "string" }, style: { type: "string" } } },
        font_size: { type: "number" },
        fills: { type: "array" },
        text_align_horizontal: { type: "string" },
        text_auto_resize: { type: "string" },
        x: { type: "number" },
        y: { type: "number" },
        width: { type: "number" },
        height: { type: "number" },
      },
    },
    run: (args) =>
      callPlugin("create_text", {
        parentId: args.parent_id,
        name: args.name,
        characters: args.characters,
        fontName: args.font_name,
        fontSize: args.font_size,
        fills: args.fills,
        textAlignHorizontal: args.text_align_horizontal,
        textAutoResize: args.text_auto_resize,
        x: args.x,
        y: args.y,
        width: args.width,
        height: args.height,
      }).then((r) => text(JSON.stringify(r, null, 2))),
  },
  set_properties: {
    description:
      "Set any writable Plugin API properties on a node in one call: {\"cornerRadius\": 8, \"opacity\": 0.5, ...}. Unknown keys throw the API's own error.",
    inputSchema: {
      type: "object",
      properties: {
        node_id: { type: "string" },
        properties: { type: "object", description: "key/value map of Plugin API properties" },
      },
      required: ["node_id", "properties"],
    },
    run: (args) =>
      callPlugin("set_properties", { nodeId: args.node_id, properties: args.properties }).then((r) =>
        text(JSON.stringify(r, null, 2)),
      ),
  },
  delete_node: {
    description: "Remove a node from the document.",
    inputSchema: {
      type: "object",
      properties: { node_id: { type: "string" } },
      required: ["node_id"],
    },
    run: (args) =>
      callPlugin("delete_node", { nodeId: args.node_id }).then((r) => text(JSON.stringify(r, null, 2))),
  },
  clone_node: {
    description: "Duplicate a node, optionally into a different parent and position.",
    inputSchema: {
      type: "object",
      properties: {
        node_id: { type: "string" },
        parent_id: { type: "string" },
        x: { type: "number" },
        y: { type: "number" },
      },
      required: ["node_id"],
    },
    run: (args) =>
      callPlugin("clone_node", {
        nodeId: args.node_id,
        parentId: args.parent_id,
        x: args.x,
        y: args.y,
      }).then((r) => text(JSON.stringify(r, null, 2))),
  },
  set_fills: {
    description: "Replace a node's fills with Figma Paint objects.",
    inputSchema: {
      type: "object",
      properties: {
        node_id: { type: "string" },
        fills: { type: "array", description: "Figma Paint objects" },
      },
      required: ["node_id", "fills"],
    },
    run: (args) =>
      callPlugin("set_fills", { nodeId: args.node_id, fills: args.fills }).then((r) =>
        text(JSON.stringify(r, null, 2)),
      ),
  },
  set_strokes: {
    description: "Replace a node's strokes with Figma Paint objects.",
    inputSchema: {
      type: "object",
      properties: {
        node_id: { type: "string" },
        strokes: { type: "array", description: "Figma Paint objects" },
      },
      required: ["node_id", "strokes"],
    },
    run: (args) =>
      callPlugin("set_strokes", { nodeId: args.node_id, strokes: args.strokes }).then((r) =>
        text(JSON.stringify(r, null, 2)),
      ),
  },
  set_effects: {
    description: "Replace a node's effects (shadows, blurs) with Figma Effect objects.",
    inputSchema: {
      type: "object",
      properties: {
        node_id: { type: "string" },
        effects: { type: "array", description: "Figma Effect objects" },
      },
      required: ["node_id", "effects"],
    },
    run: (args) =>
      callPlugin("set_effects", { nodeId: args.node_id, effects: args.effects }).then((r) =>
        text(JSON.stringify(r, null, 2)),
      ),
  },
  import_component: {
    description:
      "Instantiate a team library component by its component key (from the component's share link or get_node on an instance).",
    inputSchema: {
      type: "object",
      properties: {
        component_key: { type: "string" },
        parent_id: { type: "string" },
        x: { type: "number" },
        y: { type: "number" },
      },
      required: ["component_key"],
    },
    run: (args) =>
      callPlugin("import_component", {
        componentKey: args.component_key,
        parentId: args.parent_id,
        x: args.x,
        y: args.y,
      }).then((r) => text(JSON.stringify(r, null, 2))),
  },
  get_selection: {
    description: "What the human has selected in Figma right now.",
    inputSchema: { type: "object", properties: {} },
    run: () => callPlugin("get_selection", {}).then((r) => text(JSON.stringify(r, null, 2))),
  },
  set_selection: {
    description:
      "Select nodes in the Figma editor and scroll the viewport to them, so the human sees where you are working.",
    inputSchema: {
      type: "object",
      properties: { node_ids: { type: "array", items: { type: "string" } } },
      required: ["node_ids"],
    },
    run: (args) =>
      callPlugin("set_selection", { nodeIds: args.node_ids }).then((r) =>
        text(JSON.stringify(r, null, 2)),
      ),
  },
  export_node: {
    description:
      "Export one node as png/jpg/svg/pdf and return the bytes (base64 for png/jpg, text for svg).",
    inputSchema: {
      type: "object",
      properties: {
        node_id: { type: "string" },
        format: { type: "string", enum: ["png", "jpg", "svg", "pdf"] },
        scale: { type: "number", description: "Raster scale, default 2" },
      },
      required: ["node_id"],
    },
    run: (args) =>
      callPlugin("export_node", {
        nodeId: args.node_id,
        format: args.format ?? "png",
        scale: args.scale,
      }).then((r: any) => {
        if (r.mimeType === "image/svg+xml") {
          const svg = Buffer.from(r.data, "base64").toString("utf8");
          return text(svg);
        }
        return [{ type: "image", data: r.data, mimeType: r.mimeType }];
      }),
  },
};

// ---------------------------------------------------------------------------
// MCP over stdio
// ---------------------------------------------------------------------------

function handle(req: any): Promise<any> | any {
  const { id, method, params } = req;
  if (method === "initialize") {
    return {
      protocolVersion: params?.protocolVersion ?? "2024-11-05",
      capabilities: { tools: {} },
      serverInfo: { name: "figma-bridge", version: "0.1.0" },
    };
  }
  if (method === "notifications/initialized") return undefined;
  if (method === "ping") return { jsonrpc: "2.0", id, result: {} };
  if (method === "tools/list") {
    return {
      tools: Object.entries(TOOLS).map(([name, t]) => ({
        name,
        description: t.description,
        inputSchema: t.inputSchema,
      })),
    };
  }
  if (method === "tools/call") {
    const name = params?.name as string;
    const tool = TOOLS[name];
    if (!tool) {
      return { jsonrpc: "2.0", id, error: { code: -32602, message: `unknown tool ${name}` } };
    }
    return tool.run(params?.arguments ?? {}).then(
      (content) => ({ jsonrpc: "2.0", id, result: { content } }),
      (err: Error) => ({
        jsonrpc: "2.0",
        id,
        result: { content: [{ type: "text", text: `Error: ${err.message}` }], isError: true },
      }),
    );
  }
  return { jsonrpc: "2.0", id, error: { code: -32601, message: `method not found: ${method}` } };
}

let buf = "";
for await (const chunk of Bun.stdin.stream()) {
  buf += new TextDecoder().decode(chunk);
  let idx: number;
  while ((idx = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, idx).trim();
    buf = buf.slice(idx + 1);
    if (!line) continue;
    let req: any;
    try {
      req = JSON.parse(line);
    } catch {
      continue;
    }
    const out = await handle(req);
    if (out !== undefined) {
      process.stdout.write(JSON.stringify(out) + "\n");
    }
  }
}
