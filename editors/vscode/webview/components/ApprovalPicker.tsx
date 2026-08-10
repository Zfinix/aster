import { useRef, type ReactNode } from "react";
import { useDismiss } from "../lib/dismiss";
import type { PermissionMode } from "../../src/protocol";
import { AlertIcon, BookIcon, PencilIcon, ReviewIcon, ShieldIcon, TerminalIcon } from "./icons";

/**
 * Aster's `permissions.mode` values, passed straight through as
 * `--permission-mode`. Review is folded in as a mode entry so the picker is
 * the single place to choose what the next action does.
 */
const MODES: { mode: PermissionMode; label: string; detail: string; icon: ReactNode }[] = [
  {
    mode: "plan",
    label: "Plan",
    detail: "Explore the code and present a plan before editing",
    icon: <BookIcon />,
  },
  {
    mode: "manual",
    label: "Manual",
    detail: "Ask for approval before each edit",
    icon: <ShieldIcon />,
  },
  {
    mode: "auto",
    label: "Auto",
    detail: "Apply what passes the safety check, pause for anything risky",
    icon: <TerminalIcon />,
  },
  {
    mode: "edit",
    label: "Edit",
    detail: "Edit files without asking",
    icon: <PencilIcon />,
  },
  {
    mode: "yolo",
    label: "Yolo",
    detail: "No guardrails, unrestricted",
    icon: <AlertIcon />,
  },
];

export function ApprovalPicker({
  mode,
  onSelect,
  onClose,
  onReview,
}: {
  mode: PermissionMode;
  onSelect: (mode: PermissionMode) => void;
  onClose: () => void;
  onReview: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useDismiss(ref, onClose);

  return (
    <div className="picker" ref={ref} role="dialog" aria-label="Mode">
      <div className="picker-head">What should Aster do?</div>
      {MODES.map((m) => (
        <button
          key={m.mode}
          className="picker-row"
          data-selected={m.mode === mode}
          onClick={() => {
            onSelect(m.mode);
            onClose();
          }}
        >
          {m.icon}
          <span className="picker-body">
            <span className="picker-label">{m.label}</span>
            <span className="picker-detail">{m.detail}</span>
          </span>
          {m.mode === mode && <span className="picker-check">✓</span>}
        </button>
      ))}
      <button
        className="picker-row picker-add"
        onClick={() => {
          onReview();
          onClose();
        }}
      >
        <ReviewIcon />
        <span className="picker-body">
          <span className="picker-label">Review</span>
          <span className="picker-detail">Review the current diff for verified findings</span>
        </span>
      </button>
    </div>
  );
}

export function permissionLabel(mode: PermissionMode): string {
  return MODES.find((m) => m.mode === mode)?.label ?? mode;
}
