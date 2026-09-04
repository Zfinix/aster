import type { CSSProperties } from "react";

/** The word as a small constellation: letters rise into place the way stars
 *  come out, then twinkle on their own phase. Remounting on `key` replays the
 *  rise, so a new word appears rather than swapping. */
export function StatusWord({ word }: { word: string }) {
  return (
    <>
      <span className="sr-only">{word}</span>
      <span className="status-word" key={word} aria-hidden="true">
        {[...word].map((char, i) => (
          <span
            key={i}
            className="status-letter"
            style={{ "--i": i } as CSSProperties}
          >
            {char}
          </span>
        ))}
      </span>
    </>
  );
}
