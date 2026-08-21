/** Drawn on a 16 grid at a 1.25 stroke: horizontals, verticals and 45s, rounded
 *  off, with a 3-unit minimum gap wherever two shapes overlap. */
const GRID = 16;
/** Matches the `--glyph` column the layout reserves. The stroke is in grid
 *  units, so it scales with the box rather than being re-hinted per size. */
const SIZE = 14;

const base = {
  width: SIZE,
  height: SIZE,
  viewBox: `0 0 ${GRID} ${GRID}`,
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.25,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  "aria-hidden": true,
};

/** The mark itself: four axes, the horizontal and vertical run to the edge of
 *  the grid, and the diagonals stand off the centre so the junction stays open
 *  at 16px, the way the logo's centre block holds them apart. */
export function AsterIcon() {
  return (
    <svg {...base}>
      <path d="M1 8h14M8 1v14" />
      <path d="M10.3 5.7L12.9 3.1M5.7 5.7L3.1 3.1M10.3 10.3L12.9 12.9M5.7 10.3L3.1 12.9" />
    </svg>
  );
}

export function SendIcon() {
  return (
    <svg {...base}>
      <path d="M8 13.9 V2.1 M3.1 7 L8 2.1 l4.9 4.9" />
    </svg>
  );
}

/** The slash that opens the command menu, boxed the way its shortcut reads. */
export function CommandIcon() {
  return (
    <svg {...base}>
      <rect x="1.6" y="1.6" width="12.8" height="12.8" rx="2.8" />
      <path d="M9.7 4.8 L6.3 11.2" />
    </svg>
  );
}

export function ShieldIcon() {
  return (
    <svg {...base}>
      <path d="M8 1.5 l5.6 2 v4.7 c0 3 -2.2 5.3 -5.6 6.3 c-3.4 -1 -5.6 -3.3 -5.6 -6.3 V3.5 L8 1.5 z" />
    </svg>
  );
}

export function ChevronIcon({ open }: { open: boolean }) {
  return (
    <svg {...base} style={{ transform: open ? "rotate(90deg)" : undefined }}>
      <path d="M5.7 2.5 L11.2 8 l-5.5 5.5" />
    </svg>
  );
}

/** Points at the menu it opens, which sits above the composer. */
export function CaretUpIcon({ open }: { open: boolean }) {
  return (
    <svg {...base} style={{ transform: open ? "rotate(180deg)" : undefined }}>
      <path d="M2.7 10.7 L8 5.3 l5.3 5.3" />
    </svg>
  );
}

export function ReviewIcon() {
  return (
    <svg {...base}>
      <path d="M1.9 3.7 h12.2 M1.9 8 h7.9 M1.9 12.3 h4.7" />
    </svg>
  );
}

export function FileIcon() {
  return (
    <svg {...base}>
      <path d="M9.4 1.4 H4.2 A1.6 1.6 0 0 0 2.6 3 v10 a1.6 1.6 0 0 0 1.6 1.6 h7.7 a1.6 1.6 0 0 0 1.6 -1.6 V5.4 L9.4 1.4 z" />
      <path d="M9.4 1.4 v4.1 h4.1" />
    </svg>
  );
}

export function LayersIcon() {
  return (
    <svg {...base}>
      <path d="M8 1.5 L14.5 4.8 L8 8.1 L1.5 4.8 L8 1.5 z" />
      <path d="M1.5 8 L8 11.3 L14.5 8 M1.5 11.2 L8 14.5 L14.5 11.2" />
    </svg>
  );
}

export function FolderIcon() {
  return (
    <svg {...base}>
      <path d="M1.5 5 V3.9 a1.4 1.4 0 0 1 1.4 -1.4 h3.4 l1.9 2.5 h6 a1.4 1.4 0 0 1 1.4 1.4 v6 a1.4 1.4 0 0 1 -1.4 1.4 H2.9 a1.4 1.4 0 0 1 -1.4 -1.4 V5 z" />
    </svg>
  );
}

export function SearchIcon() {
  return (
    <svg {...base}>
      <circle cx="6.7" cy="6.7" r="4.8" />
      <path d="M10.1 10.1 l4.3 4.3" />
    </svg>
  );
}

/** Tip at the bottom left, cap at the top right: the one diagonal every icon
 *  that could run either way follows. */
export function PencilIcon() {
  return (
    <svg {...base}>
      <path d="M2 14 l1 -3.8 l7.8 -7.8 a2 2 0 0 1 2.9 2.9 L5.9 13 L2 14 z" />
      <path d="M3 10.1 L5.9 13 M9 4.1 l2.9 2.9" />
    </svg>
  );
}

export function TrashIcon() {
  return (
    <svg {...base}>
      <path d="M2 4.2 h11.9" />
      <path d="M6.3 4.2 V2.9 a1 1 0 0 1 1 -1 h1.5 a1 1 0 0 1 1 1 v1.3" />
      <path d="M3.4 4.2 l0.7 9.4 a1.1 1.1 0 0 0 1.1 1 h5.5 a1.1 1.1 0 0 0 1.1 -1 l0.7 -9.4" />
      <path d="M6.5 7.4 v4.2 M9.5 7.4 v4.2" />
    </svg>
  );
}

export function TerminalIcon() {
  return (
    <svg {...base}>
      <rect x="1.5" y="2.6" width="13" height="10.9" rx="1.7" />
      <path d="M4.8 6.3 L6.7 8.2 L4.8 10.1 M8.4 10.1 h2.8" />
    </svg>
  );
}

/** Two folds off the seam: at 16px they are what keeps the silhouette from
 *  reading as a plain capsule. */
export function BrainIcon() {
  return (
    <svg {...base}>
      <path d="M8 2.6A2.2 2.2 0 004.4 4 2 2 0 002.4 6.6 1.8 1.8 0 002.6 9.6 2.2 2.2 0 004.6 12.2 2.4 2.4 0 008 13.4" />
      <path d="M8 2.6A2.2 2.2 0 0111.6 4 2 2 0 0113.6 6.6 1.8 1.8 0 0113.4 9.6 2.2 2.2 0 0111.4 12.2 2.4 2.4 0 018 13.4" />
      <path d="M8 2.6v10.8M4.4 6.4H6M10 9.6h1.6" />
    </svg>
  );
}

export function BookIcon() {
  return (
    <svg {...base}>
      <path d="M8 4.2 a1.9 1.9 0 0 0 -1.9 -1.9 H2.2 a0.9 0.9 0 0 0 -0.9 0.9 v8.5 a0.9 0.9 0 0 0 0.9 0.9 h3.8 A1.9 1.9 0 0 1 8 14.4" />
      <path d="M8 4.2 a1.9 1.9 0 0 1 1.9 -1.9 h3.8 a0.9 0.9 0 0 1 0.9 0.9 v8.5 a0.9 0.9 0 0 1 -0.9 0.9 H9.9 A1.9 1.9 0 0 0 8 14.4" />
      <path d="M8 4.2 V14.4" />
    </svg>
  );
}

export function AlertIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="7" />
      <path d="M8 4.7 v4.3 M8 11.5 h0" />
    </svg>
  );
}

export function ExternalIcon() {
  return (
    <svg {...base}>
      <path d="M9.3 2 h4.7 v4.7 M14 2 L7.8 8.2" />
      <path d="M12.7 9.7 v3.2 a1.5 1.5 0 0 1 -1.5 1.5 H3.1 A1.5 1.5 0 0 1 1.6 12.9 V4.8 a1.5 1.5 0 0 1 1.5 -1.5 h3.2" />
    </svg>
  );
}

export function SpinnerIcon() {
  return (
    <svg {...base} className="spinner-icon">
      <circle cx="8" cy="8" r="7" strokeDasharray="27 12" />
    </svg>
  );
}

/** A speech bubble with a plus: a pencil reads as "edit this one", and what
 *  the button does is start another. */
export function NewChatIcon() {
  return (
    <svg {...base}>
      <path d="M14.2 3.5 a1.5 1.5 0 0 0 -1.5 -1.5 H3.3 a1.5 1.5 0 0 0 -1.5 1.5 v7 a1.5 1.5 0 0 0 1.5 1.5 h1.1 v2.6 L8 12.1 h4.7 a1.5 1.5 0 0 0 1.5 -1.5 V3.5 z" />
      <path d="M8 5.2 v3.6 M6.2 7 h3.6" />
    </svg>
  );
}

/** A clock glyph for reopening a saved session. */
export function HistoryIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="7" />
      <path d="M8 4.7 v3.5 l2.8 1.6" />
    </svg>
  );
}

/** Opens the add menu: files from disk, or a mention of one in the repo. */
export function PlusIcon() {
  return (
    <svg {...base}>
      <path d="M8 2.2 v11.5 M2.2 8 h11.5" />
    </svg>
  );
}

/** The plan's step marks: one ring, filled in differently per state, so a
 *  column of them reads as one control rather than five unrelated glyphs. */
export function CircleIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="7" />
    </svg>
  );
}

export function CircleDotIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="7" />
      <circle cx="8" cy="8" r="2" fill="currentColor" stroke="none" />
    </svg>
  );
}

export function CircleCheckIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="7" fill="currentColor" fillOpacity="0.16" />
      <path d="M4.9 8.1 l2.1 2.1 l3.8 -3.8" />
    </svg>
  );
}

export function CircleSkipIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="7" />
      <path d="M6.1 5.8 L8.3 8 l-2.2 2.2 M10.1 5.8 v4.5" />
    </svg>
  );
}

export function CircleXIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="7" fill="currentColor" fillOpacity="0.14" />
      <path d="M6 6 l4.1 4.1 M10 6 l-4.1 4.1" />
    </svg>
  );
}

export function UploadIcon() {
  return (
    <svg {...base}>
      <path d="M8 10.8 V2.2 M4.2 6.1 L8 2.2 l3.8 3.8" />
      <path d="M2.9 9.5 v3 a1.5 1.5 0 0 0 1.5 1.5 h7.2 a1.5 1.5 0 0 0 1.5 -1.5 V9.5" />
    </svg>
  );
}

export function ArrowUpIcon() {
  return (
    <svg {...base}>
      <path d="M8 13.9 V2.1 M3.1 7 L8 2.1 l4.9 4.9" />
    </svg>
  );
}

export function ArrowDownIcon() {
  return (
    <svg {...base}>
      <path d="M8 2.1 v11.7 M3.1 9 L8 13.9 l4.9 -4.9" />
    </svg>
  );
}

export function ArrowLeftIcon() {
  return (
    <svg {...base}>
      <path d="M13.9 8 h-11.7 M7 3.1 L2.1 8 l4.9 4.9" />
    </svg>
  );
}

export function ArrowRightIcon() {
  return (
    <svg {...base}>
      <path d="M2.1 8 h11.7 M9 3.1 L13.9 8 l-4.9 4.9" />
    </svg>
  );
}

/** Ascends bottom left to top right, the direction every arrow in the set that
 *  could run either way takes. */
export function ArrowUpRightIcon() {
  return (
    <svg {...base}>
      <path d="M3.1 12.9 L12.9 3.1 M5.2 3.1 h7.7 v7.7" />
    </svg>
  );
}

export function ArrowDownLeftIcon() {
  return (
    <svg {...base}>
      <path d="M12.9 3.1 L3.1 12.9 M10.8 12.9 H3.1 V5.2" />
    </svg>
  );
}

export function UndoIcon() {
  return (
    <svg {...base}>
      <path d="M5.7 3.5 L2 7.1 l3.6 3.6" />
      <path d="M2 7.1 h8.1 a3.6 3.6 0 0 1 0 7.2 H6.3" />
    </svg>
  );
}

export function RedoIcon() {
  return (
    <svg {...base}>
      <path d="M10.3 3.5 l3.6 3.6 l-3.6 3.6" />
      <path d="M14 7.1 H5.9 a3.6 3.6 0 0 0 0 7.2 h3.8" />
    </svg>
  );
}

export function RefreshIcon() {
  return (
    <svg {...base}>
      <path d="M14 8 a6 6 0 0 1 -10.2 4.2 L2 10.6 M2 14 v-3.4 h3.4" />
      <path d="M2 8 a6 6 0 0 1 10.2 -4.2 l1.7 1.6 M14 2 v3.4 h-3.4" />
    </svg>
  );
}

export function PlayIcon() {
  return (
    <svg {...base}>
      <path d="M4.8 2.9 l7.7 5.1 L4.8 13.1 V2.9 z" />
    </svg>
  );
}

export function PauseIcon() {
  return (
    <svg {...base}>
      <path d="M5.9 2.6 v10.9 M10.1 2.6 v10.9" />
    </svg>
  );
}

export function StopIcon() {
  return (
    <svg {...base}>
      <rect x="2.7" y="2.7" width="10.7" height="10.7" rx="2.1" />
    </svg>
  );
}

export function SquareIcon() {
  return (
    <svg {...base}>
      <rect x="1.6" y="1.6" width="12.8" height="12.8" rx="2.8" />
    </svg>
  );
}

export function SkipIcon() {
  return (
    <svg {...base}>
      <path d="M3.7 2.9 l6.8 5.1 L3.7 13.1 V2.9 z" />
      <path d="M12.9 2.6 v10.9" />
    </svg>
  );
}

export function CheckIcon() {
  return (
    <svg {...base}>
      <path d="M2.5 8.6 l3.8 3.8 l7.2 -8.3" />
    </svg>
  );
}

export function XIcon() {
  return (
    <svg {...base}>
      <path d="M3.3 3.3 l9.4 9.4 M12.7 3.3 l-9.4 9.4" />
    </svg>
  );
}

export function MinusIcon() {
  return (
    <svg {...base}>
      <path d="M2.2 8 h11.5" />
    </svg>
  );
}

export function DotIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="2.3" fill="currentColor" stroke="none" />
    </svg>
  );
}

/** The "more" dot, one of three sizes the set keeps: bigger than a line's
 *  terminal, smaller than a dot that stands on its own. */
export function MoreIcon() {
  return (
    <svg {...base}>
      <circle cx="3" cy="8" r="1.1" fill="currentColor" stroke="none" />
      <circle cx="8" cy="8" r="1.1" fill="currentColor" stroke="none" />
      <circle cx="13" cy="8" r="1.1" fill="currentColor" stroke="none" />
    </svg>
  );
}

export function MoreVerticalIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="3" r="1.1" fill="currentColor" stroke="none" />
      <circle cx="8" cy="8" r="1.1" fill="currentColor" stroke="none" />
      <circle cx="8" cy="13" r="1.1" fill="currentColor" stroke="none" />
    </svg>
  );
}

export function MenuIcon() {
  return (
    <svg {...base}>
      <path d="M1.9 4.2 h12.2 M1.9 8 h12.2 M1.9 11.8 h12.2" />
    </svg>
  );
}

export function InfoIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="7" />
      <path d="M8 7.4 v3.9 M8 4.7 h0" />
    </svg>
  );
}

export function QuestionIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="7" />
      <path d="M6 6.2 a2.1 2.1 0 1 1 3.4 2 c-0.7 0.6 -1.4 1 -1.4 2 M8 11.9 h0" />
    </svg>
  );
}

export function WarningIcon() {
  return (
    <svg {...base}>
      <path d="M8 1.8 l6.2 10.8 a1.2 1.2 0 0 1 -1 1.8 H2.8 a1.2 1.2 0 0 1 -1 -1.8 L8 1.8 z" />
      <path d="M8 6.5 v3.3 M8 12.2 h0" />
    </svg>
  );
}

export function BellIcon() {
  return (
    <svg {...base}>
      <path d="M12.5 6.5 a4.5 4.5 0 1 0 -9 0 c0 2.8 -0.6 4.2 -1.3 5 a0.5 0.5 0 0 0 0.4 0.9 h10.7 a0.5 0.5 0 0 0 0.4 -0.9 c-0.6 -0.9 -1.3 -2.2 -1.3 -5 z" />
      <path d="M6.5 14.5 a1.7 1.7 0 0 0 3 0" />
    </svg>
  );
}

export function FilePlusIcon() {
  return (
    <svg {...base}>
      <path d="M9.4 1.4 H4.2 A1.6 1.6 0 0 0 2.6 3 v10 a1.6 1.6 0 0 0 1.6 1.6 h7.7 a1.6 1.6 0 0 0 1.6 -1.6 V5.4 L9.4 1.4 z" />
      <path d="M9.4 1.4 v4.1 h4.1" />
      <path d="M8 8.4 v3.6 M6.2 10.2 h3.6" />
    </svg>
  );
}

export function FileMinusIcon() {
  return (
    <svg {...base}>
      <path d="M9.4 1.4 H4.2 A1.6 1.6 0 0 0 2.6 3 v10 a1.6 1.6 0 0 0 1.6 1.6 h7.7 a1.6 1.6 0 0 0 1.6 -1.6 V5.4 L9.4 1.4 z" />
      <path d="M9.4 1.4 v4.1 h4.1" />
      <path d="M6.2 10.2 h3.6" />
    </svg>
  );
}

export function FileCodeIcon() {
  return (
    <svg {...base}>
      <path d="M9.4 1.4 H4.2 A1.6 1.6 0 0 0 2.6 3 v10 a1.6 1.6 0 0 0 1.6 1.6 h7.7 a1.6 1.6 0 0 0 1.6 -1.6 V5.4 L9.4 1.4 z" />
      <path d="M9.4 1.4 v4.1 h4.1" />
      <path d="M6.5 9 L5 10.5 l1.5 1.5 M9.5 9 l1.5 1.5 l-1.5 1.5" />
    </svg>
  );
}

export function FolderOpenIcon() {
  return (
    <svg {...base}>
      <path d="M1.5 12.4 V3.9 a1.4 1.4 0 0 1 1.4 -1.4 h3.4 l1.9 2.5 h6 a1.4 1.4 0 0 1 1.4 1.4 v1" />
      <path d="M1.5 12.4 l1.8 -4.7 a1.4 1.4 0 0 1 1.3 -0.9 h9.7 a1.4 1.4 0 0 1 1.3 1.9 l-1.6 4.2 a1.4 1.4 0 0 1 -1.3 0.9 H2.9 a1.4 1.4 0 0 1 -1.4 -1.4 z" />
    </svg>
  );
}

export function CodeIcon() {
  return (
    <svg {...base}>
      <path d="M5.7 4.2 L1.8 8 l3.8 3.8 M10.3 4.2 L14.2 8 l-3.8 3.8" />
    </svg>
  );
}

export function BracesIcon() {
  return (
    <svg {...base}>
      <path d="M6.3 1.8 h-0.9 a1.7 1.7 0 0 0 -1.7 1.7 v2.8 A1.7 1.7 0 0 1 2 8 a1.7 1.7 0 0 1 1.7 1.7 v2.8 a1.7 1.7 0 0 0 1.7 1.7 h0.9" />
      <path d="M9.7 1.8 h0.9 a1.7 1.7 0 0 1 1.7 1.7 v2.8 A1.7 1.7 0 0 0 14 8 a1.7 1.7 0 0 0 -1.7 1.7 v2.8 a1.7 1.7 0 0 1 -1.7 1.7 h-0.9" />
    </svg>
  );
}

export function HashIcon() {
  return (
    <svg {...base}>
      <path d="M6.3 1.8 L4.9 14.2 M11.3 1.8 L9.9 14.2 M1.8 5.7 h12.4 M1.8 10.3 h12.4" />
    </svg>
  );
}

export function AtIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="2.9" />
      <path d="M10.9 5.1 v3.6 a2.2 2.2 0 0 0 3.6 1.8 A6.6 6.6 0 1 0 8 14.6" />
    </svg>
  );
}

/** Plus over minus: the two halves of a diff, stacked the way the review pane
 *  stacks them. */
export function DiffIcon() {
  return (
    <svg {...base}>
      <path d="M8 2 v6.8 M4.6 5.4 h6.8 M4.6 13.3 h6.8" />
    </svg>
  );
}

export function GitBranchIcon() {
  return (
    <svg {...base}>
      <circle cx="4.2" cy="3.1" r="1.8" />
      <circle cx="4.2" cy="12.9" r="1.8" />
      <circle cx="11.8" cy="3.1" r="1.8" />
      <path d="M4.2 4.9 v6.2" />
      <path d="M11.8 4.9 v1.2 a2.8 2.8 0 0 1 -2.8 2.8 H6.9 a2.8 2.8 0 0 0 -2.8 2.8" />
    </svg>
  );
}

export function GitCommitIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="2.8" />
      <path d="M1.8 8 h3.4 M10.8 8 h3.4" />
    </svg>
  );
}

export function GitMergeIcon() {
  return (
    <svg {...base}>
      <circle cx="4.2" cy="3.1" r="1.8" />
      <circle cx="4.2" cy="12.9" r="1.8" />
      <circle cx="11.8" cy="8" r="1.8" />
      <path d="M4.2 4.9 v6.2" />
      <path d="M10 8 H8.4 a4.3 4.3 0 0 1 -4.3 -4.3" />
    </svg>
  );
}

export function GitPullRequestIcon() {
  return (
    <svg {...base}>
      <circle cx="4.2" cy="3.1" r="1.8" />
      <circle cx="4.2" cy="12.9" r="1.8" />
      <circle cx="11.8" cy="12.9" r="1.8" />
      <path d="M4.2 4.9 v6.2 M11.8 11.1 V6.3 a2.8 2.8 0 0 0 -2.8 -2.8 H7.1" />
      <path d="M8.9 1.8 L7.1 3.5 l1.7 1.7" />
    </svg>
  );
}

export function SparkleIcon() {
  return (
    <svg {...base}>
      <path d="M8 1.5 l1.8 4.7 l4.7 1.8 l-4.7 1.8 L8 14.5 l-1.8 -4.7 L1.5 8 l4.7 -1.8 L8 1.5 z" />
    </svg>
  );
}

/** The wand runs on the set's diagonal; the spark sits at its top right, where
 *  the smaller object always goes. */
export function WandIcon() {
  return (
    <svg {...base}>
      <path d="M2 14l6.6-6.6" />
      <path d="M4.4 11.6l2 2" />
      <path d="M12.4 1l.85 1.75L15 3.6l-1.75.85L12.4 6.2l-.85-1.75L9.8 3.6l1.75-.85L12.4 1z" />
    </svg>
  );
}

/** Pins run to the edge of the grid, the way a monospace stem fills its
 *  column. */
export function ChipIcon() {
  return (
    <svg {...base}>
      <rect x="3.9" y="3.9" width="8.1" height="8.1" rx="1.5" />
      <path d="M6.3 3.9 V1.5 M9.7 3.9 V1.5 M6.3 14.5 v-2.5 M9.7 14.5 v-2.5 M3.9 6.3 H1.5 M3.9 9.7 H1.5 M14.5 6.3 h-2.5 M14.5 9.7 h-2.5" />
    </svg>
  );
}

export function CubeIcon() {
  return (
    <svg {...base}>
      <path d="M8 1.5 l6 3.3 v6.4 L8 14.5 L2 11.2 V4.8 L8 1.5 z" />
      <path d="M2 4.8 L8 8.1 L14 4.8 M8 8.1 v6.4" />
    </svg>
  );
}

/** Parallel threads: used wherever the transcript has to say how many agents
 *  are in flight. */
export function ThreadsIcon() {
  return (
    <svg {...base}>
      <path d="M3.4 5.4v7.6M8 5.4v7.6M12.6 5.4v7.6" />
      <circle cx="3.4" cy="3.4" r="1.9" />
      <circle cx="8" cy="3.4" r="1.9" />
      <circle cx="12.6" cy="3.4" r="1.9" />
    </svg>
  );
}

export function BoltIcon() {
  return (
    <svg {...base}>
      <path d="M9.3 1.5 L3 9.3 h4.4 l-0.6 5.2 l6.3 -7.8 H8.6 l0.6 -5.2 z" />
    </svg>
  );
}

/** Thinking effort: the needle swings bottom left to top right with the
 *  budget. */
export function GaugeIcon() {
  return (
    <svg {...base}>
      <path d="M1.8 11.8 a6.6 6.6 0 1 1 12.4 0" />
      <path d="M8 11.8 l3.3 -4.2" />
      <path d="M1.8 11.8 h2.3 M11.8 11.8 h2.3" />
    </svg>
  );
}

export function HourglassIcon() {
  return (
    <svg {...base}>
      <path d="M3.3 1.8 h9.4 M3.3 14.2 h9.4" />
      <path d="M4.6 1.8 v2.5 L8 8 l-3.4 3.7 v2.5 M11.4 1.8 v2.5 L8 8 l3.4 3.7 v2.5" />
    </svg>
  );
}

export function TargetIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="7" />
      <circle cx="8" cy="8" r="3.3" />
      <circle cx="8" cy="8" r="0.9" fill="currentColor" stroke="none" />
    </svg>
  );
}

export function CompassIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="7" />
      <path d="M11.3 4.7 L9.5 9.5 L4.7 11.3 l1.8 -4.8 l4.8 -1.8 z" />
    </svg>
  );
}

export function DatabaseIcon() {
  return (
    <svg {...base}>
      <ellipse cx="8" cy="3.7" rx="5.8" ry="2.2" />
      <path d="M2.2 3.7 v8.5 a5.8 2.2 0 0 0 11.5 0 V3.7" />
      <path d="M2.2 8 a5.8 2.2 0 0 0 11.5 0" />
    </svg>
  );
}

export function ServerIcon() {
  return (
    <svg {...base}>
      <rect x="1.5" y="2" width="13" height="5.1" rx="1.5" />
      <rect x="1.5" y="8.9" width="13" height="5.1" rx="1.5" />
      <path d="M4.4 4.6 h0 M4.4 11.4 h0" />
    </svg>
  );
}

export function CloudIcon() {
  return (
    <svg {...base}>
      <path d="M4.2 13.1 h7.5 a3.2 3.2 0 0 0 0.4 -6.4 a4.6 4.6 0 0 0 -8.7 -1 a3.2 3.2 0 0 0 0.9 7.4 z" />
    </svg>
  );
}

export function GlobeIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="7" />
      <path d="M1 8h14" />
      <path d="M8 1a10.8 10.8 0 010 14 10.8 10.8 0 010-14z" />
    </svg>
  );
}

export function SignalIcon() {
  return (
    <svg {...base}>
      <path d="M2 14 v-3 M6 14 V8.2 M10 14 V4.9 M14 14 V2" />
    </svg>
  );
}

/** MCP servers plug in; the two pins run off the top edge the way the chip's
 *  do. */
export function PlugIcon() {
  return (
    <svg {...base}>
      <path d="M5.9 1.5 v3.7 M10.1 1.5 v3.7" />
      <path d="M3.3 5.2 h9.4 v2.6 a4.7 4.7 0 0 1 -9.4 0 V5.2 z" />
      <path d="M8 12.5 v2" />
    </svg>
  );
}

export function PuzzleIcon() {
  return (
    <svg {...base}>
      <path d="M9.7 2.2 v-0.4 a1.7 1.7 0 0 0 -3.4 0 v0.4 H3.6 a1.4 1.4 0 0 0 -1.4 1.4 v2.7 h0.4 a1.7 1.7 0 0 1 0 3.4 h-0.4 v2.7 a1.4 1.4 0 0 0 1.4 1.4 h2.7 v-0.4 a1.7 1.7 0 0 1 3.4 0 v0.4 h2.7 a1.4 1.4 0 0 0 1.4 -1.4 V9.7 h-0.4 a1.7 1.7 0 0 1 0 -3.4 h0.4 V3.6 a1.4 1.4 0 0 0 -1.4 -1.4 H9.7 z" />
    </svg>
  );
}

export function LinkIcon() {
  return (
    <svg {...base}>
      <path d="M6.5 9.5 l3 -3" />
      <path d="M7.4 4.4 l1.9 -1.9 a3.2 3.2 0 0 1 4.5 4.5 l-1.9 1.9" />
      <path d="M8.6 11.6 l-1.9 1.9 a3.2 3.2 0 0 1 -4.5 -4.5 l1.9 -1.9" />
    </svg>
  );
}

export function KeyIcon() {
  return (
    <svg {...base}>
      <circle cx="5" cy="11" r="3.3" />
      <path d="M7.4 8.6 l6.6 -6.6" />
      <path d="M11 5 l1.7 1.7 M12.7 3.3 l1.5 1.5" />
    </svg>
  );
}

export function LockIcon() {
  return (
    <svg {...base}>
      <rect x="2.5" y="6.8" width="11.1" height="7.7" rx="1.7" />
      <path d="M5.2 6.8 V4.9 a2.8 2.8 0 0 1 5.5 0 v1.9" />
      <path d="M8 9.8 v1.7" />
    </svg>
  );
}

export function UnlockIcon() {
  return (
    <svg {...base}>
      <rect x="2.5" y="6.8" width="11.1" height="7.7" rx="1.7" />
      <path d="M5.2 6.8 V4.9 a2.8 2.8 0 0 1 5.5 0" />
      <path d="M8 9.8 v1.7" />
    </svg>
  );
}

export function EyeIcon() {
  return (
    <svg {...base}>
      <path d="M1.5 8 s2.6 -4.6 6.5 -4.6 S14.5 8 14.5 8 s-2.6 4.6 -6.5 4.6 S1.5 8 1.5 8 z" />
      <circle cx="8" cy="8" r="2.3" />
    </svg>
  );
}

/** The slash runs top left to bottom right, cutting against the direction
 *  everything else in the set travels. */
export function EyeOffIcon() {
  return (
    <svg {...base}>
      <path d="M5.3 4.1 A7 7 0 0 1 8 3.4 C11.9 3.4 14.5 8 14.5 8 a12.8 12.8 0 0 1 -2.5 3.1 M9.7 10.2 a2.3 2.3 0 0 1 -3.3 -3.3" />
      <path d="M3.9 5.2 A13.2 13.2 0 0 0 1.5 8 s2.6 4.6 6.5 4.6 a6.8 6.8 0 0 0 2.6 -0.5" />
      <path d="M2.2 2.2 l11.5 11.5" />
    </svg>
  );
}

export function SidebarIcon() {
  return (
    <svg {...base}>
      <rect x="1.5" y="2.2" width="13" height="11.5" rx="1.7" />
      <path d="M6.3 2.2 v11.5" />
    </svg>
  );
}

export function PanelIcon() {
  return (
    <svg {...base}>
      <rect x="1.5" y="2.2" width="13" height="11.5" rx="1.7" />
      <path d="M1.5 6.3 h13" />
    </svg>
  );
}

export function SplitIcon() {
  return (
    <svg {...base}>
      <rect x="1.5" y="2.2" width="13" height="11.5" rx="1.7" />
      <path d="M8 2.2 v11.5" />
    </svg>
  );
}

export function GridIcon() {
  return (
    <svg {...base}>
      <rect x="1.8" y="1.8" width="5.3" height="5.3" rx="1.3" />
      <rect x="8.9" y="1.8" width="5.3" height="5.3" rx="1.3" />
      <rect x="1.8" y="8.9" width="5.3" height="5.3" rx="1.3" />
      <rect x="8.9" y="8.9" width="5.3" height="5.3" rx="1.3" />
    </svg>
  );
}

export function ListIcon() {
  return (
    <svg {...base}>
      <path d="M5.4 3.9 h8.7 M5.4 8 h8.7 M5.4 12.1 h8.7" />
      <path d="M2.1 3.9 h0 M2.1 8 h0 M2.1 12.1 h0" />
    </svg>
  );
}

export function TableIcon() {
  return (
    <svg {...base}>
      <rect x="1.5" y="2.2" width="13" height="11.5" rx="1.7" />
      <path d="M1.5 6.3 h13 M1.5 10.1 h13 M6.7 6.3 v7.5" />
    </svg>
  );
}

export function MaximizeIcon() {
  return (
    <svg {...base}>
      <path d="M9.5 2 h4.5 v4.5 M14 2 L9.7 6.3" />
      <path d="M6.5 14 H2 V9.5 M2 14 l4.3 -4.3" />
    </svg>
  );
}

export function MinimizeIcon() {
  return (
    <svg {...base}>
      <path d="M14 6.5 H9.5 V2 M9.5 6.5 l4.5 -4.5" />
      <path d="M2 9.5 h4.5 v4.5 M6.5 9.5 l-4.5 4.5" />
    </svg>
  );
}

export function CopyIcon() {
  return (
    <svg {...base}>
      <rect x="5.4" y="5.4" width="9.1" height="9.1" rx="1.7" />
      <path d="M11.6 5.4 V3.2 a1.7 1.7 0 0 0 -1.7 -1.7 H3.2 a1.7 1.7 0 0 0 -1.7 1.7 v6.7 a1.7 1.7 0 0 0 1.7 1.7 h2.2" />
    </svg>
  );
}

export function ClipboardIcon() {
  return (
    <svg {...base}>
      <path d="M10.1 3.1 h1.5 a1.5 1.5 0 0 1 1.5 1.5 v9 a1.5 1.5 0 0 1 -1.5 1.5 H4.4 a1.5 1.5 0 0 1 -1.5 -1.5 V4.6 a1.5 1.5 0 0 1 1.5 -1.5 H5.9" />
      <rect x="5.9" y="1.2" width="4.3" height="3.4" rx="1.1" />
    </svg>
  );
}

export function DownloadIcon() {
  return (
    <svg {...base}>
      <path d="M8 2.2 v8.5 M4.2 6.9 l3.8 3.8 L11.8 6.9" />
      <path d="M2.9 9.5 v3 a1.5 1.5 0 0 0 1.5 1.5 h7.2 a1.5 1.5 0 0 0 1.5 -1.5 V9.5" />
    </svg>
  );
}

export function ShareIcon() {
  return (
    <svg {...base}>
      <circle cx="12.5" cy="3.3" r="2" />
      <circle cx="3.5" cy="8" r="2" />
      <circle cx="12.5" cy="12.7" r="2" />
      <path d="M5.3 7 l5.3 -2.8 M5.3 9 l5.3 2.8" />
    </svg>
  );
}

export function ArchiveIcon() {
  return (
    <svg {...base}>
      <rect x="1.5" y="2" width="13" height="3.6" rx="1.3" />
      <path d="M2.9 5.7 v6.8 a1.5 1.5 0 0 0 1.5 1.5 h7.2 a1.5 1.5 0 0 0 1.5 -1.5 V5.7" />
      <path d="M6.3 9.1 h3.4" />
    </svg>
  );
}

export function BoxIcon() {
  return (
    <svg {...base}>
      <path d="M8 1.5 l6 3.3 v6.4 L8 14.5 L2 11.2 V4.8 L8 1.5 z" />
      <path d="M2 4.8 L8 8.1 L14 4.8 M8 8.1 v6.4 M5 3.1 l6 3.3" />
    </svg>
  );
}

export function FilterIcon() {
  return (
    <svg {...base}>
      <path d="M1.8 2.9 h12.4 L9.4 8.4 v4.1 l-2.8 1.5 V8.4 L1.8 2.9 z" />
    </svg>
  );
}

export function SortIcon() {
  return (
    <svg {...base}>
      <path d="M3.9 2.2 v11.5 M1.5 11.3 l2.5 2.5 l2.5 -2.5" />
      <path d="M8.6 4.2 h5.9 M8.6 8 h4.3 M8.6 11.8 h2.7" />
    </svg>
  );
}

export function SlidersIcon() {
  return (
    <svg {...base}>
      <path d="M2 4.6 h11.9 M2 11.4 h11.9" />
      <circle cx="5.9" cy="4.6" r="1.9" />
      <circle cx="10.6" cy="11.4" r="1.9" />
    </svg>
  );
}

export function GearIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="5.2" />
      <circle cx="8" cy="8" r="2" />
      <path d="M13.2 8 h1.4 M2.8 8 H1.4 M8 2.8 V1.4 M8 13.2 v1.4 M11.7 4.3 l1 -1 M4.3 11.7 l-1 1 M11.7 11.7 l1 1 M4.3 4.3 l-1 -1" />
    </svg>
  );
}

export function StarIcon() {
  return (
    <svg {...base}>
      <path d="M8 1.2l2 4.2 4.6.6-3.3 3.2.8 4.6L8 11.6l-4.1 2.2.8-4.6L1.4 6l4.6-.6L8 1.2z" />
    </svg>
  );
}

export function FlagIcon() {
  return (
    <svg {...base}>
      <path d="M3.1 14.5 V1.8" />
      <path d="M3.1 2.5 h9 a0.6 0.6 0 0 1 0.5 1 L10.8 5.9 l1.8 2.4 a0.6 0.6 0 0 1 -0.5 1 H3.1" />
    </svg>
  );
}

export function PinIcon() {
  return (
    <svg {...base}>
      <path d="M9.7 1.5 l4.8 4.8 l-1.7 1.7 l-1.1 0.2 l-4.2 4.2 l-0.5 2.3 l-5.2 -5.2 l2.3 -0.5 l4.2 -4.2 l0.2 -1.1 l1.2 -1.7 z" />
      <path d="M1.8 14.2 l3.5 -3.5" />
    </svg>
  );
}

export function TagIcon() {
  return (
    <svg {...base}>
      <path d="M2 7.4 V3.3 a1.3 1.3 0 0 1 1.3 -1.3 h4.1 a1.3 1.3 0 0 1 0.9 0.4 l5.8 5.8 a1.3 1.3 0 0 1 0 1.8 l-4.1 4.1 a1.3 1.3 0 0 1 -1.8 0 l-5.8 -5.8 a1.3 1.3 0 0 1 -0.4 -0.9 z" />
      <path d="M5.1 5.1 h0" />
    </svg>
  );
}

export function BookmarkIcon() {
  return (
    <svg {...base}>
      <path d="M3.3 14.5 V3.1 a1.5 1.5 0 0 1 1.5 -1.5 h6.4 a1.5 1.5 0 0 1 1.5 1.5 v11.4 L8 11.1 l-4.7 3.4 z" />
    </svg>
  );
}

export function UserIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="5.2" r="3.1" />
      <path d="M2.2 14.5 a5.8 5.8 0 0 1 11.5 0" />
    </svg>
  );
}

export function UsersIcon() {
  return (
    <svg {...base}>
      <circle cx="6.1" cy="5.2" r="2.9" />
      <path d="M1.5 14.2 a4.6 4.6 0 0 1 9.2 0" />
      <path d="M11.2 2.8 a2.9 2.9 0 0 1 0 4.9 M12.5 10 a4.6 4.6 0 0 1 2 4.2" />
    </svg>
  );
}

export function MailIcon() {
  return (
    <svg {...base}>
      <rect x="1.5" y="2.9" width="13" height="10.2" rx="1.6" />
      <path d="M1.5 4.8 l5.7 3.8 a1.5 1.5 0 0 0 1.6 0 L14.5 4.8" />
    </svg>
  );
}

export function MessageIcon() {
  return (
    <svg {...base}>
      <path d="M14.2 3.5 a1.5 1.5 0 0 0 -1.5 -1.5 H3.3 a1.5 1.5 0 0 0 -1.5 1.5 v7 a1.5 1.5 0 0 0 1.5 1.5 h1.1 v2.6 L8 12.1 h4.7 a1.5 1.5 0 0 0 1.5 -1.5 V3.5 z" />
    </svg>
  );
}

export function CalendarIcon() {
  return (
    <svg {...base}>
      <rect x="1.5" y="3.1" width="13" height="11.4" rx="1.6" />
      <path d="M1.5 6.5 h13 M5.2 1.5 v3.2 M10.8 1.5 v3.2" />
    </svg>
  );
}

export function TimerIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="9.3" r="5.2" />
      <path d="M8 6.5 v2.8 h2.1 M6.1 1.5 h3.8" />
    </svg>
  );
}

export function FlaskIcon() {
  return (
    <svg {...base}>
      <path d="M6.1 1.5 v4.6 L2.1 12.3 a1.4 1.4 0 0 0 1.2 2.1 h9.4 a1.4 1.4 0 0 0 1.2 -2.1 L9.9 6.1 V1.5" />
      <path d="M5.2 1.5 h5.5 M4.2 9.9 h7.7" />
    </svg>
  );
}

export function BugIcon() {
  return (
    <svg {...base}>
      <path d="M4.4 6.3 a3.6 3.6 0 0 1 7.2 0 v3.6 a3.6 3.6 0 0 1 -7.2 0 V6.3 z" />
      <path d="M5.9 4.2 a2.3 2.3 0 0 1 4.3 0" />
      <path d="M4.4 7.4 H1.5 M14.5 7.4 h-2.9 M4.4 11 l-2.3 2 M11.6 11 l2.3 2 M4.8 4.4 L3.1 2.7 M11.2 4.4 L12.9 2.7" />
    </svg>
  );
}

export function WrenchIcon() {
  return (
    <svg {...base}>
      <path d="M10.4 9.1a3.7 3.7 0 004.5-4.5l-2.5 2.5-2.1-.5-.5-2.1 2.5-2.5a3.7 3.7 0 00-4.5 4.5l-5.5 5.5a2.1 2.1 0 003 3l5.1-5.9z" />
    </svg>
  );
}

export function ImageIcon() {
  return (
    <svg {...base}>
      <rect x="1.5" y="2.2" width="13" height="11.5" rx="1.7" />
      <circle cx="5.4" cy="6.1" r="1.3" />
      <path d="M1.5 11.3 l3.5 -3.5 l3.6 3.6 l2.2 -2.2 l3.6 3.6" />
    </svg>
  );
}

export function PaperclipIcon() {
  return (
    <svg {...base}>
      <path d="M11.8 7.1 L6.3 12.7 a3.2 3.2 0 0 1 -4.5 -4.5 l7.2 -7.2 a2.1 2.1 0 0 1 3 3 l-7.2 7.2 a1.1 1.1 0 0 1 -1.5 -1.5 l6.6 -6.6" />
    </svg>
  );
}

export function MicIcon() {
  return (
    <svg {...base}>
      <rect x="5.7" y="1.5" width="4.7" height="8.5" rx="2.3" />
      <path d="M3.1 8.4 v0.9 a4.9 4.9 0 0 0 9.8 0 v-0.9 M8 14.2 v0.3" />
    </svg>
  );
}

export function SunIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="3.5" />
      <path d="M8 1.4 v1.6 M8 13 v1.6 M1.4 8 h1.6 M13 8 h1.6 M3.3 3.3 l1.1 1.1 M11.5 11.5 l1.1 1.1 M11.5 4.5 l1.1 -1.1 M3.3 12.7 l1.1 -1.1" />
    </svg>
  );
}

export function MoonIcon() {
  return (
    <svg {...base}>
      <path d="M13.8 9.7 A6.5 6.5 0 0 1 6.3 2.2 a6.5 6.5 0 1 0 7.5 7.5 z" />
    </svg>
  );
}

export function MonitorIcon() {
  return (
    <svg {...base}>
      <rect x="1.5" y="2.2" width="13" height="9" rx="1.6" />
      <path d="M5.2 14.5 h5.5 M8 11.2 v3.3" />
    </svg>
  );
}

export function CoinIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="7" />
      <path d="M8 3.6v8.8" />
      <path d="M10.2 5.8H6.8a1.8 1.8 0 000 3.6h2.4a1.8 1.8 0 010 3.6H5.6" />
    </svg>
  );
}

export function FireIcon() {
  return (
    <svg {...base}>
      <path d="M8 1.5 S3.9 4.9 3.9 9 a4.1 4.1 0 0 0 8.1 0 c0 -1.5 -0.7 -2.9 -1.7 -3.9 c-0.4 1.1 -1.1 1.8 -1.8 1.8 c0.7 -2.3 -0.5 -5.3 -0.5 -5.3 z" />
    </svg>
  );
}

export function LeafIcon() {
  return (
    <svg {...base}>
      <path d="M2.2 13.8 S1.6 9 4.9 5.7 S13.8 2.2 13.8 2.2 s0.5 5.5 -2.8 8.8 s-8.7 2.7 -8.7 2.7 z" />
      <path d="M2.2 13.8 L8 8" />
    </svg>
  );
}

export function ChevronDownIcon() {
  return (
    <svg {...base}>
      <path d="M2.4 5.2L8 10.8l5.6-5.6" />
    </svg>
  );
}

export function ChevronUpIcon() {
  return (
    <svg {...base}>
      <path d="M2.4 10.8L8 5.2l5.6 5.6" />
    </svg>
  );
}

export function ChevronLeftIcon() {
  return (
    <svg {...base}>
      <path d="M10.8 2.4L5.2 8l5.6 5.6" />
    </svg>
  );
}

export function ChevronsRightIcon() {
  return (
    <svg {...base}>
      <path d="M3 3.4L7.6 8 3 12.6M9 3.4L13.6 8 9 12.6" />
    </svg>
  );
}

export function ChevronsLeftIcon() {
  return (
    <svg {...base}>
      <path d="M13 3.4L8.4 8l4.6 4.6M7 3.4L2.4 8 7 12.6" />
    </svg>
  );
}

/** Marks a step that belongs to the one above it. */
export function CornerDownRightIcon() {
  return (
    <svg {...base}>
      <path d="M3 2.4v6.4a1.6 1.6 0 001.6 1.6H14" />
      <path d="M10.6 7l3.4 3.4-3.4 3.4" />
    </svg>
  );
}

export function CheckAllIcon() {
  return (
    <svg {...base}>
      <path d="M1.4 8.6l3 3 5.4-6.2" />
      <path d="M6.6 11.6l1 1 5.4-6.2" />
    </svg>
  );
}

export function ZoomInIcon() {
  return (
    <svg {...base}>
      <circle cx="7.1" cy="7.1" r="4.9" />
      <path d="M10.6 10.6L14.6 14.6M7.1 5.2v3.8M5.2 7.1h3.8" />
    </svg>
  );
}

export function ZoomOutIcon() {
  return (
    <svg {...base}>
      <circle cx="7.1" cy="7.1" r="4.9" />
      <path d="M10.6 10.6L14.6 14.6M5.2 7.1h3.8" />
    </svg>
  );
}

export function TextIcon() {
  return (
    <svg {...base}>
      <path d="M2.4 3.6V2h11.2v1.6M8 2v12M5.6 14h4.8" />
    </svg>
  );
}

export function QuoteIcon() {
  return (
    <svg {...base}>
      <path d="M6.4 4.6H3.6A1.6 1.6 0 002 6.2v2.6a1.6 1.6 0 001.6 1.6h1.2v.4a2.6 2.6 0 01-2.6 2.6" />
      <path d="M14 4.6h-2.8a1.6 1.6 0 00-1.6 1.6v2.6a1.6 1.6 0 001.6 1.6h1.2v.4a2.6 2.6 0 01-2.6 2.6" />
    </svg>
  );
}

export function ListOrderedIcon() {
  return (
    <svg {...base}>
      <path d="M6.4 4.2H14M6.4 8H14M6.4 11.8H14" />
      <path d="M2 2.6h1v3.2M1.8 12.4a1.2 1.2 0 112 .9L1.8 15h2.2" transform="translate(0 -1.4)" />
    </svg>
  );
}

export function IndentIcon() {
  return (
    <svg {...base}>
      <path d="M6.4 4.2H14M6.4 8H14M2 11.8h12" />
      <path d="M1.6 4.6L4 7 1.6 9.4" />
    </svg>
  );
}

export function WrapIcon() {
  return (
    <svg {...base}>
      <path d="M1.6 3.4h12.8" />
      <path d="M1.6 8h10.2a2.4 2.4 0 010 4.8H8.2" />
      <path d="M10.2 10.8L8.2 12.8l2 2" />
      <path d="M1.6 12.8h3.4" />
    </svg>
  );
}

/** The `.*` of a search bar: the asterisk over its dot. */
export function RegexIcon() {
  return (
    <svg {...base}>
      <path d="M8 2.4v7.2M4.9 4.2l6.2 3.6M11.1 4.2L4.9 7.8" />
      <circle cx="8" cy="13" r="1.2" fill="currentColor" stroke="none" />
    </svg>
  );
}

/** Aa: the case-sensitive toggle in the search bar. */
export function CaseIcon() {
  return (
    <svg {...base}>
      <path d="M1.4 12.6L5 3.4l3.6 9.2M2.6 9.8h4.8" />
      <path d="M14.6 8.8v3.8M14.6 10.2a2.4 2.4 0 10-.7 1.7" />
    </svg>
  );
}

export function ReplaceIcon() {
  return (
    <svg {...base}>
      <rect x="1.4" y="8.8" width="5.8" height="5.8" rx="1.4" />
      <rect x="8.8" y="1.4" width="5.8" height="5.8" rx="1.4" />
      <path d="M7.2 11.7h3.2a1.4 1.4 0 001.4-1.4V9" />
      <path d="M10.2 10.2l1.6-1.6 1.6 1.6" />
    </svg>
  );
}

export function SaveIcon() {
  return (
    <svg {...base}>
      <path d="M14 5.4V13a1.4 1.4 0 01-1.4 1.4H3.4A1.4 1.4 0 012 13V3a1.4 1.4 0 011.4-1.4h7.2L14 5.4z" />
      <path d="M4.8 1.6v3.8h6.4V1.6M4.8 14.4v-4h6.4v4" />
    </svg>
  );
}

export function FileSearchIcon() {
  return (
    <svg {...base}>
      <path d="M12.8 6.6V13a1.6 1.6 0 01-1.6 1.6H4.4A1.6 1.6 0 012.8 13V3a1.6 1.6 0 011.6-1.6h4.2L12.8 6.6z" />
      <path d="M8.6 1.4v4.2h4.2" />
      <circle cx="7.4" cy="10" r="2.2" />
      <path d="M9 11.6l1.8 1.8" />
    </svg>
  );
}

export function FolderPlusIcon() {
  return (
    <svg {...base}>
      <path d="M1.4 4.9V3.8a1.4 1.4 0 011.4-1.4h3.4l1.9 2.5h6a1.4 1.4 0 011.4 1.4v6a1.4 1.4 0 01-1.4 1.4H2.8a1.4 1.4 0 01-1.4-1.4V4.9z" />
      <path d="M8 7.4v3.6M6.2 9.2h3.6" />
    </svg>
  );
}

export function RepoIcon() {
  return (
    <svg {...base}>
      <path d="M3.4 11.4h10.2V2.8a1.4 1.4 0 00-1.4-1.4H4.6a2.4 2.4 0 00-2.4 2.4v8.4a2.4 2.4 0 002.4 2.4h9v-2.8" />
      <path d="M5.4 4.4h5.2" />
    </svg>
  );
}

export function GitCompareIcon() {
  return (
    <svg {...base}>
      <circle cx="3.6" cy="12.4" r="1.9" />
      <circle cx="12.4" cy="3.6" r="1.9" />
      <path d="M12.4 5.5v5.5a1.4 1.4 0 01-1.4 1.4H7.4M3.6 10.5V5a1.4 1.4 0 011.4-1.4h3.6" />
      <path d="M9 1.8L10.8 3.6 9 5.4M7 10.6L5.2 12.4 7 14.2" />
    </svg>
  );
}

export function GitForkIcon() {
  return (
    <svg {...base}>
      <circle cx="3.6" cy="3.4" r="1.9" />
      <circle cx="12.4" cy="3.4" r="1.9" />
      <circle cx="8" cy="12.6" r="1.9" />
      <path d="M3.6 5.3v1.5a1.6 1.6 0 001.6 1.6h5.6a1.6 1.6 0 001.6-1.6V5.3M8 8.4v2.3" />
    </svg>
  );
}

/** Fan out: one turn splitting into the agents it launched. */
export function FanOutIcon() {
  return (
    <svg {...base}>
      <circle cx="3" cy="8" r="1.9" />
      <circle cx="13" cy="3.4" r="1.9" />
      <circle cx="13" cy="12.6" r="1.9" />
      <path d="M4.9 7.2l6.3-2.9M4.9 8.8l6.3 2.9" />
    </svg>
  );
}

export function FanInIcon() {
  return (
    <svg {...base}>
      <circle cx="13" cy="8" r="1.9" />
      <circle cx="3" cy="3.4" r="1.9" />
      <circle cx="3" cy="12.6" r="1.9" />
      <path d="M4.8 4.3l6.4 2.9M4.8 11.7l6.4-2.9" />
    </svg>
  );
}

export function LoopIcon() {
  return (
    <svg {...base}>
      <path d="M4 3.4h6.6A3.4 3.4 0 0114 6.8v0a3.4 3.4 0 01-3.4 3.4H2" />
      <path d="M4.6 7.6L2 10.2l2.6 2.6" />
    </svg>
  );
}

export function RepeatIcon() {
  return (
    <svg {...base}>
      <path d="M2.4 6.4V5a1.6 1.6 0 011.6-1.6h9.6M11 1.4l2.6 2-2.6 2" />
      <path d="M13.6 9.6V11a1.6 1.6 0 01-1.6 1.6H2.4M5 14.6l-2.6-2 2.6-2" />
    </svg>
  );
}

export function ShuffleIcon() {
  return (
    <svg {...base}>
      <path d="M2 4h2.6l6.8 8H14M2 12h2.6l2.4-2.8M9 6.4l2.4-2.4H14" />
      <path d="M11.8 1.6L14.2 4l-2.4 2.4M11.8 9.6l2.4 2.4-2.4 2.4" />
    </svg>
  );
}

export function RouteIcon() {
  return (
    <svg {...base}>
      <circle cx="3.4" cy="12.6" r="1.9" />
      <circle cx="12.6" cy="3.4" r="1.9" />
      <path d="M5.3 12.6h3.5a2.6 2.6 0 000-5.2H7.2a2.6 2.6 0 010-5.2h3.5" transform="translate(0 1.6)" />
    </svg>
  );
}

export function NetworkIcon() {
  return (
    <svg {...base}>
      <rect x="5.6" y="1.4" width="4.8" height="4.4" rx="1.2" />
      <rect x="1" y="10.2" width="4.8" height="4.4" rx="1.2" />
      <rect x="10.2" y="10.2" width="4.8" height="4.4" rx="1.2" />
      <path d="M8 5.8v2.4M3.4 10.2V8.2h9.2v2" />
    </svg>
  );
}

export function StackIcon() {
  return (
    <svg {...base}>
      <rect x="1.4" y="1.4" width="13.2" height="3.6" rx="1.2" />
      <rect x="1.4" y="6.2" width="13.2" height="3.6" rx="1.2" />
      <rect x="1.4" y="11" width="13.2" height="3.6" rx="1.2" />
    </svg>
  );
}

export function ActivityIcon() {
  return (
    <svg {...base}>
      <path d="M1 8h3.4l2.2-5.6 3 11.2 2.2-5.6H15" />
    </svg>
  );
}

export function BarChartIcon() {
  return (
    <svg {...base}>
      <path d="M2 14.4V9M6 14.4V4.2M10 14.4V6.6M14 14.4V1.6" />
    </svg>
  );
}

export function LineChartIcon() {
  return (
    <svg {...base}>
      <path d="M1.8 1.6v11.2a1.6 1.6 0 001.6 1.6h11.2" />
      <path d="M4.6 11l2.8-3.4 2.4 2.2 3.6-4.6" />
    </svg>
  );
}

export function TrendUpIcon() {
  return (
    <svg {...base}>
      <path d="M1.6 12.4l4.4-4.4 2.8 2.8L14.4 5" />
      <path d="M9.8 5h4.6v4.6" />
    </svg>
  );
}

export function PercentIcon() {
  return (
    <svg {...base}>
      <path d="M13 3L3 13" />
      <circle cx="4.4" cy="4.4" r="2" />
      <circle cx="11.6" cy="11.6" r="2" />
    </svg>
  );
}

export function BatteryIcon() {
  return (
    <svg {...base}>
      <rect x="1" y="4.4" width="11.4" height="7.2" rx="1.8" />
      <path d="M14.6 6.8v2.4" />
      <path d="M3.4 6.8v2.4M6 6.8v2.4" />
    </svg>
  );
}

export function ScaleIcon() {
  return (
    <svg {...base}>
      <path d="M8 2v12.4M4.2 14.4h7.6M8 3.4L3 5.2M8 3.4l5 1.8" />
      <path d="M1 9.2a2.4 2.4 0 004.8 0L3.4 4.6 1 9.2zM10.2 9.2a2.4 2.4 0 004.8 0l-2.4-4.6-2.4 4.6z" />
    </svg>
  );
}

export function CommandKeyIcon() {
  return (
    <svg {...base}>
      <path d="M5.8 2.2a1.8 1.8 0 100 3.6h4.4a1.8 1.8 0 100-3.6 1.8 1.8 0 00-1.8 1.8v8.4a1.8 1.8 0 101.8-1.8H5.8a1.8 1.8 0 101.8 1.8V4a1.8 1.8 0 00-1.8-1.8z" />
    </svg>
  );
}

export function ReturnIcon() {
  return (
    <svg {...base}>
      <path d="M14 2.4v5.2a2 2 0 01-2 2H2" />
      <path d="M5.4 6.2L2 9.6l3.4 3.4" />
    </svg>
  );
}

export function EscapeIcon() {
  return (
    <svg {...base}>
      <path d="M13.8 13.8L3.6 3.6" />
      <path d="M3.6 9.2V3.6h5.6" />
    </svg>
  );
}

export function TabIcon() {
  return (
    <svg {...base}>
      <path d="M2 4.4L6.4 8 2 11.6M14 2.6v10.8" />
      <path d="M2 8h4.4" />
    </svg>
  );
}

export function BackspaceIcon() {
  return (
    <svg {...base}>
      <path d="M14.4 3.4a1.4 1.4 0 00-1.4-1.4H6.2a1.4 1.4 0 00-1 .4L1 8l4.2 5.6a1.4 1.4 0 001 .4H13a1.4 1.4 0 001.4-1.4V3.4z" />
      <path d="M7.4 6l4 4M11.4 6l-4 4" />
    </svg>
  );
}

export function PowerIcon() {
  return (
    <svg {...base}>
      <path d="M8 1.4v6.8" />
      <path d="M11.9 4.3a5.6 5.6 0 11-7.8 0" />
    </svg>
  );
}

export function LogOutIcon() {
  return (
    <svg {...base}>
      <path d="M6 2H3.4A1.4 1.4 0 002 3.4v9.2A1.4 1.4 0 003.4 14H6" />
      <path d="M10.4 4.4L14 8l-3.6 3.6M14 8H6" />
    </svg>
  );
}

export function LogInIcon() {
  return (
    <svg {...base}>
      <path d="M10 2h2.6A1.4 1.4 0 0114 3.4v9.2a1.4 1.4 0 01-1.4 1.4H10" />
      <path d="M5.6 4.4L9.2 8l-3.6 3.6M9.2 8H1.4" />
    </svg>
  );
}

/** A box held inside a frame: work that only reaches as far as the sandbox
 *  lets it. */
export function SandboxIcon() {
  return (
    <svg {...base}>
      <path d="M5.4 1.6H3.4a1.8 1.8 0 00-1.8 1.8v2M10.6 1.6h2a1.8 1.8 0 011.8 1.8v2M14.4 10.6v2a1.8 1.8 0 01-1.8 1.8h-2M5.4 14.4h-2a1.8 1.8 0 01-1.8-1.8v-2" />
      <rect x="5.3" y="5.3" width="5.4" height="5.4" rx="1.4" />
    </svg>
  );
}

export function ShieldCheckIcon() {
  return (
    <svg {...base}>
      <path d="M8 1.2l5.9 2.1v4.9c0 3.2-2.3 5.6-5.9 6.6-3.6-1-5.9-3.4-5.9-6.6V3.3L8 1.2z" />
      <path d="M5.6 7.8l1.9 1.9 3.2-3.6" />
    </svg>
  );
}

export function ShieldOffIcon() {
  return (
    <svg {...base}>
      <path d="M4.6 3.9L8 1.2l5.9 2.1v4.9a6 6 0 01-.9 3.2" />
      <path d="M2.1 6v2.2c0 3.2 2.3 5.6 5.9 6.6a8.5 8.5 0 003.1-1.5" />
      <path d="M1.6 1.6l12.8 12.8" />
    </svg>
  );
}

export function BanIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="7" />
      <path d="M3.1 3.1l9.8 9.8" />
    </svg>
  );
}

export function CrosshairIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="5.4" />
      <path d="M8 1v3M8 12v3M1 8h3M12 8h3" />
    </svg>
  );
}

export function PointerIcon() {
  return (
    <svg {...base}>
      <path d="M2.6 1.8l4.6 12.4 2-5 5-2L2.6 1.8z" />
    </svg>
  );
}

export function KeyboardIcon() {
  return (
    <svg {...base}>
      <rect x="1" y="3.4" width="14" height="9.2" rx="1.8" />
      <path d="M4 6.4h.01M7 6.4h.01M10 6.4h.01M12.8 6.4h.01M4 9.8h6.4" />
    </svg>
  );
}

export function HammerIcon() {
  return (
    <svg {...base}>
      <path d="M9.1 1.5l5.2 5.2-1.8 1.8-5.2-5.2 1.8-1.8z" />
      <path d="M9.9 5.9L2.8 13" />
    </svg>
  );
}

export function ScissorsIcon() {
  return (
    <svg {...base}>
      <circle cx="4" cy="12" r="2.4" />
      <circle cx="12" cy="12" r="2.4" />
      <path d="M5.7 10.3L14 1.4M10.3 10.3L2 1.4" />
    </svg>
  );
}

export function RulerIcon() {
  return (
    <svg {...base}>
      <path d="M10.4 1.4l4.2 4.2a1.4 1.4 0 010 2L7.6 14.6a1.4 1.4 0 01-2 0L1.4 10.4a1.4 1.4 0 010-2L8.4 1.4a1.4 1.4 0 012 0z" />
      <path d="M5.4 4.4l1.8 1.8M8.2 7.2L10 9M3 6.8l1.8 1.8M7.8 2l1.8 1.8" />
    </svg>
  );
}

export function PaletteIcon() {
  return (
    <svg {...base}>
      <path d="M8 1a7 7 0 000 14 1.9 1.9 0 001.4-3.2 1.9 1.9 0 011.4-3.2h1.7A2.5 2.5 0 0015 6.1 7.1 7.1 0 008 1z" />
      <path d="M4.6 6.6h.01M7.4 4.4h.01M10.6 5.4h.01M4.2 10.2h.01" />
    </svg>
  );
}

export function ContrastIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="7" />
      <path d="M8 1v14a7 7 0 000-14z" fill="currentColor" stroke="none" />
    </svg>
  );
}

export function DropletIcon() {
  return (
    <svg {...base}>
      <path d="M8 1.2l4.2 5.4a5.3 5.3 0 11-8.4 0L8 1.2z" />
    </svg>
  );
}

export function SnowflakeIcon() {
  return (
    <svg {...base}>
      <path d="M8 1v14M2 4.5l12 7M14 4.5l-12 7" />
      <path d="M5.8 3.2L8 1l2.2 2.2M5.8 12.8L8 15l2.2-2.2" />
    </svg>
  );
}

export function RocketIcon() {
  return (
    <svg {...base}>
      <path d="M8 1.2c2.1 1.9 3.3 4.6 3.3 7.5v1.5H4.7V8.7c0-2.9 1.2-5.6 3.3-7.5z" />
      <path d="M4.7 8.9L2.2 11.2v3.4l2.5-1.7M11.3 8.9l2.5 2.3v3.4l-2.5-1.7" />
      <circle cx="8" cy="6.2" r="1.5" />
      <path d="M6.6 12.4v1.7M9.4 12.4v1.7" />
    </svg>
  );
}

export function LightbulbIcon() {
  return (
    <svg {...base}>
      <path d="M8 1.2a4.9 4.9 0 00-2.9 8.9v1.5h5.8v-1.5A4.9 4.9 0 008 1.2z" />
      <path d="M6.2 12.8h3.6M6.8 14.6h2.4" />
    </svg>
  );
}

export function MegaphoneIcon() {
  return (
    <svg {...base}>
      <path d="M14.4 2.6v9.4L5 9.8V4.8l9.4-2.2z" />
      <path d="M5 4.8H3a1.8 1.8 0 000 3.6h2" />
      <path d="M5.6 9.9l1 4a1.2 1.2 0 002.3-.5v-3" />
    </svg>
  );
}

export function InboxIcon() {
  return (
    <svg {...base}>
      <path d="M1.4 9h3.6l1.2 2h3.6l1.2-2h3.6" />
      <path d="M3.3 2.7l-1.7 5.9a1.4 1.4 0 00-.2.7v3.3A1.4 1.4 0 002.8 14h10.4a1.4 1.4 0 001.4-1.4V9.3a1.4 1.4 0 00-.2-.7l-1.7-5.9A1.4 1.4 0 0011.4 2H4.6a1.4 1.4 0 00-1.3.7z" />
    </svg>
  );
}

export function AnchorIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="3.4" r="2" />
      <path d="M8 5.4v9.2M4.2 8H1.4a6.6 6.6 0 0013.2 0h-2.8" />
    </svg>
  );
}

export function SmileIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="7" />
      <path d="M5.2 9.6a3.4 3.4 0 005.6 0M5.8 6h.01M10.2 6h.01" />
    </svg>
  );
}

export function ThumbUpIcon() {
  return (
    <svg {...base}>
      <path d="M4.6 14.4V7.2l3.6-5.8a2 2 0 011.7 3l-.8 2.6h3.9a1.8 1.8 0 011.7 2.3l-1.2 4.4a1.8 1.8 0 01-1.7 1.3H4.6z" />
      <path d="M4.6 7.2H2.2a.8.8 0 00-.8.8v5.6a.8.8 0 00.8.8h2.4" />
    </svg>
  );
}

export function ThumbDownIcon() {
  return (
    <svg {...base}>
      <path d="M4.6 1.6v7.2l3.6 5.8a2 2 0 001.7-3l-.8-2.6h3.9a1.8 1.8 0 001.7-2.3l-1.2-4.4a1.8 1.8 0 00-1.7-1.3H4.6z" />
      <path d="M4.6 8.8H2.2a.8.8 0 01-.8-.8V2.4a.8.8 0 01.8-.8h2.4" />
    </svg>
  );
}

export function HeartIcon() {
  return (
    <svg {...base}>
      <path d="M8 14.2L2.4 8.6a3.7 3.7 0 015.6-4.8 3.7 3.7 0 015.6 4.8L8 14.2z" />
    </svg>
  );
}

export function GhostIcon() {
  return (
    <svg {...base}>
      <path d="M2.4 14.6V7.4a5.6 5.6 0 0111.2 0v7.2l-1.9-1.4-1.9 1.4-1.8-1.4-1.8 1.4-1.9-1.4-1.9 1.4z" />
      <path d="M6.2 6.8h.01M9.8 6.8h.01" />
    </svg>
  );
}

/** An agent: a head that is a box, because everything else in the set that
 *  computes is a box too. */
export function AgentIcon() {
  return (
    <svg {...base}>
      <rect x="2.2" y="5" width="11.6" height="9" rx="2.2" />
      <path d="M8 1.4V5M5.8 9h.01M10.2 9h.01M6.2 11.6h3.6" />
      <path d="M2.2 8.4H1M15 8.4h-1.2" />
    </svg>
  );
}

export function ColumnsIcon() {
  return (
    <svg {...base}>
      <rect x="1.4" y="1.6" width="13.2" height="12.8" rx="1.8" />
      <path d="M5.8 1.6v12.8M10.2 1.6v12.8" />
    </svg>
  );
}

export function RowsIcon() {
  return (
    <svg {...base}>
      <rect x="1.4" y="1.6" width="13.2" height="12.8" rx="1.8" />
      <path d="M1.4 5.9h13.2M1.4 10.1h13.2" />
    </svg>
  );
}

export function WindowIcon() {
  return (
    <svg {...base}>
      <rect x="1.4" y="2.2" width="13.2" height="11.6" rx="1.8" />
      <path d="M1.4 5.8h13.2M4 4h.01M6.2 4h.01" />
    </svg>
  );
}

export function ExpandIcon() {
  return (
    <svg {...base}>
      <path d="M6 1.6H1.6V6M1.6 1.6L6.4 6.4" />
      <path d="M10 14.4h4.4V10M14.4 14.4L9.6 9.6" />
    </svg>
  );
}

export function CollapseIcon() {
  return (
    <svg {...base}>
      <path d="M1.6 6H6V1.6M6 6L1.6 1.6" />
      <path d="M14.4 10H10v4.4M10 10l4.4 4.4" />
    </svg>
  );
}

/** Filled icons are solid shapes with the interior detail cut straight out of
 *  the fill, so a filled and an outline icon read as the same glyph. Only the
 *  ones the product needs a selected or active state for have one. */
const solid = {
  width: SIZE,
  height: SIZE,
  viewBox: `0 0 ${GRID} ${GRID}`,
  fill: "currentColor",
  stroke: "none" as const,
  "aria-hidden": true,
};

export function CircleFilledIcon() {
  return (
    <svg {...solid}>
      <circle cx="8" cy="8" r="7" />
    </svg>
  );
}

export function CircleDotFilledIcon() {
  return (
    <svg {...solid}>
      <path fillRule="evenodd" clipRule="evenodd" d="M8 1a7 7 0 100 14A7 7 0 008 1zm0 4.9a2.1 2.1 0 100 4.2 2.1 2.1 0 000-4.2z" />
    </svg>
  );
}

export function CircleCheckFilledIcon() {
  return (
    <svg {...solid}>
      <path fillRule="evenodd" clipRule="evenodd" d="M8 1a7 7 0 100 14A7 7 0 008 1zM4.9 9.1l2.7 2.7 4.4-4.4-1-1-3.4 3.4-1.7-1.7-1 1z" />
    </svg>
  );
}

export function CircleXFilledIcon() {
  return (
    <svg {...solid}>
      <path fillRule="evenodd" clipRule="evenodd" d="M8 1a7 7 0 100 14A7 7 0 008 1zm2.9 8.9l-1 1L8 9l-1.9 1.9-1-1L7 8 5.1 6.1l1-1L8 7l1.9-1.9 1 1L9 8z" />
    </svg>
  );
}

export function AlertFilledIcon() {
  return (
    <svg {...solid}>
      <path fillRule="evenodd" clipRule="evenodd" d="M8 1a7 7 0 100 14A7 7 0 008 1zm0 3.4a.8.8 0 01.8.8v3.9a.8.8 0 01-1.6 0V5.2a.8.8 0 01.8-.8zm0 6.2a.9.9 0 110 1.8.9.9 0 010-1.8z" />
    </svg>
  );
}

export function InfoFilledIcon() {
  return (
    <svg {...solid}>
      <path fillRule="evenodd" clipRule="evenodd" d="M8 1a7 7 0 100 14A7 7 0 008 1zm0 2.6a.9.9 0 110 1.8.9.9 0 010-1.8zm0 3a.8.8 0 01.8.8v3.9a.8.8 0 01-1.6 0V7.4a.8.8 0 01.8-.8z" />
    </svg>
  );
}

export function WarningFilledIcon() {
  return (
    <svg {...solid}>
      <path fillRule="evenodd" clipRule="evenodd" d="M9 2.3a1.2 1.2 0 00-2 0L.9 12.6a1.2 1.2 0 001 1.8h12.2a1.2 1.2 0 001-1.8L9 2.3zM8 5.6a.8.8 0 01.8.8v3.1a.8.8 0 01-1.6 0V6.4a.8.8 0 01.8-.8zm0 5.5a.9.9 0 110 1.8.9.9 0 010-1.8z" />
    </svg>
  );
}

export function ShieldFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M8 1.5l5.6 2v4.7c0 3-2.2 5.3-5.6 6.3-3.4-1-5.6-3.3-5.6-6.3V3.5L8 1.5z" />
    </svg>
  );
}

export function ShieldCheckFilledIcon() {
  return (
    <svg {...solid}>
      <path fillRule="evenodd" clipRule="evenodd" d="M8 1.2L2.1 3.3v4.9c0 3.2 2.3 5.6 5.9 6.6 3.6-1 5.9-3.4 5.9-6.6V3.3L8 1.2zM5.1 8.4l2.4 2.3 4.1-4-1-1-3.1 3.2-1.4-1.5-1 1z" />
    </svg>
  );
}

export function StarFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M8 1.2l2 4.2 4.6.6-3.3 3.2.8 4.6L8 11.6l-4.1 2.2.8-4.6L1.4 6l4.6-.6L8 1.2z" />
    </svg>
  );
}

export function HeartFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M8 14.2L2.4 8.6a3.7 3.7 0 015.6-4.8 3.7 3.7 0 015.6 4.8L8 14.2z" />
    </svg>
  );
}

export function BookmarkFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M3.3 14.5V3.1a1.5 1.5 0 011.5-1.5h6.4a1.5 1.5 0 011.5 1.5v11.4L8 11.1l-4.7 3.4z" />
    </svg>
  );
}

export function FlagFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M2.4 1.8a.7.7 0 011.4 0v12.7a.7.7 0 01-1.4 0V1.8z" />
      <path d="M4.4 2.5h7.7a.6.6 0 01.5 1L10.8 5.9l1.8 2.4a.6.6 0 01-.5 1H4.4V2.5z" />
    </svg>
  );
}

export function PinFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M9.7 1.5l4.8 4.8-1.7 1.7-1.1.2-4.2 4.2-.5 2.3-5.2-5.2 2.3-.5 4.2-4.2.2-1.1 1.2-1.7z" />
      <path d="M1.3 13.7a.7.7 0 011 1l-.5.5a.7.7 0 01-1-1l.5-.5z" />
    </svg>
  );
}

export function TagFilledIcon() {
  return (
    <svg {...solid}>
      <path fillRule="evenodd" clipRule="evenodd" d="M2 7.4V3.3A1.3 1.3 0 013.3 2h4.1a1.3 1.3 0 01.9.4l5.8 5.8a1.3 1.3 0 010 1.8l-4.1 4.1a1.3 1.3 0 01-1.8 0l-5.8-5.8a1.3 1.3 0 01-.4-.9zm3.1-3.5a1.2 1.2 0 100 2.4 1.2 1.2 0 000-2.4z" />
    </svg>
  );
}

export function BellFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M12.5 6.5a4.5 4.5 0 10-9 0c0 2.8-.6 4.2-1.3 5a.5.5 0 00.4.9h10.7a.5.5 0 00.4-.9c-.6-.9-1.3-2.2-1.3-5z" />
      <path d="M6.5 14.5a1.7 1.7 0 003 0h-3z" />
    </svg>
  );
}

export function EyeFilledIcon() {
  return (
    <svg {...solid}>
      <path fillRule="evenodd" clipRule="evenodd" d="M8 3.4C4.1 3.4 1.5 8 1.5 8s2.6 4.6 6.5 4.6S14.5 8 14.5 8 11.9 3.4 8 3.4zm0 2.2a2.4 2.4 0 110 4.8 2.4 2.4 0 010-4.8z" />
    </svg>
  );
}

export function LockFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M5.2 6.8V4.9a2.8 2.8 0 015.5 0v1.9H9.3V4.9a1.4 1.4 0 00-2.7 0v1.9H5.2z" />
      <path fillRule="evenodd" clipRule="evenodd" d="M2.5 8.3a1.7 1.7 0 011.7-1.7h7.6a1.7 1.7 0 011.7 1.7v4.5a1.7 1.7 0 01-1.7 1.7H4.2a1.7 1.7 0 01-1.7-1.7V8.3zm5.5 1.2a.8.8 0 01.8.8v1.4a.8.8 0 01-1.6 0v-1.4a.8.8 0 01.8-.8z" />
    </svg>
  );
}

export function MessageFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M14.2 3.5a1.5 1.5 0 00-1.5-1.5H3.3a1.5 1.5 0 00-1.5 1.5v7a1.5 1.5 0 001.5 1.5h1.1v2.6L8 12.1h4.7a1.5 1.5 0 001.5-1.5V3.5z" />
    </svg>
  );
}

export function TerminalFilledIcon() {
  return (
    <svg {...solid}>
      <path fillRule="evenodd" clipRule="evenodd" d="M3.3 2.4h9.4a1.7 1.7 0 011.7 1.7v7.8a1.7 1.7 0 01-1.7 1.7H3.3a1.7 1.7 0 01-1.7-1.7V4.1a1.7 1.7 0 011.7-1.7zm2 3.4l-.9 1 1.4 1.4-1.4 1.4.9 1 2.4-2.4-2.4-2.4zm3.1 3.7a.7.7 0 000 1.4h2.8a.7.7 0 000-1.4H8.4z" />
    </svg>
  );
}

export function LayersFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M8 1.5L14.5 4.8 8 8.1 1.5 4.8 8 1.5z" />
      <path d="M1.5 7.3L8 10.6l6.5-3.3.9.7L8 11.9 .6 8l.9-.7z" />
      <path d="M1.5 10.5L8 13.8l6.5-3.3.9.7L8 15.1.6 11.2l.9-.7z" />
    </svg>
  );
}

export function CubeFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M8 1.5l6 3.3-6 3.3-6-3.3 6-3.3z" />
      <path d="M1.4 6l5.9 3.2v5.9L2 12.1a1 1 0 01-.6-.9V6z" />
      <path d="M14.6 6v5.2a1 1 0 01-.6.9l-5.3 3V9.2L14.6 6z" />
    </svg>
  );
}

export function FolderFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M1.5 5V3.9a1.4 1.4 0 011.4-1.4h3.4l1.9 2.5h6a1.4 1.4 0 011.4 1.4v6a1.4 1.4 0 01-1.4 1.4H2.9a1.4 1.4 0 01-1.4-1.4V5z" />
    </svg>
  );
}

export function FileFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M9.4 1.4H4.2A1.6 1.6 0 002.6 3v10a1.6 1.6 0 001.6 1.6h7.7a1.6 1.6 0 001.6-1.6V6h-3.1a1 1 0 01-1-1V1.4z" />
      <path d="M10.4 1.6l3 3h-2.4a.6.6 0 01-.6-.6V1.6z" />
    </svg>
  );
}

export function PlayFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M5.6 2.5a1 1 0 00-1.5.9v9.2a1 1 0 001.5.9l7.2-4.6a1 1 0 000-1.8L5.6 2.5z" />
    </svg>
  );
}

export function PauseFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M4.5 2.6h1.9a1 1 0 011 1v8.8a1 1 0 01-1 1H4.5a1 1 0 01-1-1V3.6a1 1 0 011-1z" />
      <path d="M9.6 2.6h1.9a1 1 0 011 1v8.8a1 1 0 01-1 1H9.6a1 1 0 01-1-1V3.6a1 1 0 011-1z" />
    </svg>
  );
}

export function StopFilledIcon() {
  return (
    <svg {...solid}>
      <rect x="2.9" y="2.9" width="10.2" height="10.2" rx="2.1" />
    </svg>
  );
}

export function SquareFilledIcon() {
  return (
    <svg {...solid}>
      <rect x="1.6" y="1.6" width="12.8" height="12.8" rx="2.8" />
    </svg>
  );
}

export function BoltFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M9.3 1.5L3 9.3h4.4l-.6 5.2 6.3-7.8H8.6l.7-5.2z" />
    </svg>
  );
}

export function SparkleFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M8 1.5l1.8 4.7 4.7 1.8-4.7 1.8L8 14.5l-1.8-4.7L1.5 8l4.7-1.8L8 1.5z" />
    </svg>
  );
}

export function SunFilledIcon() {
  return (
    <svg {...solid}>
      <circle cx="8" cy="8" r="3.5" />
      <path d="M8 .6a.8.8 0 01.8.8v1.6a.8.8 0 01-1.6 0V1.4A.8.8 0 018 .6zm0 11.6a.8.8 0 01.8.8v1.6a.8.8 0 01-1.6 0V13a.8.8 0 01.8-.8zM.6 8a.8.8 0 01.8-.8H3a.8.8 0 010 1.6H1.4A.8.8 0 01.6 8zm11.6 0a.8.8 0 01.8-.8h1.6a.8.8 0 010 1.6H13a.8.8 0 01-.8-.8zM2.7 2.7a.8.8 0 011.1 0l1.1 1.1a.8.8 0 01-1.1 1.1L2.7 3.8a.8.8 0 010-1.1zm8.4 8.4a.8.8 0 011.1 0l1.1 1.1a.8.8 0 01-1.1 1.1l-1.1-1.1a.8.8 0 010-1.1zm2.2-8.4a.8.8 0 010 1.1l-1.1 1.1a.8.8 0 01-1.1-1.1l1.1-1.1a.8.8 0 011.1 0zm-8.4 8.4a.8.8 0 010 1.1l-1.1 1.1a.8.8 0 01-1.1-1.1l1.1-1.1a.8.8 0 011.1 0z" />
    </svg>
  );
}

export function MoonFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M13.8 9.7A6.5 6.5 0 016.3 2.2a6.5 6.5 0 107.5 7.5z" />
    </svg>
  );
}

export function DropletFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M8 1.2l4.2 5.4a5.3 5.3 0 11-8.4 0L8 1.2z" />
    </svg>
  );
}

export function FireFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M8 1.5S3.9 4.9 3.9 9a4.1 4.1 0 008.1 0c0-1.5-.7-2.9-1.7-3.9-.4 1.1-1.1 1.8-1.8 1.8.7-2.3-.5-5.3-.5-5.3z" />
    </svg>
  );
}

export function LeafFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M2.2 13.8S1.6 9 4.9 5.7 13.8 2.2 13.8 2.2s.5 5.5-2.8 8.8-8.8 2.8-8.8 2.8z" />
    </svg>
  );
}

export function CloudFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M4.2 13.1h7.5a3.2 3.2 0 00.4-6.4 4.6 4.6 0 00-8.7-1 3.2 3.2 0 00.8 7.4z" />
    </svg>
  );
}

export function ThumbUpFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M4.6 14.4V7.2l3.6-5.8a2 2 0 011.7 3l-.8 2.6h3.9a1.8 1.8 0 011.7 2.3l-1.2 4.4a1.8 1.8 0 01-1.7 1.3H4.6z" />
      <path d="M3.6 7.2H2.2a.8.8 0 00-.8.8v5.6a.8.8 0 00.8.8h1.4V7.2z" />
    </svg>
  );
}

export function ThumbDownFilledIcon() {
  return (
    <svg {...solid}>
      <path d="M4.6 1.6v7.2l3.6 5.8a2 2 0 001.7-3l-.8-2.6h3.9a1.8 1.8 0 001.7-2.3l-1.2-4.4a1.8 1.8 0 00-1.7-1.3H4.6z" />
      <path d="M3.6 8.8H2.2a.8.8 0 01-.8-.8V2.4a.8.8 0 01.8-.8h1.4v7.2z" />
    </svg>
  );
}
