/* Inline glyphs matching the Aster mockup. Kept as a small local set so the
   chrome renders exactly as designed. */

type P = { size?: number };
const s = (n = 15) => ({ width: n, height: n, viewBox: "0 0 16 16" });

export const EditIcon = ({ size }: P) => (
  <svg {...s(size)} fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
    <path d="M11 2.5 L13.5 5 L6 12.5 L3 13 L3.5 10 Z" />
  </svg>
);

export const EyeIcon = ({ size }: P) => (
  <svg {...s(size)} fill="none" stroke="currentColor" strokeWidth="1.5">
    <circle cx="8" cy="8" r="2.2" />
    <path d="M1.5 8 Q4.5 3 8 3 Q11.5 3 14.5 8 Q11.5 13 8 13 Q4.5 13 1.5 8 Z" />
  </svg>
);

export const LayersIcon = ({ size }: P) => (
  <svg {...s(size)} fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
    <path d="M8 1.8 L14 5 L8 8.2 L2 5 Z" />
    <path d="M2 8 L8 11.2 L14 8" />
    <path d="M2 11 L8 14.2 L14 11" />
  </svg>
);

export const FolderIcon = ({ size = 14 }: P) => (
  <svg {...s(size)} fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
    <path d="M1.5 4 A1.5 1.5 0 0 1 3 2.5 H6 L7.5 4 H13 A1.5 1.5 0 0 1 14.5 5.5 V12 A1.5 1.5 0 0 1 13 13.5 H3 A1.5 1.5 0 0 1 1.5 12 Z" />
  </svg>
);

export const RepoIcon = ({ size = 13 }: P) => (
  <svg {...s(size)} fill="none" stroke="currentColor" strokeWidth="1.5">
    <rect x="2" y="3" width="12" height="10" rx="2" />
    <line x1="2" y1="6.5" x2="14" y2="6.5" />
  </svg>
);

export const BranchIcon = ({ size = 13 }: P) => (
  <svg {...s(size)} fill="none" stroke="currentColor" strokeWidth="1.5">
    <circle cx="5" cy="4" r="1.8" />
    <circle cx="5" cy="12" r="1.8" />
    <circle cx="11" cy="8" r="1.8" />
    <path d="M5 5.8v4.4M6.8 4.4Q11 5 11 6.2" />
  </svg>
);

export const AttachIcon = ({ size = 15 }: P) => (
  <svg {...s(size)} fill="none" stroke="currentColor" strokeWidth="1.5">
    <line x1="8" y1="2.5" x2="8" y2="13.5" />
    <line x1="2.5" y1="8" x2="13.5" y2="8" />
  </svg>
);

export const SendIcon = ({ size = 14 }: P) => (
  <svg {...s(size)} fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
    <path d="M8 13 V3 M4 7 L8 3 L12 7" />
  </svg>
);

export const DotsIcon = ({ size = 15 }: P) => (
  <svg {...s(size)} fill="currentColor">
    <circle cx="3" cy="8" r="1.3" />
    <circle cx="8" cy="8" r="1.3" />
    <circle cx="13" cy="8" r="1.3" />
  </svg>
);

export const ListIcon = ({ size = 15 }: P) => (
  <svg {...s(size)} fill="none" stroke="currentColor" strokeWidth="1.5">
    <rect x="2" y="1.8" width="12" height="12.4" rx="2" />
    <line x1="5" y1="5.5" x2="11" y2="5.5" />
    <line x1="5" y1="8" x2="11" y2="8" />
    <line x1="5" y1="10.5" x2="9" y2="10.5" />
  </svg>
);

export const SidebarIcon = ({ size = 15 }: P) => (
  <svg {...s(size)} fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
    <rect x="2" y="2.5" width="12" height="11" rx="2" />
    <line x1="6.5" y1="2.5" x2="6.5" y2="13.5" />
  </svg>
);

export const ExpandIcon = ({ size = 14 }: P) => (
  <svg {...s(size)} fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
    <path d="M9.5 2.5 H13.5 V6.5 M13.5 2.5 L9 7 M6.5 13.5 H2.5 V9.5 M2.5 13.5 L7 9" />
  </svg>
);

export const CloseIcon = ({ size = 15 }: P) => (
  <svg {...s(size)} fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
    <path d="M4 4 L12 12 M12 4 L4 12" />
  </svg>
);

export const GearIcon = ({ size = 16 }: P) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h0a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h0a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v0a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
  </svg>
);
