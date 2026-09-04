import type { SetupInfo } from "../../src/protocol";

/** Progress of one `aster login` run started from the panel. */
export interface LoginState {
  lines: string[];
  done?: boolean;
  ok?: boolean;
  message?: string;
}

/** How many lines of login output to keep on screen; the flow prints a handful. */
const MAX_LINES = 12;

export function loginLine(prev: LoginState | null, line: string): LoginState {
  const lines = [...(prev?.lines ?? []), line].slice(-MAX_LINES);
  return { lines };
}

export interface SetupCopy {
  title: string;
  body: string;
  /** Label for the browser sign-in button, absent when the endpoint has none. */
  signIn?: string;
  /** The env var to fill instead, when a key is an option. */
  keyVar?: string;
}

const PROVIDER_LABEL: Record<string, string> = {
  codex: "ChatGPT",
  openrouter: "OpenRouter",
  zai: "Z.ai",
};

/** The plain-language card for an endpoint that has no credentials yet. */
export function setupCopy(setup: SetupInfo): SetupCopy {
  const keyVar = setup.key_vars[0];
  if (setup.login === "codex") {
    return {
      title: "Sign in to ChatGPT",
      body: "Aster runs this model on your ChatGPT subscription. Sign in once and you're set.",
      signIn: "Sign in with ChatGPT",
    };
  }
  if (setup.login) {
    const name = PROVIDER_LABEL[setup.login] ?? setup.provider;
    return {
      title: `Sign in to ${name}`,
      body: `Aster needs a ${name} account before it can answer. Sign in, or add an API key.`,
      signIn: `Sign in with ${name}`,
      keyVar,
    };
  }
  return {
    title: `Add an API key for ${setup.provider}`,
    body: `Aster needs a key for ${setup.provider} before it can answer.`,
    keyVar,
  };
}
