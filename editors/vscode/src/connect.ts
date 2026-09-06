import { runCli, type RunResult } from "./asterCli";
import * as info from "./info";
import type { Provider } from "./protocol";

export type ConnectOutcome = { ok: true } | { ok: false; message: string };

/** What a server on this machine gets instead of a key: the CLI resolves a key
 *  for every endpoint, so `aster init` stores this placeholder for local ones. */
const LOCAL_KEY = "local";

/** Asks the endpoint for its model list with the key it is about to be given,
 *  writing nothing. The key rides in as `ASTER_API_KEY` and as the provider's
 *  own vars, since those win and the CLI would otherwise fill them from an
 *  older env file. */
export async function probe(
  cwd: string,
  provider: Provider,
  key: string | null,
  env: NodeJS.ProcessEnv
): Promise<ConnectOutcome> {
  const value = key ?? LOCAL_KEY;
  const probeEnv: NodeJS.ProcessEnv = {
    ...env,
    ASTER_BASE_URL: provider.base_url,
    ASTER_API_KEY: value,
  };
  for (const name of provider.key_env) probeEnv[name] = value;

  let out: RunResult;
  try {
    out = await runCli(["models", "--json"], cwd, undefined, probeEnv);
  } catch (err) {
    return { ok: false, message: err instanceof Error ? err.message : String(err) };
  }
  if (out.code === 0) return { ok: true };
  return { ok: false, message: explain(provider, key !== null, cliError(out)) };
}

/** Makes the endpoint the one in use and stores its key where the CLI reads
 *  it, in the user's own files rather than this workspace's. A local server
 *  gets the placeholder only if the CLI still asks for a key afterwards. */
export async function persist(
  cwd: string,
  provider: Provider,
  model: string,
  key: string | null,
  env: NodeJS.ProcessEnv
): Promise<void> {
  await info.useProvider(cwd, provider.base_url, model);
  const keyVar = provider.key_env[0];
  if (key && keyVar) {
    await info.setApiKey(cwd, keyVar, key, "global");
  } else if (!key && (await info.setupNeeded(cwd, env).catch(() => null))) {
    await info.setApiKey(cwd, "ASTER_API_KEY", LOCAL_KEY, "global");
  }
}

function cliError(out: RunResult): string {
  try {
    const parsed = JSON.parse(out.stdout.trim()) as { error?: unknown };
    if (typeof parsed.error === "string" && parsed.error) return parsed.error;
  } catch {
    // Not JSON: the CLI died before it could answer in kind.
  }
  return out.stderr.trim() || `aster exited with code ${out.code}`;
}

function explain(provider: Provider, hadKey: boolean, raw: string): string {
  const name = provider.name.replace(/\s*\(.*\)\s*$/, "");
  if (/authentication failed|unauthori[sz]ed|invalid.*api key|incorrect api key|\b401\b/i.test(raw)) {
    return `${name} rejected that key. Check it and try again.`;
  }
  if (/error sending request|connection refused|dns error|could not connect|timed out|no route|network/i.test(raw)) {
    return hadKey
      ? `Nothing answered at ${provider.base_url}. Check your connection and try again.`
      : `Nothing answered at ${provider.base_url}. Start it and try again.`;
  }
  return raw.split("\n")[0];
}
