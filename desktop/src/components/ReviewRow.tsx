import { useEffect, useRef, useState } from "react";
import { Dropdown } from "./chrome";
import { DotsIcon } from "./icons";
import { latestReview, type Conversation } from "../lib/session";
import { severityOf, SEV_RANK } from "../lib/severity";
import type { Severity } from "../lib/types";

const SEV_COLOR: Record<Severity, string> = {
  critical: "var(--red)",
  high: "var(--coral)",
  medium: "var(--amber)",
  low: "#7aa2f7",
  info: "var(--faint)",
};

function dotColor(c: Conversation): string {
  const review = latestReview(c);
  if (!review) return "var(--faint)";
  if (review.findings.length) {
    const top = review.findings
      .map((f) => severityOf(f.severity))
      .sort((a, b) => SEV_RANK[a] - SEV_RANK[b])[0];
    return SEV_COLOR[top];
  }
  return review.status === "done" ? "var(--green)" : "var(--faint)";
}

interface Props {
  convo: Conversation;
  active: boolean;
  onOpen: () => void;
  onRename: (title: string) => void;
  onDelete: () => void;
  onRerun: () => void;
  onCopyBrief: () => void;
}

export function ReviewRow({
  convo,
  active,
  onOpen,
  onRename,
  onDelete,
  onRerun,
  onCopyBrief,
}: Props) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(convo.title);
  const inputRef = useRef<HTMLInputElement>(null);

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
      <div className="thread" data-active={active}>
        <span className="t-dot" style={{ background: dotColor(convo) }} />
        <input
          ref={inputRef}
          className="thread-rename"
          value={draft}
          spellCheck={false}
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
    <div className="thread" data-active={active}>
      <button className="thread-open" onClick={onOpen}>
        <span className="t-dot" style={{ background: dotColor(convo) }} />
        <span className="t-name">{convo.title || "Working tree"}</span>
        <span className="t-meta">{convo.whenLabel}</span>
      </button>
      <span className="t-menu">
        <Dropdown
          trigger={() => (
            <span className="ghost-icon" aria-label="Review options" title="Review options">
              <DotsIcon size={14} />
            </span>
          )}
          options={[
            { value: "rename", label: "Rename" },
            { value: "rerun", label: "Re-run review" },
            { value: "brief", label: "Copy fix brief" },
            { value: "delete", label: "Delete", danger: true },
          ]}
          direction="down"
          align="right"
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
