import { runCli } from "./asterCli";
import { ConfigKey, ConfigKind, ConfigPaths, ConfigScope } from "./protocol";

/** `aster config <args> --json`, parsed, with the CLI's own error on failure. */
async function json<T>(args: string[], cwd: string): Promise<T> {
  const { stdout, stderr, code } = await runCli(["config", ...args, "--json"], cwd);
  let parsed: unknown;
  try {
    parsed = JSON.parse(stdout);
  } catch {
    throw new Error(stderr.trim() || `aster config ${args.join(" ")} failed (exit ${code})`);
  }
  const failure = parsed as { ok?: boolean; error?: string };
  if (failure.ok === false) {
    throw new Error(failure.error ?? `aster config ${args.join(" ")} failed`);
  }
  return parsed as T;
}

/** A CLI older than the settings panel omits `kind`, so the control a row gets
 *  is inferred from what the value and the documented default look like. */
function inferKind(row: Partial<ConfigKey>): ConfigKind {
  const sample = row.value ?? row.default;
  if (Array.isArray(row.value)) return "list";
  if (typeof sample === "boolean" || sample === "true" || sample === "false") return "bool";
  if (typeof sample === "number") return "number";
  return "text";
}

function normalize(row: Partial<ConfigKey> & { key: string }): ConfigKey {
  const choices = row.choices ?? [];
  return {
    key: row.key,
    label: row.label ?? row.key,
    group: row.group ?? "Other",
    kind: row.kind ?? (choices.length > 0 ? "choice" : inferKind(row)),
    choices,
    unit: row.unit ?? "none",
    value: row.value ?? null,
    display: row.display ?? "",
    default: row.default ?? "",
    source: row.source ?? "default",
    shadowed: row.shadowed ?? null,
    scopes: row.scopes ?? { global: null, local: null },
    env: row.env ?? [],
    help: row.help ?? "",
  };
}

export async function list(cwd: string): Promise<ConfigKey[]> {
  const parsed = await json<{ keys?: (Partial<ConfigKey> & { key: string })[] }>(["list"], cwd);
  return (parsed.keys ?? []).map(normalize);
}

export async function paths(cwd: string): Promise<ConfigPaths> {
  return await json<ConfigPaths>(["path"], cwd);
}

/** A list key takes its value comma-separated; an empty string empties it. */
function encode(value: string | number | boolean | string[]): string {
  return Array.isArray(value) ? value.join(",") : String(value);
}

export async function set(
  cwd: string,
  key: string,
  value: string | number | boolean | string[],
  scope: ConfigScope
): Promise<void> {
  await json(["set", key, encode(value), `--${scope}`], cwd);
}

export async function unset(cwd: string, key: string, scope: ConfigScope): Promise<void> {
  await json(["unset", key, `--${scope}`], cwd);
}
