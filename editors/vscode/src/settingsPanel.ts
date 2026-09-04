import * as vscode from "vscode";
import * as config from "./asterConfig";
import * as info from "./info";
import { snapshot } from "./settingsData";
import { ConfigScope, SettingsToHost } from "./protocol";

/** Every aster.yaml key, the extension's own settings, and the MCP servers, in
 *  one editor tab. One tab is kept at a time: a second `aster.openSettings`
 *  reveals the open one rather than stacking another copy of the same state. */
export class SettingsPanel {
  private static readonly viewType = "aster.settings";
  private tab: vscode.WebviewPanel | undefined;

  constructor(private readonly context: vscode.ExtensionContext) {}

  open(): void {
    if (this.tab) {
      this.tab.reveal();
      return;
    }
    const tab = vscode.window.createWebviewPanel(
      SettingsPanel.viewType,
      "Aster Settings",
      vscode.ViewColumn.Active,
      { enableScripts: true, retainContextWhenHidden: true }
    );
    tab.iconPath = vscode.Uri.joinPath(this.context.extensionUri, "media", "aster.svg");
    tab.webview.html = this.html(tab.webview);
    tab.webview.onDidReceiveMessage((message: SettingsToHost) => void this.handle(message));
    tab.onDidDispose(() => (this.tab = undefined));
    this.tab = tab;
  }

  refresh(): void {
    if (this.tab) void this.send();
  }

  private post(message: { type: string; [key: string]: unknown }): void {
    void this.tab?.webview.postMessage(message);
  }

  private async handle(message: SettingsToHost): Promise<void> {
    switch (message.type) {
      case "ready":
      case "reload":
        await this.send();
        break;

      case "setKey":
        await this.write(message.key, () =>
          config.set(this.root(), message.key, message.value, message.scope)
        );
        break;

      case "unsetKey":
        await this.write(message.key, () =>
          config.unset(this.root(), message.key, message.scope)
        );
        break;

      case "setApiKey":
        await this.write(message.var, () =>
          info.setApiKey(this.root(), message.var, message.value, message.scope)
        );
        break;

      case "unsetApiKey":
        await this.write(message.var, () =>
          info.unsetApiKey(this.root(), message.var, message.scope)
        );
        break;

      // Reveals never touch the snapshot: the value goes straight to the row
      // that asked and lives only as long as it is shown.
      case "revealApiKey":
        try {
          const value = await info.revealApiKey(this.root(), message.var);
          this.post({ type: "apiKeyValue", var: message.var, value });
        } catch (err) {
          this.post({ type: "settingsError", key: message.var, message: describe(err) });
        }
        break;

      case "setEditor":
        await this.write(message.key, async () => {
          await vscode.workspace
            .getConfiguration("aster")
            .update(message.key, message.value, vscode.ConfigurationTarget.Global);
        });
        break;

      case "toggleMcp":
        await this.write(message.name, () =>
          info.toggleMcp(this.root(), message.name, message.disabled)
        );
        break;

      case "openConfigFile":
        await this.openFile(message.scope);
        break;
    }
  }

  private async write(key: string, apply: () => Promise<void>): Promise<void> {
    try {
      await apply();
      await this.send();
    } catch (err) {
      this.post({ type: "settingsError", key, message: describe(err) });
    }
  }

  private async openFile(scope: ConfigScope): Promise<void> {
    try {
      const paths = await config.paths(this.root());
      const target =
        scope === "global" ? paths.global : (paths.project ?? paths.project_default);
      const uri = vscode.Uri.file(target);
      // `aster config set` creates the file on first write; opening one that is
      // not there yet should not fail, so an untitled buffer stands in.
      const exists = await vscode.workspace.fs.stat(uri).then(
        () => true,
        () => false
      );
      await vscode.window.showTextDocument(
        exists ? uri : uri.with({ scheme: "untitled" })
      );
    } catch (err) {
      this.post({ type: "settingsError", message: describe(err) });
    }
  }

  private root(): string {
    return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? process.cwd();
  }

  private async send(): Promise<void> {
    const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? null;
    this.post({ type: "settings", snapshot: await snapshot(root) });
  }

  private html(webview: vscode.Webview): string {
    const asset = (...parts: string[]) =>
      webview.asWebviewUri(vscode.Uri.joinPath(this.context.extensionUri, "media", ...parts));
    const nonce = nonceString();

    return `<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${webview.cspSource} data:; style-src ${webview.cspSource} 'unsafe-inline'; script-src 'nonce-${nonce}';" />
    <link rel="stylesheet" href="${asset("webview", "settings.css")}" />
    <title>Aster Settings</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" nonce="${nonce}" src="${asset("webview", "settings.js")}"></script>
  </body>
</html>`;
  }
}

function describe(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function nonceString(): string {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  return Array.from({ length: 32 }, () => chars[Math.floor(Math.random() * chars.length)]).join(
    ""
  );
}
