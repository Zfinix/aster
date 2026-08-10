import type { ToHost, ToWebview } from "../../src/protocol";

interface VsCodeApi {
  postMessage(message: ToHost): void;
  getState(): unknown;
  setState(state: unknown): void;
}

declare function acquireVsCodeApi(): VsCodeApi;

const api = acquireVsCodeApi();

export function post(message: ToHost): void {
  api.postMessage(message);
}

/** Subscribe to host messages. Returns an unsubscribe. */
export function onHostMessage(handler: (message: ToWebview) => void): () => void {
  const listener = (event: MessageEvent<ToWebview>) => handler(event.data);
  window.addEventListener("message", listener);
  return () => window.removeEventListener("message", listener);
}

/**
 * Webview state survives the view being hidden and rebuilt, so the thread is
 * still there when the panel comes back.
 */
export function persist(state: unknown): void {
  api.setState(state);
}

export function restore<T>(): T | undefined {
  return api.getState() as T | undefined;
}
