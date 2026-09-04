import * as path from "node:path";
import * as vscode from "vscode";
import { checkBinary, cliConfig, LoginRun, ProviderOverride, runCli, runLogin } from "./asterCli";
import * as info from "./info";
import { ChatRunner } from "./chatRunner";
import { IGNORED, searchFiles, skillCommands } from "./commands";
import { MODELS, RECOMMENDED } from "./models";
import { deleteSession, listSessions, loadSession, renameSession } from "./sessions";
import { FindingDiagnostics } from "./diagnostics";
import { FindingsTreeProvider, openFinding } from "./findingsTree";
import { OutputContentProvider } from "./outputProvider";
import {
  ChatMessage,
  ChatStreamEvent,
  Effort,
  PastedFile,
  PermissionMode,
  ReviewSource,
  ToHost,
  ToWebview,
} from "./protocol";
import { currentBranch, repoName, workspaceRoot } from "./repo";
import { ReviewRunner } from "./reviewRunner";

const PERMISSION_KEY = "aster.permissionMode";
const MODEL_KEY = "aster.model";
const CUSTOM_MODELS_KEY = "aster.customModels";
const RECENT_MODELS_KEY = "aster.recentModels";
const EFFORT_KEY = "aster.effort";
const PROVIDER_KEY = "aster.provider";

export class AsterPanel implements vscode.WebviewViewProvider {
  static readonly viewType = "asterChat";
  static readonly primaryViewType = "asterChatPrimary";
  static readonly tabViewType = "asterChatTab";

  private focusTarget = `${AsterPanel.viewType}.focus`;
  private active: vscode.Webview | undefined;
  private readonly tabs = new Set<vscode.WebviewPanel>();
  private sidebar: vscode.WebviewView | undefined;
  private readonly surfaces = new Set<vscode.Webview>();
  private readonly chatRunners = new Map<vscode.Webview, ChatRunner>();
  private readonly reviewRunners = new Map<vscode.Webview, ReviewRunner>();
  private login: LoginRun | undefined;

  constructor(
    private readonly context: vscode.ExtensionContext,
    private readonly diagnostics: FindingDiagnostics,
    private readonly tree: FindingsTreeProvider,
    private readonly output: vscode.OutputChannel
  ) {}

  private getChatRunner(webview: vscode.Webview): ChatRunner {
    let runner = this.chatRunners.get(webview);
    if (!runner) {
      runner = new ChatRunner();
      this.chatRunners.set(webview, runner);
    }
    return runner;
  }

  private getReviewRunner(webview: vscode.Webview): ReviewRunner {
    let runner = this.reviewRunners.get(webview);
    if (!runner) {
      runner = new ReviewRunner();
      this.reviewRunners.set(webview, runner);
    }
    return runner;
  }


  resolveWebviewView(view: vscode.WebviewView): void {
    // Both view types are registered, but only the one whose container matches
    // the host's capability ever resolves.
    this.focusTarget = `${view.viewType}.focus`;
    this.sidebar = view;
    view.onDidDispose(() => {
      this.sidebar = undefined;
      this.detach(view.webview);
    });
    this.attach(view.webview);
  }

  private attach(webview: vscode.Webview): void {
    webview.options = {
      enableScripts: true,
      localResourceRoots: [vscode.Uri.joinPath(this.context.extensionUri, "media")],
    };
    webview.html = this.html(webview);
    this.surfaces.add(webview);
    webview.onDidReceiveMessage((message: ToHost) => {
      this.active = webview;
      void this.onMessage(message, webview);
    });
    this.active ??= webview;
  }

  private detach(webview: vscode.Webview): void {
    this.chatRunners.get(webview)?.cancel();
    this.reviewRunners.get(webview)?.cancel();
    this.chatRunners.delete(webview);
    this.reviewRunners.delete(webview);
    this.surfaces.delete(webview);
    if (this.active === webview) {
      this.active = this.surfaces.values().next().value;
    }
  }

  deserializeWebviewPanel(panel: vscode.WebviewPanel): void {
    panel.iconPath = vscode.Uri.joinPath(this.context.extensionUri, "media", "aster.svg");
    this.tabs.add(panel);
    panel.onDidDispose(() => {
      this.tabs.delete(panel);
      this.detach(panel.webview);
    });
    this.attach(panel.webview);
    this.active = panel.webview;
  }

  openInEditor(column: vscode.ViewColumn): void {
    const tab = vscode.window.createWebviewPanel(
      AsterPanel.tabViewType,
      "Aster",
      column,
      { enableScripts: true, retainContextWhenHidden: true }
    );
    tab.iconPath = vscode.Uri.joinPath(this.context.extensionUri, "media", "aster.svg");
    this.tabs.add(tab);
    tab.onDidDispose(() => {
      this.tabs.delete(tab);
      this.detach(tab.webview);
    });
    this.attach(tab.webview);
    // A command that opened a tab meant that tab: `attach` only fills `active`
    // when nothing holds it, which would leave replies going to the old surface.
    this.active = tab.webview;
  }

  async openInNewWindow(): Promise<void> {
    this.openInEditor(vscode.ViewColumn.Active);
    await vscode.commands.executeCommand("workbench.action.moveEditorToNewWindow");
  }

  reveal(): void {
    if (this.sidebar?.visible) {
      this.active = this.sidebar.webview;
      return;
    }
    const open = [...this.tabs].pop();
    if (open) {
      open.reveal(open.viewColumn ?? vscode.ViewColumn.Active);
      this.active = open.webview;
      return;
    }
    this.openInEditor(vscode.ViewColumn.Active);
  }

  async focus(): Promise<void> {
    await vscode.commands.executeCommand(this.focusTarget);
  }

  async startReview(source: ReviewSource): Promise<void> {
    this.reveal();
    // Whichever surface reveal() landed on owns this run.
    const origin = this.active;
    if (!origin) {
      return;
    }
    const id = `review-${Date.now()}`;
    this.postTo(origin, { type: "reviewStarted", id, source });
    await this.runReview(id, source, origin);
  }

  cancelReview(): void {
    for (const runner of this.reviewRunners.values()) {
      runner.cancel();
    }
    this.broadcastRunState();
  }

  private cancelRuns(webview: vscode.Webview | undefined): void {
    if (!webview) return;
    this.chatRunners.get(webview)?.cancel();
    this.reviewRunners.get(webview)?.cancel();
    this.broadcastRunState();
  }

  newConversation(): void {
    if (this.sidebar?.visible) {
      this.active = this.sidebar.webview;
      this.post({ type: "newConversation" });
      return;
    }
    this.openInEditor(vscode.ViewColumn.Active);
  }

  configChanged(): void {
    for (const surface of this.surfaces) {
      void this.sendInit(surface);
    }
  }

  showCommandMenu(): void {
    this.reveal();
    this.post({ type: "openCommandMenu" });
  }

  insertMention(): void {
    const editor = vscode.window.activeTextEditor;
    if (!editor) {
      return;
    }
    const root = workspaceRoot();
    const path =
      root && editor.document.uri.fsPath.startsWith(root)
        ? editor.document.uri.fsPath.slice(root.length + 1)
        : editor.document.uri.fsPath;

    const { start, end } = editor.selection;
    const text = editor.selection.isEmpty
      ? `@${path}`
      : `@${path}#L${start.line + 1}-${end.line + 1}`;

    this.reveal();
    this.post({ type: "insertMention", text });
  }

  private insertPaths(uris: string[]): void {
    const root = workspaceRoot();
    const storage = this.context.globalStorageUri.fsPath;
    const mentions = uris
      .map((raw) => {
        try {
          return vscode.Uri.parse(raw).fsPath;
        } catch {
          return "";
        }
      })
      .filter(Boolean)
      .map((fsPath) => {
        if (root && fsPath.startsWith(`${root}/`)) return fsPath.slice(root.length + 1);
        // Pasted files live in extension storage, outside the workspace, so
        // the CLI could never resolve a bare basename against the repo root.
        // The full path is required; the chip derives its label from it.
        if (storage && fsPath.startsWith(`${storage}/`)) return fsPath;
        return fsPath;
      })
      .map((p) => `@${p}`);

    if (mentions.length > 0) {
      this.post({ type: "insertMention", text: mentions.join(" ") });
    }
  }

  private async insertPasted(files: PastedFile[]): Promise<void> {
    const uris: string[] = [];
    for (const file of files) {
      const found = await this.findPasted(file);
      if (found) {
        uris.push(found.toString());
        continue;
      }
      if (file.data === undefined) {
        void vscode.window.showWarningMessage(
          `Aster: ${file.name} is too large to paste; mention it with @ instead.`
        );
        continue;
      }
      uris.push((await this.writePasted(file, file.data)).toString());
    }
    this.insertPaths(uris);
  }

  private async findPasted(file: PastedFile): Promise<vscode.Uri | undefined> {
    if (/[[\]{}*?]/.test(file.name)) {
      return undefined;
    }
    // Two candidates: one match is the file, more than one is a guess.
    const found = await vscode.workspace.findFiles(`**/${file.name}`, IGNORED, 2);
    if (found.length !== 1) {
      return undefined;
    }
    const stat = await vscode.workspace.fs.stat(found[0]);
    return stat.size === file.size ? found[0] : undefined;
  }

  private async writePasted(file: PastedFile, data: string): Promise<vscode.Uri> {
    const dir = vscode.Uri.joinPath(this.context.globalStorageUri, "pasted");
    await vscode.workspace.fs.createDirectory(dir);
    // Stamped, so a second screenshot never overwrites the one already mentioned.
    const target = vscode.Uri.joinPath(dir, `${Date.now().toString(36)}-${file.name}`);
    await vscode.workspace.fs.writeFile(target, Buffer.from(data, "base64"));
    return target;
  }

  async reopenSession(): Promise<void> {
    const root = workspaceRoot();
    if (!root) {
      return;
    }
    const sessions = await listSessions(root);
    if (sessions.length === 0) {
      void vscode.window.showInformationMessage("Aster: no saved sessions for this repo.");
      return;
    }
    const picked = await vscode.window.showQuickPick(
      sessions.map((s) => ({
        label: s.title,
        description: `${s.turns} turn${s.turns === 1 ? "" : "s"}`,
        detail: s.model ?? undefined,
        id: s.id,
      })),
      { placeHolder: "Reopen an Aster session" }
    );
    if (!picked) {
      return;
    }
    this.reveal();
    // Reopening a session abandons whatever turn is in flight on the active
    // surface; stop it so the loaded session starts clean.
    this.cancelRuns(this.active);
    this.post({ type: "sessionLoaded", id: picked.id, title: picked.label, turns: await loadSession(root, picked.id) });
    // The tab that owns this webview shows the session the user just reopened,
    // so it takes the saved name immediately rather than waiting for a turn.
    if (this.active) {
      this.nameTab(this.active, { type: "title", title: picked.label });
    }
  }

  private post(message: ToWebview): void {
    void this.active?.postMessage(message);
  }

  private postTo(target: vscode.Webview | undefined, message: ToWebview): void {
    void (target ?? this.active)?.postMessage(message);
  }

  private nameTab(origin: vscode.Webview, event: ChatStreamEvent): void {
    const title =
      event.type === "title"
        ? event.title
        : event.type === "done"
          ? event.title
          : undefined;
    if (!title) return;
    for (const tab of this.tabs) {
      if (tab.webview === origin) {
        tab.title = title;
        return;
      }
    }
  }

  private broadcastRunState(): void {
    for (const surface of this.surfaces) {
      const message: ToWebview = {
        type: "runState",
        review: this.reviewRunners.get(surface)?.running ?? false,
        chat: this.chatRunners.get(surface)?.running ?? false,
      };
      void surface.postMessage(message);
    }
  }

  private permissionMode(): PermissionMode {
    const stored = this.context.globalState.get<PermissionMode>(PERMISSION_KEY);
    if (!stored) return "edit";
    // Migrate the old alias names to the canonical CLI spellings.
    if (stored === ("ask" as PermissionMode)) return "manual";
    if (stored === ("deny" as PermissionMode)) return "plan";
    return stored;
  }

  private model(): string | null {
    return this.context.globalState.get<string>(MODEL_KEY) || null;
  }

  private customModels(): string[] {
    return this.context.globalState.get<string[]>(CUSTOM_MODELS_KEY) ?? [];
  }

  private recentModels(): string[] {
    return this.context.globalState.get<string[]>(RECENT_MODELS_KEY) ?? [];
  }

  private effort(): Effort | null {
    return this.context.globalState.get<Effort>(EFFORT_KEY) ?? null;
  }

  private env(): NodeJS.ProcessEnv {
    return process.env;
  }

  private async migrateProviderOverride(root: string | undefined): Promise<void> {
    const legacy = this.context.globalState.get<ProviderOverride>(PROVIDER_KEY);
    if (!legacy?.baseUrl) return;
    await this.context.globalState.update(PROVIDER_KEY, undefined);
    if (root) await info.useProvider(root, legacy.baseUrl).catch(() => {});
  }

  private async onMessage(message: ToHost, origin: vscode.Webview): Promise<void> {
    switch (message.type) {
      case "ready":
        await this.sendInit();
        this.broadcastRunState();
        break;
      case "chat":
        await this.runChat(message, origin);
        break;
      case "review":
        await this.runReview(message.id, message.source, origin);
        break;
      // Cancel both: whichever is idle is a no-op, and this removes any
      // dependence on the webview guessing which kind of run is in flight.
      case "cancelReview":
      case "cancelChat":
        this.reviewRunners.get(origin)?.cancel();
        this.chatRunners.get(origin)?.cancel();
        this.broadcastRunState();
        break;
      case "openFinding": {
        const root = workspaceRoot();
        if (root) {
          await openFinding(message.finding, root);
        }
        break;
      }
      case "openFile": {
        const root = workspaceRoot();
        if (root) {
          const uri = vscode.Uri.joinPath(vscode.Uri.file(root), message.path);
          await vscode.window.showTextDocument(await vscode.workspace.openTextDocument(uri));
        }
        break;
      }
      case "openExternal": {
        await vscode.env.openExternal(vscode.Uri.parse(message.url));
        break;
      }
      case "openUntitled": {
        const lang = languageId(message.lang);
        const uri = OutputContentProvider.provideUri(message.content, message.title);
        const doc = await vscode.workspace.openTextDocument(uri);
        if (lang) {
          await vscode.languages.setTextDocumentLanguage(doc, lang);
        }
        // A document flagged as such (a plan, an agent report) reads better
        // rendered than as source, so hand it to the markdown preview.
        if (message.doc && lang === "markdown") {
          await vscode.commands.executeCommand("markdown.showPreview", uri, true);
          break;
        }
        await vscode.window.showTextDocument(doc, { preview: true });
        break;
      }
      case "setPermissionMode":
        await this.context.globalState.update(PERMISSION_KEY, message.mode);
        break;
      case "setEffort":
        await this.context.globalState.update(EFFORT_KEY, message.effort ?? undefined);
        break;
      case "setModel": {
        await this.context.globalState.update(MODEL_KEY, message.model);
        const custom = this.customModels();
        if (message.model && !MODELS.includes(message.model) && !custom.includes(message.model)) {
          await this.context.globalState.update(CUSTOM_MODELS_KEY, [...custom, message.model]);
        }
        if (message.model) {
          const recent = this.recentModels().filter((m) => m !== message.model);
          await this.context.globalState.update(RECENT_MODELS_KEY, [message.model, ...recent].slice(0, 5));
          // Written through the CLI too, so the terminal and desktop agree.
          const root = workspaceRoot();
          if (root) void info.useModel(root, message.model).catch(() => {});
        }
        break;
      }
      // Answers belong to the turn that asked, which is the asking surface's own.
      case "approval":
        this.chatRunners.get(origin)?.approve(message.allow);
        break;
      case "answer":
        this.chatRunners.get(origin)?.answer(message.choice);
        break;
      case "inject":
        this.chatRunners.get(origin)?.inject(message.text);
        break;
      case "searchFiles":
        this.post({
          type: "fileResults",
          requestId: message.requestId,
          paths: await searchFiles(message.query),
        });
        break;
      case "readFile": {
        const root = workspaceRoot();
        const uri = root ? vscode.Uri.joinPath(vscode.Uri.file(root), message.path) : undefined;
        let file: { path: string; lang?: string; content: string; truncated: boolean } | null = null;
        if (uri) {
          try {
            const text = new TextDecoder().decode(await vscode.workspace.fs.readFile(uri));
            const lines = text.split("\n");
            const content = lines.slice(0, 200).join("\n").slice(0, 32000);
            file = {
              path: message.path,
              lang: message.path.split(".").pop(),
              content,
              truncated: content.length < text.length,
            };
          } catch {
            file = null;
          }
        }
        this.post({ type: "filePreview", requestId: message.requestId, file });
        break;
      }
      case "listSessions": {
        const root = workspaceRoot();
        this.post({ type: "sessions", sessions: root ? await listSessions(root) : [] });
        break;
      }
      case "info":
        await this.sendInfo(message.id, message.topic);
        break;
      case "attachFiles": {
        const root = workspaceRoot();
        const picked = await vscode.window.showOpenDialog({
          canSelectMany: true,
          openLabel: "Attach",
          defaultUri: root ? vscode.Uri.file(root) : undefined,
        });
        this.insertPaths((picked ?? []).map((uri) => uri.toString()));
        break;
      }
      case "dropFiles":
        this.insertPaths(message.uris);
        break;
      case "pasteFiles":
        await this.insertPasted(message.files);
        break;
      case "listMcp":
        await this.sendMcpServers();
        break;
      case "toggleMcp": {
        const root = workspaceRoot();
        if (!root) break;
        try {
          await info.toggleMcp(root, message.name, message.disabled);
        } catch (err) {
          void vscode.window.showErrorMessage(`Aster: ${describe(err)}`);
        }
        // Re-read rather than assume: the toggle writes a config file, and what
        // landed there is what the next turn will start.
        await this.sendMcpServers();
        break;
      }
      case "listProviders": {
        const root = workspaceRoot();
        this.post({
          type: "providers",
          providers: root ? await info.providers(root, this.env()).catch(() => []) : [],
        });
        break;
      }
      case "setProvider":
        await this.switchProvider(message.baseUrl, message.model);
        break;
      case "compact":
        await this.runCompact(message.id, message.messages);
        break;
      // Not every endpoint implements `/models`, so the failure is reported
      // rather than swallowed: the picker says so and offers to take an id.
      case "fetchModels": {
        const root = workspaceRoot();
        if (!root) {
          this.post({ type: "modelsLoaded", models: [], error: "Open a folder first." });
          break;
        }
        try {
          const { stdout, stderr, code } = await runCli(
            ["models", "--json"],
            root,
            undefined,
            this.env()
          );
          const parsed = JSON.parse(stdout) as string[] | { ok?: boolean; error?: string };
          if (code === 0 && Array.isArray(parsed)) {
            this.post({ type: "modelsLoaded", models: parsed });
          } else {
            const error = Array.isArray(parsed) ? stderr.trim() : parsed.error;
            this.post({
              type: "modelsLoaded",
              models: [],
              error: error || "This endpoint did not list its models.",
            });
          }
        } catch (err) {
          this.post({ type: "modelsLoaded", models: [], error: describe(err) });
        }
        break;
      }
      case "loadSession": {
        const root = workspaceRoot();
        if (!root) break;
        try {
          // Switching sessions abandons the turn in flight on the active
          // surface; stop it so the loaded session starts clean.
          this.cancelRuns(this.active);
          const title = (await listSessions(root)).find((s) => s.id === message.id)?.title ?? null;
          this.post({
            type: "sessionLoaded",
            id: message.id,
            title,
            turns: await loadSession(root, message.id),
          });
          // The tab that owns this webview shows the session the user just
          // loaded, so it takes the saved name immediately rather than waiting
          // for a turn.
          if (title) {
            this.nameTab(origin, { type: "title", title });
          }
        } catch (err) {
          void vscode.window.showErrorMessage(
            `Aster: ${err instanceof Error ? err.message : String(err)}`
          );
        }
        break;
      }
      // Both answer with the fresh list, so the panel never has to guess what
      // the store now holds.
      case "deleteSession":
      case "renameSession": {
        const root = workspaceRoot();
        if (!root) break;
        try {
          if (message.type === "deleteSession") {
            await deleteSession(root, message.id);
          } else {
            await renameSession(root, message.id, message.title);
            // The tab that owns this webview shows the session the user just
            // named, so it takes the new name immediately rather than waiting
            // for the next turn's title event.
            this.nameTab(origin, { type: "title", title: message.title });
          }
        } catch (err) {
          void vscode.window.showErrorMessage(`Aster: ${describe(err)}`);
        }
        this.post({ type: "sessions", sessions: await listSessions(root) });
        break;
      }
      case "fixFinding": {
        const root = workspaceRoot();
        if (!root) break;
        const result = await runFix(root, [message.finding]);
        if (result.length > 0) {
          this.post({ type: "fixResult", finding: message.finding, ...result[0] });
        }
        break;
      }
      case "fixAllFindings": {
        const root = workspaceRoot();
        if (!root) break;
        const results = await runFix(root, message.findings);
        this.post({
          type: "fixAllResult",
          results: results.map((r, i) => ({
            finding: message.findings[i],
            status: r.status,
            reason: r.reason,
          })),
        });
        break;
      }
      case "runCommand":
        if (message.command.startsWith("aster.")) {
          await vscode.commands.executeCommand(message.command);
        }
        break;
      case "login":
        await this.runLogin(message.target, origin);
        break;
    }
  }

  private async runLogin(target: string, origin: vscode.Webview): Promise<void> {
    const root = workspaceRoot();
    if (!root) {
      this.postTo(origin, { type: "loginDone", ok: false, message: "Open a folder first." });
      return;
    }
    this.login?.cancel();
    let last = "";
    let run: LoginRun;
    try {
      run = runLogin(target, root, this.env(), (line) => {
        last = line;
        this.output.appendLine(`[login] ${line}`);
        this.postTo(origin, { type: "loginOutput", line });
      });
    } catch (err) {
      this.postTo(origin, { type: "loginDone", ok: false, message: describe(err) });
      return;
    }
    this.login = run;
    try {
      const code = await run.done;
      if (this.login !== run) return;
      this.postTo(origin, {
        type: "loginDone",
        ok: code === 0,
        message: code === 0 ? "Signed in." : last || `aster login exited with code ${code}.`,
      });
    } catch (err) {
      this.postTo(origin, { type: "loginDone", ok: false, message: describe(err) });
    } finally {
      if (this.login === run) this.login = undefined;
    }
  }

  private async sendMcpServers(): Promise<void> {
    const root = workspaceRoot();
    const servers = root ? await info.mcpServers(root).catch(() => []) : [];
    this.post({ type: "mcpServers", servers });
  }

  private async sendInfo(id: string, topic: "status" | "memory" | "diff"): Promise<void> {
    const root = workspaceRoot();
    if (!root) {
      this.post({ type: "infoCard", id, title: topic, note: "Open a folder first.", error: true });
      return;
    }
    try {
      if (topic === "status") {
        this.post({ type: "infoCard", id, title: "Status", rows: await info.status(root, this.env()) });
        return;
      }
      if (topic === "memory") {
        const rows = await info.memoryBlocks(root);
        this.post({
          type: "infoCard",
          id,
          title: "Memory",
          rows,
          note: rows.length ? undefined : "Nothing remembered yet.",
        });
        return;
      }
      const diff = await info.workingDiff(root);
      this.post({
        type: "infoCard",
        id,
        title: "Uncommitted changes",
        body: diff.trim() || undefined,
        lang: "diff",
        note: diff.trim() ? undefined : "No uncommitted changes.",
      });
    } catch (err) {
      this.post({ type: "infoCard", id, title: topic, note: describe(err), error: true });
    }
  }

  private async switchProvider(baseUrl: string, model: string): Promise<void> {
    const root = workspaceRoot();
    if (!root) return;
    const catalog = await info.providers(root).catch(() => []);
    const chosen = catalog.find((p) => p.base_url === baseUrl);
    try {
      await info.useProvider(root, baseUrl, model);
    } catch (err) {
      void vscode.window.showErrorMessage(`Aster: ${describe(err)}`);
      return;
    }
    await this.context.globalState.update(MODEL_KEY, model);

    if (!(await info.hasKey(root, this.env()).catch(() => true))) {
      void vscode.window.showWarningMessage(
        `Aster: no key found for ${chosen?.name ?? baseUrl}. Add one in Aster Settings, under API keys.`
      );
    }
    this.post({
      type: "providerChanged",
      provider: chosen?.name ?? baseUrl,
      model,
      models: await info.modelsFor(root, model),
    });
  }

  private async runCompact(id: string, messages: ChatMessage[]): Promise<void> {
    const root = workspaceRoot();
    if (!root) return;
    try {
      const result = await info.compact(root, messages, this.model(), this.env());
      this.post({ type: "compacted", id, ...result });
    } catch (err) {
      this.post({ type: "chatError", id, message: describe(err) });
    }
  }

  private async sendInit(target?: vscode.Webview): Promise<void> {
    const root = workspaceRoot();
    await this.migrateProviderOverride(root);
    // The panel's own pick wins for this window; otherwise the config decides,
    // so a fresh install opens on what the terminal already uses.
    const model =
      this.model() ?? (root ? await info.currentModel(root).catch(() => null) : null);
    this.postTo(target, {
      type: "init",
      workspaceRoot: root ?? null,
      repoName: repoName(root),
      branch: root ? await currentBranch(root) : null,
      model,
      models: [...MODELS, ...this.customModels().filter((m) => !MODELS.includes(m))],
      recommended: [...RECOMMENDED],
      recent: this.recentModels(),
      contextBudget: root ? await info.contextBudget(root, this.env()) : 0,
      permissionMode: this.permissionMode(),
      effort: this.effort(),
      binaryOk: await checkBinary(cliConfig().binary),
      skills: await skillCommands(root),
      setup: root ? await info.setupNeeded(root, this.env()).catch(() => null) : null,
    });
  }

  private async runChat(message: Extract<ToHost, { type: "chat" }>, origin: vscode.Webview): Promise<void> {
    const root = workspaceRoot();
    if (!root) {
      this.post({ type: "chatError", id: message.id, message: "Open a folder to chat with Aster." });
      return;
    }
    const chat = this.getChatRunner(origin);
    if (chat.running) {
      this.post({ type: "chatError", id: message.id, message: "A turn is already running." });
      return;
    }

    let sawTerminal = false;
    try {
      const running = chat.run({
        messages: message.messages,
        cwd: root,
        model: message.model,
        permissionMode: message.permissionMode,
        effort: message.effort,
        env: this.env(),
        session: message.session,
        onEvent: (event) => {
          if (event.type === "done" || event.type === "error") {
            sawTerminal = true;
          }
          if (event.type === "done" && event.edits.length > 0) {
            this.output.appendLine(`[edits] ${event.edits.join(", ")}`);
          }
          this.nameTab(origin, event);
          origin.postMessage({ type: "chatEvent", id: message.id, event });
        },
        onStderr: (line) => this.output.appendLine(line),
      });
      this.broadcastRunState();
      const code = await running;
      if (!sawTerminal) {
        origin.postMessage({
          type: "chatError",
          id: message.id,
          message: `aster chat exited with code ${code}. See the Aster output channel.`,
        });
      }
    } catch (err) {
      origin.postMessage({
        type: "chatError",
        id: message.id,
        message: err instanceof Error ? err.message : String(err),
      });
    } finally {
      this.broadcastRunState();
    }
  }

  private async runReview(id: string, source: ReviewSource, origin: vscode.Webview): Promise<void> {
    const root = workspaceRoot();
    if (!root) {
      this.post({ type: "reviewError", id, message: "Open a folder to review." });
      return;
    }
    const runner = this.getReviewRunner(origin);
    if (runner.running) {
      this.post({ type: "reviewError", id, message: "A review is already running." });
      return;
    }

    this.diagnostics.clear();
    this.tree.clear();
    await vscode.commands.executeCommand("setContext", "aster.reviewRunning", true);

    try {
      const running = runner.run({
        cwd: root,
        source,
        env: this.env(),
        onEvent: (event) => {
          if (event.type === "finding") {
            this.diagnostics.add(event, root);
            this.tree.add(event);
          }
          origin.postMessage({ type: "reviewEvent", id, event });
        },
        onStderr: (line) => {
          this.output.appendLine(line);
          origin.postMessage({ type: "log", line });
        },
      });
      this.broadcastRunState();
      const code = await running;
      if (code !== 0) {
        origin.postMessage({
          type: "reviewError",
          id,
          message: `aster exited with code ${code}. See the Aster output channel.`,
        });
      } else {
        origin.postMessage({ type: "reviewDone", id });
      }
    } catch (err) {
      origin.postMessage({
        type: "reviewError",
        id,
        message: err instanceof Error ? err.message : String(err),
      });
    } finally {
      await vscode.commands.executeCommand("setContext", "aster.reviewRunning", false);
      this.broadcastRunState();
    }
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
    <link rel="stylesheet" href="${asset("webview", "index.css")}" />
    <title>Aster</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" nonce="${nonce}" src="${asset("webview", "index.js")}"></script>
  </body>
</html>`;
  }
}

function describe(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function languageId(lang: string | undefined): string | undefined {
  if (!lang) return undefined;
  const aliases: Record<string, string> = {
    ts: "typescript",
    tsx: "typescriptreact",
    js: "javascript",
    jsx: "javascriptreact",
    py: "python",
    rs: "rust",
    sh: "shellscript",
    bash: "shellscript",
    zsh: "shellscript",
    yml: "yaml",
    md: "markdown",
    text: "plaintext",
  };
  const key = lang.toLowerCase();
  return aliases[key] ?? key;
}

function nonceString(): string {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  return Array.from({ length: 32 }, () =>
    chars.charAt(Math.floor(Math.random() * chars.length))
  ).join("");
}

interface FixOutput {
  file_path: string;
  line: number;
  title: string;
  status: string;
  reason?: string;
  patch?: string;
}

async function runFix(
  cwd: string,
  findings: { file_path: string; line: number; severity: string; category: string; title: string; description: string; suggestion: string }[]
): Promise<FixOutput[]> {
  const input = JSON.stringify(findings);
  try {
    const { stdout, code } = await runCli(
      ["fix", "--findings-json", "-", "--apply", "--json"],
      cwd,
      input
    );
    if (code !== 0) {
      return findings.map((f) => ({
        file_path: f.file_path,
        line: f.line,
        title: f.title,
        status: "error",
        reason: `aster fix exited with code ${code}`,
      }));
    }
    return JSON.parse(stdout) as FixOutput[];
  } catch (err) {
    return findings.map((f) => ({
      file_path: f.file_path,
      line: f.line,
      title: f.title,
      status: "error",
      reason: err instanceof Error ? err.message : String(err),
    }));
  }
}
