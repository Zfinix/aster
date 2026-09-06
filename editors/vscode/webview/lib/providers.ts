import type { Provider } from "../../src/protocol";

/** How an endpoint gets its credentials from the panel. Sign-in is decided the
 *  way the CLI decides it, by host; z.ai's login needs a pasted URL on stdin,
 *  so from the panel it takes a key like everyone else. */
export type ProviderAuth =
  | { kind: "login"; target: "openrouter" | "codex" }
  | { kind: "key"; keyVar: string }
  | { kind: "none" };

export function providerAuth(provider: Provider): ProviderAuth {
  const host = hostOf(provider.base_url);
  if (host.includes("openrouter")) return { kind: "login", target: "openrouter" };
  if (host === "chatgpt.com" && provider.base_url.includes("/codex")) {
    return { kind: "login", target: "codex" };
  }
  const keyVar = provider.key_env[0];
  return keyVar ? { kind: "key", keyVar } : { kind: "none" };
}

/** The catalog's names carry a note in brackets ("Ollama (local)"); the row
 *  shows the name and keeps the note for its second line. */
export function providerLabel(provider: Provider): { label: string; note: string | null } {
  const match = /^(.*?)\s*\((.*)\)\s*$/.exec(provider.name);
  if (!match) return { label: provider.name, note: null };
  const note = match[2].trim();
  return { label: match[1].trim(), note: /^local$/i.test(note) ? null : note };
}

export function providerDetail(provider: Provider): string {
  const { note } = providerLabel(provider);
  const how = {
    login: "Sign in with your browser",
    key: "Paste an API key",
    none: "Runs on this machine",
  }[providerAuth(provider).kind];
  return note ? `${note} · ${how}` : how;
}

const SHORTLIST_HOSTS = [
  "openrouter.ai",
  "chatgpt.com",
  "api.openai.com",
  "api.anthropic.com",
  "generativelanguage.googleapis.com",
  "api.deepseek.com",
];

/** The first screen: sign-in first, then the big four, in that order. Anything
 *  else waits behind "More providers". */
export function shortlist(providers: Provider[]): { first: Provider[]; rest: Provider[] } {
  const first = SHORTLIST_HOSTS.flatMap((host) =>
    providers.filter((p) => hostOf(p.base_url) === host)
  );
  if (first.length === 0) return { first: providers, rest: [] };
  const rest = providers.filter((p) => !first.includes(p));
  return { first, rest };
}

const KEY_PAGES: Record<string, string> = {
  "api.openai.com": "https://platform.openai.com/api-keys",
  "api.anthropic.com": "https://console.anthropic.com/settings/keys",
  "generativelanguage.googleapis.com": "https://aistudio.google.com/apikey",
  "api.deepseek.com": "https://platform.deepseek.com/api_keys",
  "api.x.ai": "https://console.x.ai",
  "api.mistral.ai": "https://console.mistral.ai/api-keys",
  "api.groq.com": "https://console.groq.com/keys",
  "api.together.xyz": "https://api.together.ai/settings/api-keys",
  "api.fireworks.ai": "https://fireworks.ai/account/api-keys",
  "api.moonshot.ai": "https://platform.moonshot.ai/console/api-keys",
  "api.z.ai": "https://z.ai/manage-apikey/apikey-list",
  "api.perplexity.ai": "https://www.perplexity.ai/settings/api",
  "api.cohere.ai": "https://dashboard.cohere.com/api-keys",
  "router.huggingface.co": "https://huggingface.co/settings/tokens",
  "models.github.ai": "https://github.com/settings/tokens",
};

export function keyPage(provider: Provider): string | null {
  return KEY_PAGES[hostOf(provider.base_url)] ?? null;
}

function hostOf(url: string): string {
  try {
    return new URL(url).host.toLowerCase();
  } catch {
    return url.toLowerCase();
  }
}
