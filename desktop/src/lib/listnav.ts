import { useEffect, useRef, useState, type RefObject } from "react";

interface Key {
  key: string;
  shiftKey: boolean;
  preventDefault: () => void;
}

/** The keyboard half of a list: an active row, arrows that wrap, Enter that
 *  picks. The caller decides where the keys come from. The pointer moves the
 *  same cursor and takes it along when it leaves, so no highlight is left behind. */
export function useListNav<T extends HTMLElement>({
  count,
  onPick,
  resetOn,
  tabCompletes = false,
}: {
  count: number;
  onPick: (index: number, complete: boolean) => void;
  resetOn?: unknown;
  tabCompletes?: boolean;
}) {
  const [active, setActive] = useState(0);
  const activeRef = useRef<T>(null);

  useEffect(() => setActive(0), [resetOn]);

  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const onKey = (e: Key): boolean => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const by = e.key === "ArrowDown" ? 1 : -1;
      // From nowhere, down lands on the first row and up on the last.
      setActive((i) => (count ? (i < 0 ? (by > 0 ? 0 : count - 1) : (i + by + count) % count) : 0));
      return true;
    }
    const picks = e.key === "Enter" || (tabCompletes && e.key === "Tab");
    if (picks && !e.shiftKey && active >= 0 && active < count) {
      e.preventDefault();
      onPick(active, e.key === "Tab");
      return true;
    }
    return false;
  };

  const seat = (index: number): RefObject<T | null> | undefined =>
    index === active ? activeRef : undefined;

  const leave = () => setActive(-1);

  return { active, setActive, leave, onKey, seat };
}
