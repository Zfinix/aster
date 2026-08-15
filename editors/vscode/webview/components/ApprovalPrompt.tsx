import { useEffect, useRef, useState } from "react";
import type { ApprovalAsk } from "../lib/thread";
import { languageFromPath } from "../lib/highlight";
import { DiffView } from "./DiffView";

/** The preview opens with `edit <path>:`, which is the only clue to what
 *  language the lines below it are. */
const HEADER = /^\w+ (\S+?):?$/;

/** What the CLI is asking about, from the verb its preview opens with. */
function question(ask: ApprovalAsk): string {
  if (ask.kind === "plan") return "Approve this plan and start editing?";
  const verb = ask.preview.trimStart().split(/\s/)[0];
  if (verb === "run") return "Allow this command?";
  if (verb === "read") return "Allow reading this file?";
  if (verb === "edit") return "Allow this edit?";
  return "Allow this?";
}

/**
 * Shown inline while the CLI blocks on an approval. The choices are numbered
 * and answer to their digit: the keyboard is where the reader already is, and
 * a decision that stops the turn should not need the mouse.
 */
export function ApprovalPrompt({
  ask,
  onRespond,
  onRedirect,
}: {
  ask: ApprovalAsk;
  onRespond: (allow: boolean, always?: boolean) => void;
  /** Reject, and tell the agent what to do instead. */
  onRedirect: (instead: string) => void;
}) {
  const [instead, setInstead] = useState("");
  const ref = useRef<HTMLDivElement>(null);
  const lines = ask.preview.split("\n");
  const lang = languageFromPath(HEADER.exec(lines[0])?.[1]);

  // Remembering the answer needs something to remember it against, and only a
  // scoped ask (a directory, a credential) carries one.
  const options = [
    { label: "Yes", run: () => onRespond(true) },
    ...(ask.scope
      ? [{ label: `Yes, and don't ask again for ${ask.scope}`, run: () => onRespond(true, true) }]
      : []),
    { label: "No", run: () => onRespond(false) },
  ];

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Not while the reader is writing an alternative: "3" belongs in the box.
      if (document.activeElement?.tagName === "TEXTAREA") return;
      if (e.key === "Escape") {
        e.preventDefault();
        onRespond(false);
        return;
      }
      const n = Number(e.key);
      if (Number.isInteger(n) && n >= 1 && n <= options.length) {
        e.preventDefault();
        options[n - 1].run();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [options.length, onRespond]);

  const redirect = () => {
    const text = instead.trim();
    if (!text) return;
    setInstead("");
    onRedirect(text);
  };

  return (
    <div className="approval" ref={ref}>
      <div className="approval-head">{question(ask)}</div>
      <pre className="approval-preview">
        <DiffView lines={lines} lang={lang} />
      </pre>

      <div className="approval-options">
        {options.map((option, i) => (
          <button key={option.label} className="approval-option" onClick={option.run}>
            <span className="approval-key">{i + 1}</span>
            <span className="approval-label">{option.label}</span>
          </button>
        ))}
      </div>

      <textarea
        className="approval-instead"
        rows={1}
        value={instead}
        placeholder="Tell Aster what to do instead"
        onChange={(e) => setInstead(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            redirect();
          }
        }}
      />
      <div className="approval-hint">Esc to reject</div>
    </div>
  );
}
