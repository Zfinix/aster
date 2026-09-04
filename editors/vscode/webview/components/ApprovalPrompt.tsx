import { useEffect, useState } from "react";
import type { ApprovalAsk } from "../lib/thread";
import { useLayer } from "../lib/layer";
import { languageFromPath } from "../lib/highlight";
import { inEditor, post } from "../lib/host";
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
  const isPlan = ask.kind === "plan" && !!ask.markdown;
  const [editingPlan, setEditingPlan] = useState(false);
  const [planDraft, setPlanDraft] = useState(ask.markdown ?? "");
  const lines = ask.preview.split("\n");
  const lang = languageFromPath(HEADER.exec(lines[0])?.[1]);

  // A plan is a document, not a preview: it opens in a tab of its own so the
  // card is left holding the decision and nothing else.
  const openPlan = () =>
    post({
      type: "openUntitled",
      content: ask.markdown ?? "",
      lang: "markdown",
      title: "Plan",
      doc: true,
    });

  // Only an editor opens it unasked. A browser tab has to come from the click
  // below, or a popup blocker takes it and the plan opens nowhere at all.
  useEffect(() => {
    if (isPlan && inEditor) openPlan();
  }, [isPlan, ask.markdown]);

  // Remembering the answer needs something to remember it against, and only a
  // scoped ask (a directory, a credential) carries one.
  const options = [
    { label: "Yes", run: () => onRespond(true) },
    ...(ask.scope
      ? [{ label: `Yes, and don't ask again for ${ask.scope}`, run: () => onRespond(true, true) }]
      : []),
    { label: "No", run: () => onRespond(false) },
  ];

  // Not while the reader is writing an alternative: "3" and Escape belong to
  // the box. A menu open above the card takes Escape first.
  const typing = () => document.activeElement?.tagName === "TEXTAREA";
  useLayer(() => {
    if (!typing()) onRespond(false);
  });

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (typing()) return;
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
    if (isPlan && editingPlan) {
      const draft = planDraft.trim();
      if (!draft) return;
      setEditingPlan(false);
      onRedirect(`Revised plan:\n${draft}`);
      return;
    }
    const text = instead.trim();
    if (!text) return;
    setInstead("");
    onRedirect(text);
  };

  return (
    <div className="approval">
      <div className="approval-head">{question(ask)}</div>
      {isPlan && editingPlan && (
        <textarea
          className="approval-plan-edit"
          rows={Math.min(16, planDraft.split("\n").length + 1)}
          value={planDraft}
          onChange={(e) => setPlanDraft(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              redirect();
            }
          }}
        />
      )}
      {!isPlan && (
        <pre className="approval-preview">
          <DiffView lines={lines} lang={lang} />
        </pre>
      )}

      <div className="approval-options">
        {isPlan && !editingPlan && (
          <button className="approval-option" onClick={openPlan}>
            <span className="approval-key">↗</span>
            <span className="approval-label">
              {inEditor ? "Read the plan" : "Read the plan in a tab"}
            </span>
          </button>
        )}
        {isPlan && (
          <button className="approval-option" onClick={() => setEditingPlan((v) => !v)}>
            <span className="approval-key">✎</span>
            <span className="approval-label">{editingPlan ? "Done editing" : "Edit plan"}</span>
          </button>
        )}
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
        placeholder={isPlan ? "Or tell Aster what to change" : "Tell Aster what to do instead"}
        onChange={(e) => setInstead(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            redirect();
          }
        }}
      />
      <div className="approval-hint">
        {isPlan && editingPlan ? "Cmd+Enter to send the revised plan" : "Esc to reject"}
      </div>
    </div>
  );
}
