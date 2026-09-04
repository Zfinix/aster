import { exec } from "child_process";
import { runCli } from "./asterCli";
import { ApiKey, ChatMessage, InfoRow, McpServer, Provider, SetupInfo, TranscriptTurn } from "./protocol";

async function json<T>(
  args: string[],
  cwd: string,
  env?: NodeJS.ProcessEnv,
  stdin?: string
): Promise<T> {
  const { stdout, stderr, code } = await runCli([...args, "--json"], cwd, stdin, env);
  let parsed: unknown;
  try {
    parsed = JSON.parse(stdout);
  } catch {
    throw new Error(
      stderr.trim() || `aster ${args.slice(0, 2).join(" ")} failed (exit ${code})`
    );
  }
  const failure = parsed as { ok?: boolean; error?: string };
  if (failure.ok === false) {
    throw new Error(failure.error ?? `aster ${args.slice(0, 2).join(" ")} failed`);
  }
  return parsed as T;
}

interface StatusJson {
  model: string;
  provider: string;
  base_url: string;
  effort: string;
  mode: string;
  limits: { max_tool_rounds: number; command_timeout_secs: number; compact_budget_chars: number };
  mcp: { servers: { name: string; disabled: boolean }[]; enabled: number };
  memory_blocks: number | null;
  sessions: number | null;
  skills: number;
}

/** The TUI's `/status` rows. Session-local facts (context, usage) are the
 *  panel's to add: the CLI has no view of a conversation it is not running. */
export async function status(cwd: string, env?: NodeJS.ProcessEnv): Promise<InfoRow[]> {
  const s = await json<StatusJson>(["status"], cwd, env);
  const servers = s.mcp.servers.length;
  return [
    { label: "model", value: s.model },
    { label: "provider", value: `${s.provider} · ${s.base_url}` },
    { label: "effort", value: s.effort },
    { label: "mode", value: s.mode },
    {
      label: "context",
      value: `${humanCount(s.limits.compact_budget_chars)} chars before auto-compact`,
    },
    { label: "rounds", value: `${s.limits.max_tool_rounds} tool rounds per turn` },
    {
      label: "mcp",
      value: servers === 0 ? "none configured" : `${s.mcp.enabled} of ${servers} enabled`,
    },
    { label: "skills", value: `${s.skills}` },
    { label: "memory", value: s.memory_blocks === null ? "unavailable" : `${s.memory_blocks} blocks` },
    { label: "sessions", value: s.sessions === null ? "unavailable" : `${s.sessions}` },
  ];
}

/** The history size the CLI auto-compacts above, so the panel can show how much
 *  of it the conversation has spent. Zero when the CLI cannot be asked. */
export async function contextBudget(cwd: string, env?: NodeJS.ProcessEnv): Promise<number> {
  try {
    const s = await json<StatusJson>(["status"], cwd, env);
    return s.limits.compact_budget_chars;
  } catch {
    return 0;
  }
}

export async function memoryBlocks(cwd: string): Promise<InfoRow[]> {
  const parsed = await json<{ blocks?: { name: string; description: string }[] }>(
    ["memory", "list"],
    cwd
  );
  return (parsed.blocks ?? []).map((b) => ({ label: b.name, value: b.description }));
}

export async function mcpServers(cwd: string): Promise<McpServer[]> {
  const parsed = await json<{ servers?: McpServer[] }>(["mcp", "list", "--no-connect"], cwd);
  return parsed.servers ?? [];
}

export async function toggleMcp(cwd: string, name: string, disabled: boolean): Promise<void> {
  await json(["mcp", disabled ? "disable" : "enable", name], cwd);
}

export async function providers(cwd: string, env?: NodeJS.ProcessEnv): Promise<Provider[]> {
  const parsed = await json<{ providers?: Provider[] }>(["models", "--providers"], cwd, env);
  return parsed.providers ?? [];
}

/** Point every surface at an endpoint: written to aster.yaml via the CLI, the
 *  file the terminal, desktop, and this panel all read. */
export async function useProvider(cwd: string, baseUrl: string, model?: string): Promise<void> {
  const args = ["provider", "use", baseUrl];
  if (model) args.push("--model", model);
  await json(args, cwd);
}

/** Save the model where every surface reads it. */
export async function useModel(cwd: string, model: string): Promise<void> {
  await json(["model", "use", model], cwd);
}

/** The model the next turn resolves to, from the same config the CLI reads. */
export async function currentModel(cwd: string): Promise<string | null> {
  const parsed = await json<{ model?: string }>(["config", "model"], cwd);
  return parsed.model ?? null;
}

interface KeyStatus {
  configured?: boolean;
  setup?: SetupInfo | null;
  vars?: { var: string; source: string }[];
}

/** Whether the current endpoint has a key or a login. Older CLIs report only
 *  the vars, which misses a ChatGPT login; the newer `configured` flag wins. */
export async function hasKey(cwd: string, env?: NodeJS.ProcessEnv): Promise<boolean> {
  const parsed = await json<KeyStatus>(["config", "key"], cwd, env);
  if (typeof parsed.configured === "boolean") return parsed.configured;
  return (parsed.vars ?? []).some((v) => v.source !== "unset");
}

/** The login or key the endpoint still needs; null once it has one. */
export async function setupNeeded(cwd: string, env?: NodeJS.ProcessEnv): Promise<SetupInfo | null> {
  const parsed = await json<KeyStatus>(["config", "key"], cwd, env);
  return parsed.setup ?? null;
}

/** Every key Aster reads, set or not, masked. Values never travel here. */
export async function apiKeys(cwd: string): Promise<ApiKey[]> {
  const parsed = await json<{ keys?: ApiKey[] }>(["key", "list", "--all"], cwd);
  return parsed.keys ?? [];
}

/** The value travels on stdin, never argv, so it stays off the process list
 *  and out of any error the CLI reports. */
export async function setApiKey(
  cwd: string,
  name: string,
  value: string,
  scope: "global" | "local"
): Promise<void> {
  const args = ["key", "set", name, "--stdin"];
  if (scope === "local") args.push("--local");
  await json(args, cwd, undefined, value);
}

export async function unsetApiKey(
  cwd: string,
  name: string,
  scope: "global" | "local"
): Promise<void> {
  await json(["key", "unset", name, `--${scope}`], cwd);
}

/** The live value, for the settings page's reveal button. */
export async function revealApiKey(cwd: string, name: string): Promise<string | null> {
  const parsed = await json<{ value?: string | null }>(["key", "get", name], cwd);
  return parsed.value ?? null;
}

/** The endpoint's catalog. A provider that will not answer is not fatal: the
 *  picker still switched, and a model can be typed by hand. */
export async function modelsFor(
  cwd: string,
  model: string,
  env?: NodeJS.ProcessEnv
): Promise<string[]> {
  try {
    const { stdout, code } = await runCli(
      ["models", "--model", model, "--json"],
      cwd,
      undefined,
      env
    );
    return code === 0 ? (JSON.parse(stdout) as string[]) : [];
  } catch {
    return [];
  }
}

export interface Compacted {
  summary: string;
  folded: number;
  messages: TranscriptTurn[];
}

/** Fold the head of a transcript into a summary. The panel owns its history, so
 *  the shorter one comes back for it to adopt rather than being applied here. */
export async function compact(
  cwd: string,
  messages: ChatMessage[],
  model: string | null,
  env?: NodeJS.ProcessEnv
): Promise<Compacted> {
  const args = ["chat", "--messages-json", "-", "--compact", "--json"];
  if (model) {
    args.push("--model", model);
  }
  const { stdout, stderr, code } = await runCli(args, cwd, JSON.stringify(messages), env);
  let parsed: { ok?: boolean; error?: string } & Partial<Compacted>;
  try {
    parsed = JSON.parse(stdout);
  } catch {
    throw new Error(stderr.trim() || `compacting failed (exit ${code})`);
  }
  if (parsed.ok === false || !parsed.messages) {
    throw new Error(parsed.error ?? "compacting failed");
  }
  return { summary: parsed.summary ?? "", folded: parsed.folded ?? 0, messages: parsed.messages };
}

/** Everything uncommitted, the same range the TUI's `/diff` shows. */
export function workingDiff(cwd: string): Promise<string> {
  return new Promise((resolve, reject) => {
    exec("git diff HEAD", { cwd, maxBuffer: 8 * 1024 * 1024 }, (err, stdout, stderr) => {
      if (err && !stdout) {
        reject(new Error(stderr.trim() || "could not run git"));
        return;
      }
      resolve(stdout);
    });
  });
}

function humanCount(n: number): string {
  return n >= 1000 ? `${Math.round(n / 1000)}k` : `${n}`;
}
