import { useEffect, useRef, useState } from "react";
import { Dropdown } from "./Dropdown";
import { MoreIcon } from "./icons";
import { latestReview, type Conversation } from "../lib/session";
import { severityOf, SEV_RANK } from "../lib/severity";
import type { Severity } from "../lib/types";

const SEV_COLOR: Record<Severity, string> = {
  critical: "var(--sev-critical)",
  high: "var(--sev-high)",
  medium: "var(--sev-medium)",
  low: "var(--sev-low)",
  info: "var(--sev-info)",
};

function dotColor(c: Conversation): string | null {
  const review = latestReview(c);
  if (!review) return null;
  if (review.findings.length) {
    const top = review.findings
      .map((f) => severityOf(f.severity))
      .sort((a, b) => SEV_RANK[a] - SEV_RANK[b])[0];
    return SEV_COLOR[top];
  }
  return review.status === "done" ? "var(--diff-add)" : "var(--fg-dim)";
}

export function SessionRow({
  convo,
  active,
  onOpen,
  onRename,
  onDelete,
  onRerun,
  onCopyBrief,
}: {
  convo: Conversation;
  active: boolean;
  onOpen: () => void;
  onRename: (title: string) => void;
  onDelete: () => void;
  onRerun: () => void;
  onCopyBrief: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [menu, setMenu] = useState(false);
  const [draft, setDraft] = useState(convo.title);
  const inputRef = useRef<HTMLInputElement>(null);
  const dot = dotColor(convo);

  useEffect(() => {
    if (editing) inputRef.current?.select();
  }, [editing]);

  const commit = () => {
    const t = draft.trim();
    if (t && t !== convo.title) onRename(t);
    setEditing(false);
  };

  if (editing) {
    return (
      <div className="session-row" data-active={active}>
        <input
          ref={inputRef}
          className="session-rename"
          value={draft}
          spellCheck={false}
          aria-label="Conversation name"
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") commit();
            if (e.key === "Escape") {
              setDraft(convo.title);
              setEditing(false);
            }
          }}
        />
      </div>
    );
  }

  return (
    <div className="session-row" data-active={active} data-menu={menu}>
      <button type="button" className="session-open" title={convo.title} onClick={onOpen}>
        {dot && <span className="session-dot" style={{ background: dot }} />}
        <span className="session-title">{convo.title || "Working tree"}</span>
      </button>
      <span className="session-when">{convo.whenLabel}</span>
      <span className="session-actions">
        <Dropdown
          triggerClass="icon-btn"
          label="Conversation options"
          trigger={() => <MoreIcon />}
          options={[
            { value: "rename", label: "Rename" },
            { value: "rerun", label: "Run the review again" },
            { value: "brief", label: "Copy fix brief" },
            { value: "delete", label: "Delete", danger: true },
          ]}
          dir="down"
          align="right"
          onOpenChange={setMenu}
          onSelect={(v) => {
            if (v === "rename") {
              setDraft(convo.title);
              setEditing(true);
            } else if (v === "rerun") onRerun();
            else if (v === "brief") onCopyBrief();
            else if (v === "delete") onDelete();
          }}
        />
      </span>
    </div>
  );
}
