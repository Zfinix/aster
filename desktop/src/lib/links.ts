import { openExternal } from "./aster";

/** Send link clicks to the OS browser. A plain anchor navigates the app's own
 *  webview away with no way back, so every click is intercepted here rather
 *  than per component. */
export function followLinksExternally(): () => void {
  const onClick = (event: MouseEvent) => {
    if (event.defaultPrevented || event.button !== 0) return;
    const anchor = (event.target as Element | null)?.closest?.("a");
    const url = anchor?.getAttribute("href");
    if (!url || url.startsWith("#")) return;
    event.preventDefault();
    void openExternal(new URL(url, window.location.href).href).catch(() => {});
  };
  document.addEventListener("click", onClick);
  return () => document.removeEventListener("click", onClick);
}
