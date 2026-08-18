import * as vscode from "vscode";
import { FindingDiagnostics } from "./diagnostics";
import { FindingsTreeProvider, openFinding } from "./findingsTree";
import { AsterPanel } from "./panel";
import { runCli } from "./asterCli";
import { registerOutputProvider } from "./outputProvider";
import { Finding } from "./types";

/**
 * Contributing a view container to the secondary sidebar (the right-hand pane,
 * where Codex and Claude Code live) needs VS Code 1.106. Older hosts fall back
 * to the activity bar container, gated on this context key.
 */
function supportsSecondarySidebar(): boolean {
  const [major = 0, minor = 0] = vscode.version.split(".").map(Number);
  return major > 1 || (major === 1 && minor >= 106);
}

export function activate(context: vscode.ExtensionContext): void {
  void vscode.commands.executeCommand(
    "setContext",
    "aster.noSecondarySidebar",
    !supportsSecondarySidebar()
  );

  const output = vscode.window.createOutputChannel("Aster");
  const diagnostics = new FindingDiagnostics();
  const tree = new FindingsTreeProvider();
  const panel = new AsterPanel(context, diagnostics, tree, output);

  registerOutputProvider(context);

  const statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 50);
  statusBar.command = "aster.reviewBranch";
  statusBar.text = "$(sparkle) Aster";
  statusBar.tooltip = "Review current branch with Aster";
  statusBar.show();

  context.subscriptions.push(
    output,
    diagnostics,
    statusBar,
    vscode.window.registerWebviewViewProvider(AsterPanel.viewType, panel, {
      webviewOptions: { retainContextWhenHidden: true },
    }),
    vscode.window.registerWebviewViewProvider(AsterPanel.primaryViewType, panel, {
      webviewOptions: { retainContextWhenHidden: true },
    }),
    vscode.window.registerTreeDataProvider("asterFindings", tree),
    vscode.commands.registerCommand("aster.openPanel", () =>
      panel.openInEditor(vscode.ViewColumn.Active)
    ),
    vscode.commands.registerCommand("aster.openInSidebar", () => panel.focus()),
    vscode.commands.registerCommand("aster.openInNewTab", () =>
      panel.openInEditor(vscode.ViewColumn.Beside)
    ),
    vscode.commands.registerCommand("aster.openInPrimaryEditor", () =>
      panel.openInEditor(vscode.ViewColumn.One)
    ),
    vscode.commands.registerCommand("aster.openInNewWindow", () => panel.openInNewWindow()),
    vscode.commands.registerCommand("aster.newConversation", () => panel.newConversation()),
    vscode.commands.registerCommand("aster.commandMenu", () => panel.showCommandMenu()),
    vscode.commands.registerCommand("aster.reopenSession", () => panel.reopenSession()),
    vscode.commands.registerCommand("aster.insertMention", () => panel.insertMention()),
    vscode.commands.registerCommand("aster.reviewBranch", () =>
      panel.startReview({ kind: "working" })
    ),
    vscode.commands.registerCommand("aster.reviewRange", async () => {
      const value = await vscode.window.showInputBox({
        prompt: "Git range to review",
        placeHolder: "main..HEAD",
      });
      if (value) {
        await panel.startReview({ kind: "range", value });
      }
    }),
    vscode.commands.registerCommand("aster.reviewPr", async () => {
      const value = await vscode.window.showInputBox({
        prompt: "GitHub PR number to review",
        placeHolder: "42",
        validateInput: (v) => (/^\d+$/.test(v) ? undefined : "Enter a PR number"),
      });
      if (value) {
        await panel.startReview({ kind: "pr", value });
      }
    }),
    vscode.commands.registerCommand("aster.cancelReview", () => panel.cancelReview()),
    vscode.commands.registerCommand("aster.clearFindings", () => {
      diagnostics.clear();
      tree.clear();
    }),
    vscode.commands.registerCommand("aster.openFinding", (finding: Finding) => {
      const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
      if (root) {
        void openFinding(finding, root);
      }
    }),
    vscode.commands.registerCommand("aster.fixFinding", async (finding: Finding) => {
      const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
      if (!root) return;
      const input = JSON.stringify([finding]);
      try {
        const { stdout, code } = await runCli(
          ["fix", "--findings-json", "-", "--apply", "--json"],
          root,
          input
        );
        if (code !== 0) {
          void vscode.window.showErrorMessage(`Aster fix failed with code ${code}`);
          return;
        }
        const results = JSON.parse(stdout) as { status: string; reason?: string }[];
        const result = results[0];
        if (result) {
          if (result.status === "applied") {
            void vscode.window.showInformationMessage(`Fix applied: ${finding.title}`);
          } else {
            void vscode.window.showWarningMessage(
              `Fix ${result.status}: ${result.reason ?? finding.title}`
            );
          }
        }
      } catch (err) {
        void vscode.window.showErrorMessage(
          `Aster fix error: ${err instanceof Error ? err.message : String(err)}`
        );
      }
    })
  );
}

export function deactivate(): void {}
