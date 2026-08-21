import { parts } from "../lib/greeting";

/** A tip's keys and controls sit in chips, the way a keycap reads, with the
 *  prose between them left to wrap on its own. */
export function Tip({ text }: { text: string }) {
  return (
    <>
      {parts(text).map((part, i) =>
        part.chip ? (
          <kbd key={i} className="tip-chip">
            {part.text}
          </kbd>
        ) : (
          <span key={i}>{part.text}</span>
        ),
      )}
    </>
  );
}
