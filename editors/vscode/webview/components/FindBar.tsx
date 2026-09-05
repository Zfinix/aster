import { useCallback, useEffect, useRef, useState } from "react";
import { clearMatches, collectMatches, showMatches } from "../lib/find";
import { ArrowDownIcon, ArrowUpIcon, XIcon } from "./icons";

/** Find in the conversation, for the sidebar where the editor has no find
 *  widget to lend. Cmd+F opens it, Enter steps, Escape closes. */
export function FindBar() {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [at, setAt] = useState(0);
  const [total, setTotal] = useState(0);
  const input = useRef<HTMLInputElement>(null);

  const refresh = useCallback((text: string, index: number) => {
    const thread = document.querySelector(".thread");
    const matches = thread && text ? collectMatches(thread, text) : [];
    const current = matches.length ? ((index % matches.length) + matches.length) % matches.length : 0;
    setTotal(matches.length);
    setAt(current);
    if (matches.length) {
      showMatches(matches, current);
    } else {
      clearMatches();
    }
  }, []);

  const close = useCallback(() => {
    setOpen(false);
    clearMatches();
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "f" || !(e.metaKey || e.ctrlKey) || e.altKey || e.shiftKey) return;
      e.preventDefault();
      // The editor forwards the keystroke to its own find otherwise, which
      // lands in whatever text editor is open rather than here.
      e.stopPropagation();
      setOpen(true);
      requestAnimationFrame(() => input.current?.select());
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    if (!open) return;
    refresh(query, 0);
  }, [open, query, refresh]);

  // A streaming reply moves the text under the matches, so they follow it.
  useEffect(() => {
    if (!open || !query) return;
    const thread = document.querySelector(".thread");
    if (!thread) return;
    let frame = 0;
    const observer = new MutationObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => refresh(query, at));
    });
    observer.observe(thread, { childList: true, characterData: true, subtree: true });
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  }, [open, query, at, refresh]);

  if (!open) return null;

  const step = (by: number) => refresh(query, at + by);
  const count = !query ? "" : total ? `${at + 1} of ${total}` : "No results";

  return (
    <div className="find-bar" role="search">
      <input
        ref={input}
        value={query}
        placeholder="Find in conversation"
        aria-label="Find in conversation"
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            step(e.shiftKey ? -1 : 1);
          } else if (e.key === "Escape") {
            e.preventDefault();
            close();
          }
        }}
      />
      <span className="find-count" data-empty={Boolean(query) && !total ? "" : undefined}>
        {count}
      </span>
      <button
        className="icon-btn"
        onClick={() => step(-1)}
        disabled={!total}
        title="Previous match"
        aria-label="Previous match"
      >
        <ArrowUpIcon />
      </button>
      <button
        className="icon-btn"
        onClick={() => step(1)}
        disabled={!total}
        title="Next match"
        aria-label="Next match"
      >
        <ArrowDownIcon />
      </button>
      <button className="icon-btn" onClick={close} title="Close" aria-label="Close find">
        <XIcon />
      </button>
    </div>
  );
}
