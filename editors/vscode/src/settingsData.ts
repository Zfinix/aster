import * as vscode from "vscode";
import * as config from "./asterConfig";
import * as info from "./info";
import { checkBinary, cliConfig } from "./asterCli";
import { EditorSettings, SettingsSnapshot } from "./protocol";
import { MODELS } from "./models";

export function editorSettings(): EditorSettings {
  const { binary, minConfidence, extraArgs } = cliConfig();
  return {
    binaryPath: binary,
    minConfidence,
    extraArgs,
    publishDiagnostics: vscode.workspace
      .getConfiguration("aster")
      .get<boolean>("publishDiagnostics", false),
  };
}

/** Everything the settings page draws, read fresh. The editor settings need no
 *  CLI and the MCP list is not worth failing the page over, so the parts that
 *  can still answer do, and only the config read reports an error. */
export async function snapshot(root: string | null): Promise<SettingsSnapshot> {
  const binaryOk = await checkBinary(cliConfig().binary);
  const base: SettingsSnapshot = {
    keys: [],
    paths: null,
    editor: editorSettings(),
    servers: [],
    models: [],
    providers: [],
    workspaceRoot: root,
    binaryOk,
  };
  if (!binaryOk) {
    return base;
  }

  const cwd = root ?? process.cwd();
  const [keys, paths, servers, providers] = await Promise.all([
    config.list(cwd).catch((err: unknown) => err as Error),
    config.paths(cwd).catch(() => null),
    info.mcpServers(cwd).catch(() => []),
    info.providers(cwd).catch(() => []),
  ]);
  const configured = Array.isArray(keys)
    ? keys.find((key) => key.key === "review.model")?.value
    : null;
  const catalog = await info.modelsFor(cwd, String(configured ?? MODELS[0])).catch(() => []);
  return {
    ...base,
    keys: Array.isArray(keys) ? keys : [],
    paths,
    servers,
    providers,
    // The vetted list first so the useful ids are at the top of the menu, then
    // whatever else the endpoint offers.
    models: [...MODELS, ...catalog.filter((id) => !MODELS.includes(id))],
    ...(Array.isArray(keys) ? {} : { error: describe(keys) }),
  };
}

function describe(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
