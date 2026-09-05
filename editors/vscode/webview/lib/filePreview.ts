import { inEditor, post } from "./host";

const EVENT = "aster:file-preview";

/** In the editor a file opens as a real tab; the browser has none, so it
 *  peeks at the file over the thread instead. */
export function openFilePreview(path: string, line?: number): void {
  if (inEditor) {
    post({ type: "openFile", path, line });
    return;
  }
  window.dispatchEvent(new CustomEvent(EVENT, { detail: path }));
}

export function onFilePreviewOpen(handler: (path: string) => void): () => void {
  const listener = (event: Event) => handler((event as CustomEvent<string>).detail);
  window.addEventListener(EVENT, listener);
  return () => window.removeEventListener(EVENT, listener);
}
