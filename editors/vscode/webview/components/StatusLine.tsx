import { useEffect, useState } from "react";
import { Mark } from "./Mark";
import { StatusWord } from "./StatusWord";

const WORDS = [
  "Astering",
  "Stargazing",
  "Orbiting",
  "Charting",
  "Triangulating",
  "Navigating",
  "Plotting",
  "Scanning",
  "Squinting",
  "Combing",
  "Sifting",
  "Untangling",
  "Unravelling",
  "Tracing",
  "Threading",
  "Poring",
  "Surveying",
  "Sounding",
  "Prospecting",
  "Excavating",
  "Decoding",
  "Whittling",
  "Winnowing",
  "Nitpicking",
  "Grokking",
  "Rummaging",
  "Foraging",
  "Circling",
  "Homing",
  "Aligning",
  "Calibrating",
  "Focusing",
  "Reckoning",
  "Deducing",
  "Sleuthing",
];

const ROTATE_MS = 3200;

function elapsedLabel(secs: number): string {
  return secs < 60 ? `${secs}s` : `${Math.floor(secs / 60)}m ${secs % 60}s`;
}

function anotherWord(current: string): string {
  const rest = WORDS.filter((w) => w !== current);
  return rest[Math.floor(Math.random() * rest.length)];
}

export function StatusLine() {
  const [word, setWord] = useState(() => WORDS[Math.floor(Math.random() * WORDS.length)]);
  const [elapsed, setElapsed] = useState(0);

  useEffect(() => {
    const started = Date.now();
    const tick = window.setInterval(
      () => setElapsed(Math.floor((Date.now() - started) / 1000)),
      1000
    );
    const rotate = window.setInterval(() => setWord(anotherWord), ROTATE_MS);
    return () => {
      window.clearInterval(tick);
      window.clearInterval(rotate);
    };
  }, []);

  return (
    <div className="status" role="status">
      <Mark px={1} className="mark-busy" label="Working" />
      <StatusWord word={word} />
      {elapsed >= 3 && <span className="status-elapsed">{elapsedLabel(elapsed)}</span>}
    </div>
  );
}
