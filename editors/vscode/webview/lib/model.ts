/** Mirrors the desktop app's model naming; see desktop/src/lib/session.ts. */

/** Humanize one slug token by rule, not by lookup:
 *  "3.1" -> "3.1", "v1" -> "v1", "80b"/"a3b" -> "80B"/"A3B",
 *  "gpt"/"glm" -> "GPT"/"GLM" (no vowels = acronym), "qwen3" -> "Qwen3". */
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

/** "google/gemini-3.1-flash-lite" -> "google" */
export function modelProvider(id: string): string {
  return id.includes("/") ? id.split("/")[0] : "";
}
