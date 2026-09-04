import type { ReactNode } from "react";
import type { PermissionMode } from "../../src/protocol";
import {
  BoltIcon,
  FireIcon,
  ListOrderedIcon,
  PencilIcon,
  ReviewIcon,
  ShieldIcon,
} from "./icons";

/**
 * Aster's `permissions.mode` values, passed straight through as
 * `--permission-mode`. Review is folded in as a mode entry so the picker is
 * the single place to choose what the next action does.
 */
const MODES: {
  mode: PermissionMode;
  label: string;
  detail: string;
  icon: ReactNode;
  /** Worth the colour: this one hands over the machine. */
  danger?: boolean;
}[] = [
  {
    mode: "plan",
    label: "Plan",
    detail: "Explore the code and present a plan before editing",
    icon: <ListOrderedIcon />,
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
    icon: <BoltIcon />,
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
    detail: "Unrestricted access to the internet and every file on this machine",
    icon: <FireIcon />,
    danger: true,
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
  return (
    <div className="picker picker-modes" role="dialog" aria-label="Mode">
      <div className="picker-head">How should Aster&apos;s actions be approved?</div>
      {MODES.map((m) => (
        <button
          key={m.mode}
          className="picker-row"
          data-selected={m.mode === mode}
          data-danger={m.danger}
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

/** The composer's mode button wears the mode it is in, so yolo does not look
 *  like every other setting. */
export function permissionIcon(mode: PermissionMode): ReactNode {
  return MODES.find((m) => m.mode === mode)?.icon ?? <ShieldIcon />;
}
