import * as path from "path";
import * as vscode from "vscode";
import { Finding } from "./types";

const SEVERITY_MAP: Record<string, vscode.DiagnosticSeverity> = {
  critical: vscode.DiagnosticSeverity.Error,
  high: vscode.DiagnosticSeverity.Error,
  medium: vscode.DiagnosticSeverity.Warning,
  low: vscode.DiagnosticSeverity.Information,
  info: vscode.DiagnosticSeverity.Hint,
};

function publishingEnabled(): boolean {
  return vscode.workspace.getConfiguration("aster").get<boolean>("publishDiagnostics", false);
}

export class FindingDiagnostics {
  private readonly collection: vscode.DiagnosticCollection;
  private readonly byFile = new Map<string, vscode.Diagnostic[]>();
  private readonly configWatcher: vscode.Disposable;

  constructor() {
    this.collection = vscode.languages.createDiagnosticCollection("aster");
    // Turning the setting off mid-session should empty the tab, not leave the
    // last review's findings stranded there.
    this.configWatcher = vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("aster.publishDiagnostics")) {
        this.republish();
      }
    });
  }

  add(finding: Finding, workspaceRoot: string): void {
    const filePath = path.isAbsolute(finding.file_path)
      ? finding.file_path
      : path.join(workspaceRoot, finding.file_path);

    const endLine = Math.max(finding.line - 1, 0);
    const startLine = Math.max((finding.start_line ?? finding.line) - 1, 0);
    const range = new vscode.Range(startLine, 0, endLine, Number.MAX_SAFE_INTEGER);

    const confidence =
      finding.confidence !== null && finding.confidence !== undefined
        ? ` (confidence ${Math.round(finding.confidence * 100)}%)`
        : "";
    const diagnostic = new vscode.Diagnostic(
      range,
      `${finding.title}${confidence}\n${finding.description}\nSuggestion: ${finding.suggestion}`,
      SEVERITY_MAP[finding.severity] ?? vscode.DiagnosticSeverity.Warning
    );
    diagnostic.source = "aster";
    diagnostic.code = finding.category;

    const existing = this.byFile.get(filePath) ?? [];
    existing.push(diagnostic);
    this.byFile.set(filePath, existing);
    if (publishingEnabled()) {
      this.collection.set(vscode.Uri.file(filePath), existing);
    }
  }

  private republish(): void {
    this.collection.clear();
    if (!publishingEnabled()) {
      return;
    }
    for (const [filePath, diagnostics] of this.byFile) {
      this.collection.set(vscode.Uri.file(filePath), diagnostics);
    }
  }

  clear(): void {
    this.byFile.clear();
    this.collection.clear();
  }

  dispose(): void {
    this.configWatcher.dispose();
    this.collection.dispose();
  }
}
