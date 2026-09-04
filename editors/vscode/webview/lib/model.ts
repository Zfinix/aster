/** Mirrors the desktop app's model naming; see desktop/src/lib/session.ts. */

function caseToken(word: string): string {
  if (/^[0-9.]+$/.test(word)) return word;
  if (/^v\d+$/i.test(word)) return word.toLowerCase();
  if (/\d/.test(word) && word.length <= 3) return word.toUpperCase();
  if (/^[a-z]+$/i.test(word) && !/[aeiouy]/i.test(word)) return word.toUpperCase();
  return word.charAt(0).toUpperCase() + word.slice(1);
}

/** "google/gemini-3.1-flash-lite" -> "Gemini 3.1 Flash Lite" */
export function modelShort(id: string | null): string {
  if (!id) return "Default";
  const slug = id.split("/").pop() || id;
  return slug.split("-").map(caseToken).join(" ");
}

/** The composer chip's name: "claude-fable-5-1" -> "Fable 5.1". The family
 *  prefix and a trailing date stamp are noise at chip size, and split version
 *  digits read as one number. */
export function modelChip(id: string | null): string {
  if (!id) return "Default";
  const slug = id.split("/").pop() || id;
  const tokens = slug
    .split("-")
    .filter((word, at) => !(at === 0 && word === "claude") && !/^\d{8}$/.test(word));
  const out: string[] = [];
  for (const word of tokens) {
    const last = out[out.length - 1];
    if (/^\d+$/.test(word) && last !== undefined && /^\d+(\.\d+)*$/.test(last)) {
      out[out.length - 1] = `${last}.${word}`;
    } else {
      out.push(caseToken(word));
    }
  }
  return out.join(" ") || modelShort(id);
}

/** "google/gemini-3.1-flash-lite" -> "google" */
export function modelProvider(id: string): string {
  return id.includes("/") ? id.split("/")[0] : "";
}

export function recentsFor(recent: string[], catalog: string[] | null): string[] {
  if (!catalog || catalog.length === 0) return recent;
  return recent.filter((id) => catalog.includes(id));
}
