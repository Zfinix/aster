import { useEffect, useState } from "react";

/** The current time, refreshed once a second while `live` is true, so an
 *  elapsed counter ticks without every card running its own clock. */
export function useNow(live: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!live) return;
    setNow(Date.now());
    const tick = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(tick);
  }, [live]);
  return now;
}
