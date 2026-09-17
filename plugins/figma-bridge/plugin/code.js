/**
 * figma-bridge plugin.
 *
 * Runs inside the Figma desktop app. Connects out to the relay on
 * 127.0.0.1:4395 and executes Plugin API calls it forwards. Reconnects with
 * backoff so it survives the relay restarting.
 */

const RELAY_URL = "ws://127.0.0.1:4395/plugin";
const MAX_BACKOFF_MS = 10_000;

let ws = null;
let backoff = 500;
let closed = false;

function connect() {
  if (closed) return;
  try {
    ws = new WebSocket(RELAY_URL);
  } catch (err) {
    scheduleReconnect();
    return;
  }

  ws.onopen = () => {
    backoff = 500;
  };

  ws.onmessage = (event) => {
    let msg;
    try {
      msg = JSON.parse(event.data);
    } catch {
      return;
    }
    if (typeof msg.id !== "number" || typeof msg.method !== "string") return;
    handle(msg).catch(() => {});
  };

  ws.onclose = () => {
    ws = null;
    scheduleReconnect();
  };

  ws.onerror = () => {
    try {
      ws.close();
    } catch {}
  };
}

function scheduleReconnect() {
  if (closed) return;
  const wait = backoff;
  backoff = Math.min(backoff * 2, MAX_BACKOFF_MS);
  setTimeout(connect, wait);
}

async function handle(msg) {
  let result = null;
  try {
    result = { ok: true, result: await run(msg.method, msg.params ?? {}) };
  } catch (err) {
    result = { ok: false, error: err && err.message ? err.message : String(err) };
  }
  if (ws && ws.readyState === 1) {
    ws.send(JSON.stringify({ id: msg.id, ...result }));
  }
}

async function run(method, params) {
  switch (method) {
    case "ping":
      return { file: figma.root.name, page: figma.currentPage.name };
    case "execute":
      return execute(params.code);
    case "get_tree":
      return getTree(params);
    case "get_node":
      return getNode(params.nodeId);
    case "get_screenshot":
      return screenshot(params);
    case "notify":
      figma.notify(String(params.message));
      return null;
    case "create_frame":
      return createFrame(params);
    case "create_text":
      return createText(params);
    case "set_properties":
      return setProperties(params);
    case "delete_node":
      return deleteNode(params.nodeId);
    case "clone_node":
      return cloneNode(params);
    case "set_fills":
      return setFills(params);
    case "set_strokes":
      return setStrokes(params);
    case "set_effects":
      return setEffects(params);
    case "import_component":
      return importComponent(params);
    case "get_selection":
      return getSelection();
    case "set_selection":
      return setSelection(params.nodeIds);
    case "export_node":
      return exportNode(params);
    default:
      throw new Error(`unknown method ${method}`);
  }
}

// ---------------------------------------------------------------------------
// Methods
// ---------------------------------------------------------------------------

async function execute(code) {
  const fn = new Function("figma", `"use strict";\nreturn (async () => {\n${code}\n})();`);
  const value = await fn(figma);
  return { value: serialize(value) };
}

function getTree(params) {
  const maxDepth = Number.isFinite(params.maxDepth) ? params.maxDepth : 6;
  const filter = params.filter ? new RegExp(params.filter) : null;
  const root = params.nodeId ? figma.getNodeById(params.nodeId) : figma.root;
  if (!root) throw new Error(`no node ${params.nodeId}`);
  return summarize(root, 0, maxDepth, filter);
}

function summarize(node, depth, maxDepth, filter) {
  const out = {
    id: node.id,
    name: node.name,
    type: node.type,
  };
  if (node.children && depth < maxDepth) {
    out.children = node.children
      .filter((c) => !filter || filter.test(c.type))
      .map((c) => summarize(c, depth + 1, maxDepth, filter));
  } else if (node.children) {
    out.truncated = true;
    out.childCount = node.children.length;
  }
  return out;
}

function getNode(nodeId) {
  const node = figma.getNodeById(nodeId);
  if (!node) throw new Error(`no node ${nodeId}`);
  const out = {
    id: node.id,
    name: node.name,
    type: node.type,
  };
  for (const key of [
    "fills",
    "strokes",
    "effects",
    "constraints",
    "layoutMode",
    "primaryAxisAlignItems",
    "counterAxisAlignItems",
    "itemSpacing",
    "paddingLeft",
    "paddingRight",
    "paddingTop",
    "paddingBottom",
    "cornerRadius",
    "opacity",
    "visible",
    "characters",
    "fontSize",
    "fontName",
    "textAlignHorizontal",
    "textAlignVertical",
    "lineHeight",
    "letterSpacing",
  ]) {
    try {
      const v = node[key];
      if (v !== undefined) out[key] = serialize(v);
    } catch {}
  }
  try {
    out.absoluteTransform = node.absoluteTransform;
  } catch {}
  try {
    out.absoluteBoundingBox = node.absoluteBoundingBox;
  } catch {}
  if (node.children) {
    out.children = node.children.map((c) => ({ id: c.id, name: c.name, type: c.type }));
  }
  return out;
}

async function screenshot(params) {
  let node = figma.currentPage;
  let note = null;
  if (params.nodeId) {
    const n = figma.getNodeById(params.nodeId);
    if (!n) throw new Error(`no node ${params.nodeId}`);
    node = n;
  } else if (params.viewport) {
    // Export the visible area by rendering the page's visible nodes region.
    const nodes = figma.currentPage.children.filter((c) => c.visible);
    if (nodes.length === 0) throw new Error("page is empty");
    note = "viewport export approximated by rendering all top-level nodes";
    node = figma.currentPage;
  }
  const format = params.format === "jpg" ? "JPG" : "PNG";
  const bytes = await node.exportAsync({
    format,
    constraint: { type: "SCALE", value: params.scale ?? 1 },
  });
  return {
    data: arrayBufferToBase64(bytes),
    mimeType: format === "JPG" ? "image/jpeg" : "image/png",
    note,
  };
}

// ---------------------------------------------------------------------------
// Write surface
// ---------------------------------------------------------------------------

function createFrame(params) {
  const parent = params.parentId ? figma.getNodeById(params.parentId) : figma.currentPage;
  if (!parent) throw new Error(`no node ${params.parentId}`);
  const frame = figma.createFrame();
  frame.name = params.name ?? "Frame";
  frame.resize(Number(params.width ?? 100), Number(params.height ?? 100));
  if (params.x !== undefined) frame.x = Number(params.x);
  if (params.y !== undefined) frame.y = Number(params.y);
  if (params.fills) frame.fills = params.fills;
  if (params.cornerRadius !== undefined) frame.cornerRadius = Number(params.cornerRadius);
  if (params.layoutMode) {
    frame.layoutMode = params.layoutMode;
    if (params.itemSpacing !== undefined) frame.itemSpacing = Number(params.itemSpacing);
    if (params.paddingLeft !== undefined) frame.paddingLeft = Number(params.paddingLeft);
    if (params.paddingRight !== undefined) frame.paddingRight = Number(params.paddingRight);
    if (params.paddingTop !== undefined) frame.paddingTop = Number(params.paddingTop);
    if (params.paddingBottom !== undefined) frame.paddingBottom = Number(params.paddingBottom);
    if (params.primaryAxisAlignItems) frame.primaryAxisAlignItems = params.primaryAxisAlignItems;
    if (params.counterAxisAlignItems) frame.counterAxisAlignItems = params.counterAxisAlignItems;
  }
  parent.appendChild(frame);
  return { id: frame.id, name: frame.name };
}

function createText(params) {
  const parent = params.parentId ? figma.getNodeById(params.parentId) : figma.currentPage;
  if (!parent) throw new Error(`no node ${params.parentId}`);
  const text = figma.createText();
  text.name = params.name ?? "Text";
  if (params.fontName) text.fontName = params.fontName;
  text.characters = String(params.characters ?? "");
  if (params.fontSize !== undefined) text.fontSize = Number(params.fontSize);
  if (params.fills) text.fills = params.fills;
  if (params.textAlignHorizontal) text.textAlignHorizontal = params.textAlignHorizontal;
  if (params.textAutoResize) text.textAutoResize = params.textAutoResize;
  if (params.width !== undefined && params.height !== undefined) {
    text.resize(Number(params.width), Number(params.height));
  }
  if (params.x !== undefined) text.x = Number(params.x);
  if (params.y !== undefined) text.y = Number(params.y);
  parent.appendChild(text);
  return { id: text.id, name: text.name };
}

function setProperties(params) {
  const node = figma.getNodeById(params.nodeId);
  if (!node) throw new Error(`no node ${params.nodeId}`);
  const applied = [];
  for (const [key, value] of Object.entries(params.properties ?? {})) {
    node[key] = value;
    applied.push(key);
  }
  return { id: node.id, applied };
}

function deleteNode(nodeId) {
  const node = figma.getNodeById(nodeId);
  if (!node) throw new Error(`no node ${nodeId}`);
  const type = node.type;
  node.remove();
  return { removed: nodeId, type };
}

function cloneNode(params) {
  const node = figma.getNodeById(params.nodeId);
  if (!node) throw new Error(`no node ${params.nodeId}`);
  const copy = node.clone();
  const parent = params.parentId ? figma.getNodeById(params.parentId) : node.parent;
  if (!parent) throw new Error(`no node ${params.parentId}`);
  parent.appendChild(copy);
  if (params.x !== undefined) copy.x = Number(params.x);
  if (params.y !== undefined) copy.y = Number(params.y);
  return { id: copy.id, name: copy.name };
}

function setFills(params) {
  return paint(params, "fills");
}

function setStrokes(params) {
  return paint(params, "strokes");
}

function setEffects(params) {
  const node = figma.getNodeById(params.nodeId);
  if (!node) throw new Error(`no node ${params.nodeId}`);
  node.effects = params.effects;
  return { id: node.id, effects: serialize(node.effects) };
}

function paint(params, prop) {
  const node = figma.getNodeById(params.nodeId);
  if (!node) throw new Error(`no node ${params.nodeId}`);
  node[prop] = params[prop];
  return { id: node.id, [prop]: serialize(node[prop]) };
}

async function importComponent(params) {
  const key = params.componentKey;
  if (!key) throw new Error("componentKey is required");
  const component = await figma.importComponentAsync({ key });
  const instance = component.createInstance();
  const parent = params.parentId ? figma.getNodeById(params.parentId) : figma.currentPage;
  if (!parent) throw new Error(`no node ${params.parentId}`);
  parent.appendChild(instance);
  if (params.x !== undefined) instance.x = Number(params.x);
  if (params.y !== undefined) instance.y = Number(params.y);
  return { id: instance.id, name: instance.name, componentKey: key };
}

// ---------------------------------------------------------------------------
// Selection and export
// ---------------------------------------------------------------------------

function getSelection() {
  return {
    nodes: figma.currentPage.selection.map((n) => ({
      id: n.id,
      name: n.name,
      type: n.type,
    })),
  };
}

function setSelection(nodeIds) {
  const nodes = (nodeIds ?? []).map((id) => {
    const n = figma.getNodeById(id);
    if (!n) throw new Error(`no node ${id}`);
    return n;
  });
  figma.currentPage.selection = nodes;
  if (nodes.length > 0) {
    figma.viewport.scrollAndZoomIntoView(nodes);
  }
  return { selected: nodes.map((n) => n.id) };
}

async function exportNode(params) {
  const node = figma.getNodeById(params.nodeId);
  if (!node) throw new Error(`no node ${params.nodeId}`);
  const formatMap = { png: "PNG", jpg: "JPG", svg: "SVG", pdf: "PDF" };
  const format = formatMap[params.format] ?? "PNG";
  const settings = { format };
  if (format === "PNG" || format === "JPG") {
    settings.constraint = { type: "SCALE", value: params.scale ?? 2 };
  }
  const bytes = await node.exportAsync(settings);
  return {
    data: arrayBufferToBase64(bytes),
    mimeType:
      format === "PNG"
        ? "image/png"
        : format === "JPG"
          ? "image/jpeg"
          : format === "SVG"
            ? "image/svg+xml"
            : "application/pdf",
  };
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

function serialize(value, depth = 0) {
  if (value === null) return null;
  if (value === undefined) return undefined;
  const t = typeof value;
  if (t === "string" || t === "boolean") return value;
  if (t === "number") return value;
  if (Array.isArray(value)) {
    if (depth > 8) return "[...]";
    return value.map((v) => serialize(v, depth + 1));
  }
  if (t === "object") {
    if (depth > 8) return "{...}";
    const out = {};
    for (const k of Object.keys(value)) {
      try {
        out[k] = serialize(value[k], depth + 1);
      } catch {
        out[k] = "<unserializable>";
      }
    }
    return out;
  }
  return String(value);
}

function arrayBufferToBase64(buffer) {
  const bytes = new Uint8Array(buffer);
  let binary = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

connect();
