import * as vscode from "vscode";
import { runCli } from "./asterCli";
import { SkillCommand } from "./protocol";

interface SkillEntry {
  name: string;
  description: string;
}

/** `aster skills list --json`: skills grouped by scope, project before global,
 *  plus the ones installed plugins contribute under their own name. */
interface SkillsList {
  scopes?: { scope: string; skills: SkillEntry[] }[];
  plugins?: { plugin: string; skills: SkillEntry[] }[];
}

/**
 * Every skill the session can see, project scope first so it shadows global,
 * and plugin-contributed skills last under the plugin that brought them.
 */
export async function skillCommands(cwd: string | undefined): Promise<SkillCommand[]> {
  if (!cwd) {
    return [];
  }
  try {
    const { stdout, code } = await runCli(["skills", "list", "--json"], cwd);
    if (code !== 0) {
      return [];
    }
    const parsed = JSON.parse(stdout) as SkillsList;
    const scoped = (parsed.scopes ?? []).flatMap((s) =>
      (s.skills ?? []).map((skill) => command(skill))
    );
    const fromPlugins = (parsed.plugins ?? []).flatMap((p) =>
      (p.skills ?? []).map((skill) => command(skill, p.plugin))
    );
    const seen = new Set<string>();
    return [...scoped, ...fromPlugins].filter((s) => !seen.has(s.name) && seen.add(s.name));
  } catch {
    return [];
  }
}

function command(skill: SkillEntry, plugin?: string): SkillCommand {
  return { name: skill.name, detail: firstLine(skill.description), plugin };
}

/** Skill descriptions run to a paragraph of trigger phrases; a menu row has
 *  room for a sentence. */
function firstLine(description: string): string {
  const sentence = description.split(/(?<=\.)\s/)[0] ?? description;
  return sentence.length > 120 ? `${sentence.slice(0, 117)}…` : sentence;
}

/** Workspace files matching a fuzzy query, for @ mentions. */
export async function searchFiles(query: string): Promise<string[]> {
  const pattern = query ? `**/*${query}*` : "**/*";
  const uris = await vscode.workspace.findFiles(
    pattern,
    "**/{node_modules,target,dist,.git}/**",
    50
  );
  const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  return uris
    .map((uri) => (root && uri.fsPath.startsWith(root) ? uri.fsPath.slice(root.length + 1) : uri.fsPath))
    .sort((a, b) => a.length - b.length);
}
