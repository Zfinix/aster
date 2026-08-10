import { exec } from "child_process";
import * as path from "path";
import * as vscode from "vscode";

export function workspaceRoot(): string | undefined {
  return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
}

export function repoName(root: string | undefined): string | null {
  return root ? path.basename(root) : null;
}

/** Current branch, or null outside a git repo / on a detached head. */
export function currentBranch(root: string): Promise<string | null> {
  return new Promise((resolve) => {
    exec("git rev-parse --abbrev-ref HEAD", { cwd: root }, (err, stdout) => {
      const branch = stdout.trim();
      resolve(err || !branch || branch === "HEAD" ? null : branch);
    });
  });
}
