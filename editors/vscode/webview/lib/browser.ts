import type { ReviewSource, ToHost, ToWebview } from "../../src/protocol";

/**
 * The host, when the page is a page: `aster serve` on this machine instead of
 * an extension. Messages go out as one POST each and come back on an event
 * stream, and the handful of things only an editor can do are answered here
 * with what a browser has instead.
 */

const HOST = "/api/host";
const EVENTS = "/api/events";

type Handler = (message: ToWebview) => void;

const handlers = new Set<Handler>();
/** Posts go out in order: two that cross would let an answer overtake the
 *  prompt it belongs to. */
let queue: Promise<unknown> = Promise.resolve();
let stream: EventSource | undefined;
/** A reconnect means the server came back, so the page asks to be told where
 *  things stand. The first connection is not one: App sends `ready` itself. */
let connected = false;

export function post(message: ToHost): void {
  switch (message.type) {
    // A browser opens links, scratch tabs, and file pickers on its own.
    case "openExternal":
      window.open(message.url, "_blank", "noopener,noreferrer");
      return;
    case "openUntitled":
      openScratch(message.content, message.lang, message.title);
      return;
    case "attachFiles":
      void attach();
      return;
    case "runCommand":
      runCommand(message.command);
      return;
    default:
      queue = queue.then(() => send(message)).catch(() => {});
  }
}

export function subscribe(handler: Handler): () => void {
  handlers.add(handler);
  connect();
  return () => handlers.delete(handler);
}

/** Thread state, kept per tab: a reload is the browser's version of the panel
 *  being hidden and rebuilt. */
export function persist(state: unknown): void {
  try {
    sessionStorage.setItem("aster.state", JSON.stringify(state));
  } catch {
    // A full or blocked store is not worth losing a turn over.
  }
}

export function restore<T>(): T | undefined {
  try {
    const saved = sessionStorage.getItem("aster.state");
    return saved ? (JSON.parse(saved) as T) : undefined;
  } catch {
    return undefined;
  }
}

async function send(message: ToHost): Promise<void> {
  await fetch(HOST, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(message),
  });
}

function connect(): void {
  if (stream) return;
  stream = new EventSource(EVENTS);
  stream.onmessage = (event: MessageEvent<string>) => {
    try {
      dispatch(JSON.parse(event.data) as ToWebview);
    } catch {
      // A malformed line is not worth killing the stream over.
    }
  };
  // EventSource reconnects on its own; what it cannot do is know the server
  // restarted underneath it, so every reopen re-asks for the state.
  stream.onopen = () => {
    if (connected) post({ type: "ready" });
    connected = true;
  };
}

function dispatch(message: ToWebview): void {
  for (const handler of handlers) {
    handler(message);
  }
}

/** The extension's commands, in the terms a page has. */
function runCommand(command: string): void {
  if (command === "aster.newConversation") {
    dispatch({ type: "newConversation" });
    return;
  }
  const kind = command === "aster.reviewRange" ? "range" : command === "aster.reviewPr" ? "pr" : null;
  if (!kind) return;
  const value = window.prompt(
    kind === "range" ? "Review a range, e.g. main..HEAD" : "Review a pull request, by number"
  );
  if (!value?.trim()) return;
  const source = { kind, value: value.trim() } as ReviewSource;
  const id = `review-${Date.now()}`;
  dispatch({ type: "reviewStarted", id, source });
  post({ type: "review", id, source });
}

/**
 * A code block, opened the way a page can open one: over the thread. Its own
 * tab would have to be a `blob:` URL, which not every browser will put in one,
 * and the ones that refuse load it in place: the session goes with it.
 */
function openScratch(content: string, lang?: string, title?: string): void {
  dispatch({ type: "scratch", content, lang, title });
}

/** The composer's `+`. A picked file arrives as bytes, like a paste, because
 *  the browser will not say where it came from. */
async function attach(): Promise<void> {
  const input = document.createElement("input");
  input.type = "file";
  input.multiple = true;
  const picked = await new Promise<FileList | null>((resolve) => {
    input.addEventListener("change", () => resolve(input.files), { once: true });
    input.addEventListener("cancel", () => resolve(null), { once: true });
    input.click();
  });
  const files = Array.from(picked ?? []);
  if (files.length === 0) return;
  post({
    type: "pasteFiles",
    files: await Promise.all(
      files.map(async (file) => ({
        name: file.name,
        size: file.size,
        data: await base64(file),
      }))
    ),
  });
}

function base64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error(`could not read ${file.name}`));
    // A data URL is base64 after the comma, which is the shape the host wants.
    reader.onload = () => resolve(String(reader.result).split(",")[1] ?? "");
    reader.readAsDataURL(file);
  });
}
