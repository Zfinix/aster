const stroke = {
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.5,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};

export function ChevronIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 16 16" aria-hidden="true">
      <path d="M4 6.5 8 10.5l4-4" {...stroke} />
    </svg>
  );
}

export function CloseIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 16 16" aria-hidden="true">
      <path d="M4 4l8 8M12 4l-8 8" {...stroke} />
    </svg>
  );
}

export function ResetIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 16 16" aria-hidden="true">
      <path d="M3 8a5 5 0 1 0 1.6-3.7M3 3v2.5h2.5" {...stroke} />
    </svg>
  );
}

export function SearchIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 16 16" aria-hidden="true">
      <circle cx="7" cy="7" r="4.25" {...stroke} />
      <path d="M10.2 10.2 13.5 13.5" {...stroke} />
    </svg>
  );
}

export function EyeIcon({ off }: { off?: boolean }) {
  return (
    <svg width="12" height="12" viewBox="0 0 16 16" aria-hidden="true">
      <path d="M1.8 8C3.3 5 5.5 3.5 8 3.5S12.7 5 14.2 8C12.7 11 10.5 12.5 8 12.5S3.3 11 1.8 8Z" {...stroke} />
      <circle cx="8" cy="8" r="1.9" {...stroke} />
      {off && <path d="M3.2 12.8 12.8 3.2" {...stroke} />}
    </svg>
  );
}

export function WarnIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 16 16" aria-hidden="true">
      <path d="M8 2.5 14.5 13.5H1.5L8 2.5ZM8 6.5v3M8 11.5v.5" {...stroke} />
    </svg>
  );
}
