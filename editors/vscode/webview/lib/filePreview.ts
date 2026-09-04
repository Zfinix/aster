const EVENT = "aster:file-preview";

/** Ask for a peek at a file instead of throwing it at the OS. */
export function openFilePreview(path: string): void {
  window.dispatchEvent(new CustomEvent(EVENT, { detail: path }));
}

export function onFilePreviewOpen(handler: (path: string) => void): () => void {
  const listener = (event: Event) => handler((event as CustomEvent<string>).detail);
  window.addEventListener(EVENT, listener);
  return () => window.removeEventListener(EVENT, listener);
}