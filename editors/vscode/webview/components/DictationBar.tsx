import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { onHostMessage } from "../lib/host";
import type { DictationState } from "../lib/dictation";
import { IconMorphGlyph, sendStop } from "../interior/icon-morph";

const BARS = 5;
const QUIET = 0.004;
const LOUD = 0.25;

/** Loudness on the same log scale as the TUI meter: room hum sits flat, talk
 *  climbs. Levels are RMS from 0 to 1. */
function height(level: number): number {
  const scaled = Math.log10(level / QUIET) / Math.log10(LOUD / QUIET);
  return Math.min(Math.max(scaled, 0), 1);
}

/** Stands in for the composer's footer while the mic is on: a live meter and
 *  timer, a stop that keeps the words in the box, and a send that ships them. */
export function DictationBar({
  state,
  onStop,
  onSend,
  onDiscard,
}: {
  state: Exclude<DictationState, "idle">;
  onStop: () => void;
  onSend: () => void;
  onDiscard: () => void;
}) {
  const [levels, setLevels] = useState<number[]>(() => Array(BARS).fill(0));
  const [heard, setHeard] = useState("");
  const [started] = useState(Date.now);
  const [now, setNow] = useState(started);

  useEffect(
    () =>
      onHostMessage((message) => {
        if (message.type !== "dictation") return;
        const event = message.event;
        if (event.type === "level") setLevels((prev) => [...prev.slice(1), event.level]);
        else if (event.type === "partial") setHeard(event.text);
      }),
    [],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onDiscard();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onDiscard]);

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(timer);
  }, []);

  const listening = state === "listening";
  const secs = Math.floor((now - started) / 1000);
  const clock = `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;

  return (
    <div className="dictation-bar" data-state={state}>
      <span className="dictation-meter" aria-hidden="true">
        {levels.map((level, i) => (
          <span key={i} style={{ transform: `scaleY(${0.25 + 0.75 * height(level)})` }} />
        ))}
      </span>
      <span className="dictation-label" role="status">
        {heard ? (
          <Heard text={heard} />
        ) : listening ? (
          <>
            Listening <span className="dictation-time">{clock}</span>
            <span className="dictation-hint"> · Esc to discard</span>
          </>
        ) : (
          "Turning it into text…"
        )}
      </span>
      <button
        className="dictation-stop"
        onClick={onStop}
        disabled={!listening}
        title="Stop and keep the text"
        aria-label="Stop and keep the text"
      >
        <IconMorphGlyph shapes={sendStop} active={1} />
      </button>
      <button className="send" onClick={onSend} title="Stop and send" aria-label="Stop and send">
        <IconMorphGlyph shapes={sendStop} active={0} />
      </button>
    </div>
  );
}

/** The words so far, newest at the right edge. The last word is still a guess,
 *  so it stays dim until the next one arrives. */
function Heard({ text }: { text: string }) {
  const ref = useRef<HTMLSpanElement>(null);
  const [overflow, setOverflow] = useState(false);
  useLayoutEffect(() => {
    const el = ref.current;
    if (el) setOverflow(el.scrollWidth > el.clientWidth);
  }, [text]);
  const cut = text.trimEnd().lastIndexOf(" ") + 1;
  return (
    <span ref={ref} className="dictation-heard" data-overflow={overflow}>
      <span>
        {text.slice(0, cut)}
        <span className="dictation-guess">{text.slice(cut)}</span>
      </span>
    </span>
  );
}
