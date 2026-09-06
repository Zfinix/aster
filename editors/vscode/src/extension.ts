import * as vscode from "vscode";
import { FindingDiagnostics } from "./diagnostics";
import { FindingsTreeProvider, openFinding } from "./findingsTree";
import { AsterPanel } from "./panel";
import { SettingsPanel } from "./settingsPanel";
import { cliConfig, runCli, useBundledCli } from "./asterCli";
import { installShellCommand, onShellPath, refreshShellCommand } from "./shellCommand";
import { registerOutputProvider } from "./outputProvider";
import { Finding } from "./types";

function supportsSecondarySidebar(): boolean {
  const [major = 0, minor = 0] = vscode.version.split(".").map(Number);
  return major > 1 || (major === 1 && minor >= 106);
}

export function activate(context: vscode.ExtensionContext): void {
  const bundled = useBundledCli(context.extensionPath);
  if (bundled) {
    // The link an older version made points into its own folder, which this
    // update replaced; repair it before anything asks the user about it.
    void refreshShellCommand(bundled).then(() => offerShellCommand(context, bundled));
  }

  function report(result: { ok: boolean; message: string }): void {
    void (result.ok
      ? vscode.window.showInformationMessage(result.message)
      : vscode.window.showErrorMessage(`Could not add aster to your PATH: ${result.message}`));
  }

  /** Offered once, the first time a build carrying the CLI starts up without an
   *  aster the user's own terminal can find. */
  async function offerShellCommand(
    context: vscode.ExtensionContext,
    binary: string
  ): Promise<void> {
    const KEY = "aster.shellCommandOffered";
    if (context.globalState.get<boolean>(KEY)) return;
    if (await onShellPath()) return;
    await context.globalState.update(KEY, true);
    const yes = "Add to PATH";
    const answer = await vscode.window.showInformationMessage(
      "Aster ships its CLI with the extension. Add the aster command to your PATH?",
      yes,
      "Not now"
    );
    if (answer !== yes) return;
    report(await installShellCommand(binary));
  }

  void vscode.commands.executeCommand(
    "setContext",
    "aster.noSecondarySidebar",
    !supportsSecondarySidebar()
  );

  const output = vscode.window.createOutputChannel("Aster");
  const diagnostics = new FindingDiagnostics();
  const tree = new FindingsTreeProvider();
  const panel = new AsterPanel(context, diagnostics, tree, output);
  const settings = new SettingsPanel(context);

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
    vscode.commands.registerCommand("aster.openSettings", () => settings.open()),
    vscode.commands.registerCommand("aster.installShellCommand", async () => {
      const binary = useBundledCli(context.extensionPath) ?? cliConfig().binary;
      report(await installShellCommand(binary));
    }),
    // `aster.binaryPath` decides whether the panel believes the CLI exists, so
    // pointing it somewhere new has to re-run that check rather than wait for a
    // reload; the settings tab re-reads for the same reason.
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (!event.affectsConfiguration("aster")) return;
      panel.configChanged();
      settings.refresh();
    }),
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
