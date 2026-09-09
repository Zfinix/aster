import { play, setEnabled, type SoundName } from "cuelume";

const KEY = "aster.sounds";

/** The sounds a finished turn or review can land on. */
const COMPLETION_SOUNDS = ["sparkle", "ready", "success", "arrival", "bloom"] as const;

let completion: SoundName = "sparkle";

/** Sounds are on unless the user muted them. In the editor the preference is
 *  the `aster.sounds` setting and the host pushes it on init; localStorage is
 *  the cache here and the only store in the browser devhost. */
export function soundsEnabled(): boolean {
  try {
    return localStorage.getItem(KEY) !== "off";
  } catch {
    return true;
  }
}

export function setSoundsEnabled(on: boolean): void {
  try {
    if (on) localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, "off");
  } catch {
    // No storage: the toggle still applies for this session.
  }
  setEnabled(on);
}

export function setCompletionSound(name: string): void {
  if ((COMPLETION_SOUNDS as readonly string[]).includes(name)) {
    completion = name as SoundName;
  }
}

/** The cue for a finished turn or review. */
export function playCompletion(): void {
  play(completion);
}

/** Once at startup, before anything can play. */
export function initSounds(): void {
  setEnabled(soundsEnabled());
}

export type { SoundName };