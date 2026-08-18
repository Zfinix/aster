const base = {
  width: 14,
  height: 14,
  viewBox: "0 0 16 16",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.4,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  "aria-hidden": true,
};

export function AsterIcon() {
  return (
    <svg {...base}>
      <path d="M8 2v12M2.9 5l10.2 6M13.1 5L2.9 11" />
    </svg>
  );
}

export function SendIcon() {
  return (
    <svg {...base}>
      <path d="M8 13V3M3.5 7.5L8 3l4.5 4.5" />
    </svg>
  );
}

/** The slash that opens the command menu, boxed the way its shortcut reads. */
export function CommandIcon() {
  return (
    <svg {...base}>
      <rect x="2.5" y="2.5" width="11" height="11" rx="2.5" />
      <path d="M9.5 5.5l-3 5" />
    </svg>
  );
}

export function ShieldIcon() {
  return (
    <svg {...base}>
      <path d="M8 2l4.5 1.7v4.1c0 2.6-1.8 4.9-4.5 6-2.7-1.1-4.5-3.4-4.5-6V3.7L8 2z" />
    </svg>
  );
}

export function ChevronIcon({ open }: { open: boolean }) {
  return (
    <svg {...base} style={{ transform: open ? "rotate(90deg)" : undefined }}>
      <path d="M6 3.5L10.5 8 6 12.5" />
    </svg>
  );
}

/** Points at the menu it opens, which sits above the composer. */
export function CaretUpIcon({ open }: { open: boolean }) {
  return (
    <svg {...base} style={{ transform: open ? "rotate(180deg)" : undefined }}>
      <path d="M4 9.5L8 5.5l4 4" />
    </svg>
  );
}

export function ReviewIcon() {
  return (
    <svg {...base}>
      <path d="M2.5 4h11M2.5 8h7M2.5 12h4" />
    </svg>
  );
}

export function FileIcon() {
  return (
    <svg {...base}>
      <path d="M9 2H4.5A1.5 1.5 0 003 3.5v9A1.5 1.5 0 004.5 14h7a1.5 1.5 0 001.5-1.5V6L9 2z" />
      <path d="M9 2v4h4" />
    </svg>
  );
}

export function LayersIcon() {
  return (
    <svg {...base}>
      <path d="M8 1.8L1.8 5 8 8.2 14.2 5 8 1.8z" />
      <path d="M1.8 8.2L8 11.4l6.2-3.2M1.8 11.2L8 14.4l6.2-3.2" />
    </svg>
  );
}

export function FolderIcon() {
  return (
    <svg {...base}>
      <path d="M2.5 12.5v-9h4l1.5 2h5.5v7a.5.5 0 01-.5.5h-10a.5.5 0 01-.5-.5z" />
    </svg>
  );
}

export function SearchIcon() {
  return (
    <svg {...base}>
      <circle cx="7" cy="7" r="4" />
      <path d="M10 10l3.5 3.5" />
    </svg>
  );
}

export function PencilIcon() {
  return (
    <svg {...base}>
      <path d="M11.5 2.5l2 2L6 12l-3 1 1-3 7.5-7.5z" />
    </svg>
  );
}

export function TerminalIcon() {
  return (
    <svg {...base}>
      <rect x="2" y="3" width="12" height="10" rx="1.3" />
      <path d="M5 7l1.8 1.8L5 10.6M8.8 10.6h2.4" />
    </svg>
  );
}

export function BrainIcon() {
  return (
    <svg {...base}>
      <path d="M8 3.5a2 2 0 00-3.6 1.2A2 2 0 003 8a2 2 0 001.6 2 2 2 0 003.4 1.4V3.5z" />
      <path d="M8 3.5a2 2 0 013.6 1.2A2 2 0 0113 8a2 2 0 01-1.6 2A2 2 0 018 11.4" />
    </svg>
  );
}

export function BookIcon() {
  return (
    <svg {...base}>
      <path d="M3 3.5h3.5A1.5 1.5 0 018 5v8a1.2 1.2 0 00-1.2-1.2H3v-8.3z" />
      <path d="M13 3.5H9.5A1.5 1.5 0 008 5v8a1.2 1.2 0 011.2-1.2H13v-8.3z" />
    </svg>
  );
}

export function AlertIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="5.5" />
      <path d="M8 5.2v3.4M8 10.6v.6" />
    </svg>
  );
}

export function ExternalIcon() {
  return (
    <svg {...base}>
      <path d="M9 3h4v4M13 3L7.5 8.5" />
      <path d="M11.5 9.5v3a1 1 0 01-1 1h-7a1 1 0 01-1-1v-7a1 1 0 011-1h3" />
    </svg>
  );
}

export function SpinnerIcon() {
  return (
    <svg {...base} className="spinner-icon">
      <circle cx="8" cy="8" r="5.5" strokeDasharray="28" strokeDashoffset="8" />
    </svg>
  );
}

/** A speech bubble with a plus: a pencil reads as "edit this one", and what
 *  the button does is start another. */
export function NewChatIcon() {
  return (
    <svg {...base}>
      <path d="M14 7.4c0 2.7-2.7 4.9-6 4.9a7 7 0 0 1-1.8-.2L3 13.3l1-2.4C2.8 10 2 8.8 2 7.4c0-2.7 2.7-4.9 6-4.9s6 2.2 6 4.9z" />
      <path d="M8 5.6v3.6M6.2 7.4h3.6" />
    </svg>
  );
}

/** A clock glyph for reopening a saved session. */
export function HistoryIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="5.5" />
      <path d="M8 4.5V8l2.5 1.5" />
    </svg>
  );
}


/** Opens the add menu: files from disk, or a mention of one in the repo. */
export function PlusIcon() {
  return (
    <svg {...base}>
      <path d="M8 3.5v9M3.5 8h9" />
    </svg>
  );
}

/** The plan's step marks: one ring, filled in differently per state, so a
 *  column of them reads as one control rather than five unrelated glyphs. */
export function CircleIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="5.5" />
    </svg>
  );
}

export function CircleDotIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="5.5" />
      <circle cx="8" cy="8" r="2" fill="currentColor" stroke="none" />
    </svg>
  );
}

export function CircleCheckIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="5.5" fill="currentColor" fillOpacity="0.16" />
      <path d="M5.5 8.2l1.8 1.8 3.2-3.8" />
    </svg>
  );
}

export function CircleSkipIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="5.5" />
      <path d="M6.2 6l2.2 2-2.2 2M9.9 6v4" />
    </svg>
  );
}

export function CircleXIcon() {
  return (
    <svg {...base}>
      <circle cx="8" cy="8" r="5.5" fill="currentColor" fillOpacity="0.14" />
      <path d="M6.2 6.2l3.6 3.6M9.8 6.2l-3.6 3.6" />
    </svg>
  );
}

export function UploadIcon() {
  return (
    <svg {...base}>
      <path d="M8 10.5v-8M4.8 5.7L8 2.5l3.2 3.2" />
      <path d="M2.8 11v1.5A1.5 1.5 0 004.3 14h7.4a1.5 1.5 0 001.5-1.5V11" />
    </svg>
  );
}
