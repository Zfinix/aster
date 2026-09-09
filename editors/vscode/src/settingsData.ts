import * as vscode from "vscode";
import * as config from "./asterConfig";
import * as info from "./info";
import { checkBinary, cliConfig } from "./asterCli";
import { EditorSettings, SettingsSnapshot } from "./protocol";

export function editorSettings(): EditorSettings {
  const { binary, minConfidence, extraArgs } = cliConfig();
  return {
    binaryPath: binary,
    minConfidence,
    extraArgs,
    publishDiagnostics: vscode.workspace
      .getConfiguration("aster")
      .get<boolean>("publishDiagnostics", false),
    sounds: vscode.workspace.getConfiguration("aster").get<boolean>("sounds", true),
    completionSound: vscode.workspace
      .getConfiguration("aster")
      .get<string>("completionSound", "sparkle"),
  };
}

/** Everything the settings page draws, read fresh. The editor settings need no
 *  CLI and the MCP list is not worth failing the page over, so the parts that
 *  can still answer do, and only the config read reports an error. */
export async function snapshot(root: string | null): Promise<SettingsSnapshot> {
  const binaryOk = await checkBinary(cliConfig().binary);
  const base: SettingsSnapshot = {
    keys: [],
    apiKeys: [],
    envVars: [],
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
  const [keys, apiKeys, paths, servers, providers, envVars] = await Promise.all([
    config.list(cwd).catch((err: unknown) => err as Error),
    info.apiKeys(cwd).catch(() => []),
    config.paths(cwd).catch(() => null),
    info.mcpServers(cwd).catch(() => []),
    info.providers(cwd).catch(() => []),
    info.envVars(cwd).catch(() => []),
  ]);
  const configured = Array.isArray(keys)
    ? keys.find((key) => key.key === "review.model")?.value
    : null;
  const model = configured ? String(configured) : await info.currentModel(cwd).catch(() => null);
  const catalog = model
    ? await info.modelsFor(cwd, model).catch(() => [])
    : await info.recommendedModels(cwd).catch(() => []);
  return {
    ...base,
    keys: Array.isArray(keys) ? keys : [],
    apiKeys,
    envVars,
    paths,
    servers,
    providers,
    // Only what this endpoint serves: another provider's ids are not choices.
    models: catalog,
    ...(Array.isArray(keys) ? {} : { error: describe(keys) }),
  };
}

function describe(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
