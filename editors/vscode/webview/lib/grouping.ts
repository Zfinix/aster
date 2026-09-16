const KEY = "aster.groupToolCalls";

/** Tool calls fold unless the user turned it off. In the editor the host pushes
 *  the `aster.groupToolCalls` setting on init; localStorage is the cache here. */
export function groupingEnabled(): boolean {
  try {
    return localStorage.getItem(KEY) !== "off";
  } catch {
    return true;
  }
}

export function setGroupingEnabled(on: boolean): void {
  try {
    if (on) localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, "off");
  } catch {
    // No storage: the toggle still applies for this session.
  }
}
