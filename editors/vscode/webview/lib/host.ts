import type { ToHost, ToWebview } from "../../src/protocol";
import * as browser from "./browser";

interface VsCodeApi {
  postMessage(message: ToHost): void;
  getState(): unknown;
  setState(state: unknown): void;
}

declare function acquireVsCodeApi(): VsCodeApi;

const api = typeof acquireVsCodeApi === "function" ? acquireVsCodeApi() : undefined;

export const inEditor = api !== undefined;

/** Whether the surface already has a find widget: a browser and an editor tab
 *  do, the sidebar view does not. */
export const nativeFind =
  !inEditor ||
  (typeof document !== "undefined" &&
    document.getElementById("root")?.dataset.surface !== "sidebar");

export function post(message: ToHost): void {
  api ? api.postMessage(message) : browser.post(message);
}

/** Subscribe to host messages. Returns an unsubscribe. */
export function onHostMessage(handler: (message: ToWebview) => void): () => void {
  if (!api) {
    return browser.subscribe(handler);
  }
  const listener = (event: MessageEvent<ToWebview>) => handler(event.data);
  window.addEventListener("message", listener);
  return () => window.removeEventListener("message", listener);
}

/** Webview state survives the view being hidden and rebuilt, so the thread is
 *  still there when the panel comes back. */
export function persist(state: unknown): void {
  api ? api.setState(state) : browser.persist(state);
}

export function restore<T>(): T | undefined {
  return api ? (api.getState() as T | undefined) : browser.restore<T>();
}
