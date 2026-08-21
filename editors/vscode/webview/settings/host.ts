import type { SettingsToHost, SettingsToWebview } from "../../src/protocol";

interface VsCodeApi {
  postMessage(message: SettingsToHost): void;
}

declare function acquireVsCodeApi(): VsCodeApi;

/** Like the chat panel's bridge: the editor's API when there is one, and the
 *  `aster serve` endpoints when the page is just a page. */
const api = typeof acquireVsCodeApi === "function" ? acquireVsCodeApi() : undefined;

const HOST = "/api/settings";
const EVENTS = "/api/settings/events";

let queue: Promise<unknown> = Promise.resolve();

export function post(message: SettingsToHost): void {
  if (api) {
    api.postMessage(message);
    return;
  }
  // Posts go out in order: a write and the re-read it triggers must not cross.
  queue = queue
    .then(() =>
      fetch(HOST, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(message),
      })
    )
    .catch(() => {});
}

/** Subscribe to host messages. Returns an unsubscribe. */
export function onHostMessage(handler: (message: SettingsToWebview) => void): () => void {
  if (!api) {
    const stream = new EventSource(EVENTS);
    stream.onmessage = (event) => handler(JSON.parse(event.data) as SettingsToWebview);
    return () => stream.close();
  }
  const listener = (event: MessageEvent<SettingsToWebview>) => handler(event.data);
  window.addEventListener("message", listener);
  return () => window.removeEventListener("message", listener);
}
