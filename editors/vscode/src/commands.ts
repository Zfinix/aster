import * as vscode from "vscode";
import { runCli } from "./asterCli";
import { SkillCommand } from "./protocol";

interface SkillEntry {
  name: string;
  description: string;
}

interface SkillsList {
  scopes?: { scope: string; skills: SkillEntry[] }[];
  plugins?: { plugin: string; skills: SkillEntry[] }[];
}

/** Every skill the session can see, project scope first so it shadows global,
 *  and plugin-contributed skills last under the plugin that brought them. */
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

function firstLine(description: string): string {
  const sentence = description.split(/(?<=\.)\s/)[0] ?? description;
  return sentence.length > 120 ? `${sentence.slice(0, 117)}…` : sentence;
}

export const IGNORED = "**/{node_modules,target,dist,.git}/**";

/** Workspace files and folders matching a fuzzy query, for @ mentions. Folders
 *  keep a trailing slash so the picker can tell the two apart; `findFiles` only
 *  yields files, so the folders are read back off the paths under them. */
export async function searchFiles(query: string): Promise<string[]> {
  const [files, nested] = await Promise.all([
    vscode.workspace.findFiles(query ? `**/*${query}*` : "**/*", IGNORED, 50),
    vscode.workspace.findFiles(query ? `**/*${query}*/**` : "*/**", IGNORED, 1000),
  ]);
  const paths = new Set(relative(files));
  for (const path of relative(nested)) {
    for (const folder of folders(path)) {
      if (matches(folder, query)) {
        paths.add(`${folder}/`);
      }
    }
  }
  return [...paths].sort(shallowestFirst).slice(0, 50);
}

function relative(uris: vscode.Uri[]): string[] {
  const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  return uris.map((uri) => {
    const path = root && uri.fsPath.startsWith(root) ? uri.fsPath.slice(root.length + 1) : uri.fsPath;
    return path.replace(/\\/g, "/");
  });
}

function folders(path: string): string[] {
  const parts = path.split("/").slice(0, -1);
  return parts.map((_, i) => parts.slice(0, i + 1).join("/"));
}

function matches(path: string, query: string): boolean {
  return path.slice(path.lastIndexOf("/") + 1).toLowerCase().includes(query.toLowerCase());
}

function shallowestFirst(a: string, b: string): number {
  const left = a.replace(/\/$/, "");
  const right = b.replace(/\/$/, "");
  const depth = left.split("/").length - right.split("/").length;
  return depth !== 0 ? depth : left.localeCompare(right);
}
