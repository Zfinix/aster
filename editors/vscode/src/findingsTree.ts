import * as path from "path";
import * as vscode from "vscode";
import { Finding } from "./types";

const SEVERITY_ORDER = ["critical", "high", "medium", "low", "info"];

const SEVERITY_ICON: Record<string, vscode.ThemeIcon> = {
  critical: new vscode.ThemeIcon("error", new vscode.ThemeColor("errorForeground")),
  high: new vscode.ThemeIcon("error", new vscode.ThemeColor("errorForeground")),
  medium: new vscode.ThemeIcon("warning", new vscode.ThemeColor("editorWarning.foreground")),
  low: new vscode.ThemeIcon("info"),
  info: new vscode.ThemeIcon("lightbulb"),
};

export class FindingItem extends vscode.TreeItem {
  constructor(public readonly finding: Finding) {
    super(finding.title, vscode.TreeItemCollapsibleState.None);
    this.description = `${finding.file_path}:${finding.line}`;
    this.iconPath = SEVERITY_ICON[finding.severity] ?? new vscode.ThemeIcon("warning");
    this.contextValue = "finding";
    this.tooltip = new vscode.MarkdownString(
      [
        `**${finding.title}**`,
        "",
        `\`${finding.severity}\` · \`${finding.category}\`` +
          (finding.confidence != null
            ? ` · ${Math.round(finding.confidence * 100)}% confidence`
            : ""),
        "",
        finding.description,
        "",
        `**Suggestion:** ${finding.suggestion}`,
      ].join("\n")
    );
    this.command = {
      command: "aster.openFinding",
      title: "Open Finding",
      arguments: [finding],
    };
  }
}

export class FindingsTreeProvider implements vscode.TreeDataProvider<FindingItem> {
  private readonly emitter = new vscode.EventEmitter<FindingItem | undefined>();
  readonly onDidChangeTreeData = this.emitter.event;

  private findings: Finding[] = [];

  add(finding: Finding): void {
    this.findings.push(finding);
    this.findings.sort(
      (a, b) =>
        SEVERITY_ORDER.indexOf(a.severity) - SEVERITY_ORDER.indexOf(b.severity) ||
        a.file_path.localeCompare(b.file_path) ||
        a.line - b.line
    );
    this.emitter.fire(undefined);
  }

  clear(): void {
    this.findings = [];
    this.emitter.fire(undefined);
  }

  get count(): number {
    return this.findings.length;
  }

  getTreeItem(element: FindingItem): vscode.TreeItem {
    return element;
  }

  getChildren(element?: FindingItem): FindingItem[] {
    if (element) {
      return [];
    }
    return this.findings.map((f) => new FindingItem(f));
  }
}

export async function openFinding(finding: Finding, workspaceRoot: string): Promise<void> {
  const filePath = path.isAbsolute(finding.file_path)
    ? finding.file_path
    : path.join(workspaceRoot, finding.file_path);
  const line = Math.max(finding.line - 1, 0);
  const document = await vscode.workspace.openTextDocument(filePath);
  const editor = await vscode.window.showTextDocument(document);
  const position = new vscode.Position(line, 0);
  editor.selection = new vscode.Selection(position, position);
  editor.revealRange(new vscode.Range(position, position), vscode.TextEditorRevealType.InCenter);
}
