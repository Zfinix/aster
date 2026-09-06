import * as os from "node:os";
import * as path from "node:path";
import * as vscode from "vscode";
import { checkBinary, cliConfig, INSTALL_CMD, installCli, LoginRun, ProviderOverride, runCli, runLogin } from "./asterCli";
import * as info from "./info";
import { persist, probe } from "./connect";
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
  ConnectAuth,
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

const IMAGE_MIME: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  svg: "image/svg+xml",
  bmp: "image/bmp",
  ico: "image/x-icon",
};

const DOC_MIME: Record<string, string> = {
  pdf: "application/pdf",
  doc: "application/msword",
  docx: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  xls: "application/vnd.ms-excel",
  xlsx: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
  ppt: "application/vnd.ms-powerpoint",
  pptx: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
  odt: "application/vnd.oasis.opendocument.text",
  ods: "application/vnd.oasis.opendocument.spreadsheet",
  odp: "application/vnd.oasis.opendocument.presentation",
  rtf: "application/rtf",
  epub: "application/epub+zip",
  mobi: "application/x-mobipocket-ebook",
  azw: "application/vnd.amazon.ebook",
  csv: "text/csv",
  txt: "text/plain",
  md: "text/markdown",
  tex: "application/x-tex",
  xml: "application/xml",
  json: "application/json",
  html: "text/html",
  yaml: "text/yaml",
  yml: "text/yaml",
  tsv: "text/tab-separated-values",
  pages: "application/vnd.apple.pages",
  numbers: "application/vnd.apple.numbers",
  key: "application/vnd.apple.keynote",
  ps: "application/postscript",
  msg: "application/vnd.ms-outlook",
  eml: "message/rfc822",
  dot: "application/msword",
  dotx: "application/vnd.openxmlformats-officedocument.wordprocessingml.template",
  xlt: "application/vnd.ms-excel",
  xltx: "application/vnd.openxmlformats-officedocument.spreadsheetml.template",
  pot: "application/vnd.ms-powerpoint",
  potx: "application/vnd.openxmlformats-officedocument.presentationml.template",
};

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
    this.attach(view.webview, "sidebar");
  }

  private attach(webview: vscode.Webview, surface: Surface): void {
    webview.options = {
      enableScripts: true,
      localResourceRoots: [vscode.Uri.joinPath(this.context.extensionUri, "media")],
    };
    webview.html = this.html(webview, surface);
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
    this.attach(panel.webview, "tab");
    this.active = panel.webview;
  }

  openInEditor(column: vscode.ViewColumn): void {
    const tab = vscode.window.createWebviewPanel(
      AsterPanel.tabViewType,
      "Aster",
      column,
      { enableScripts: true, retainContextWhenHidden: true, enableFindWidget: true }
    );
    tab.iconPath = vscode.Uri.joinPath(this.context.extensionUri, "media", "aster.svg");
    this.tabs.add(tab);
    tab.onDidDispose(() => {
      this.tabs.delete(tab);
      this.detach(tab.webview);
    });
    this.attach(tab.webview, "tab");
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
        // Staged pastes live in the OS temp dir, outside the workspace, so the
        // CLI could never resolve a bare basename against the repo root. The
        // full path is required; the chip derives its label from it.
        return fsPath;
      })
      .map((p) => `@${p}`);

    if (mentions.length > 0) {
      this.post({ type: "insertMention", text: mentions.join(" "), mentions });
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
          `Aster: ${file.name} is too large to attach (over 64MB); mention it with @ instead.`
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
    // The OS temp dir, so a staged paste is cleaned up by the OS instead of
    // accumulating in extension storage forever.
    const dir = vscode.Uri.file(path.join(os.tmpdir(), "aster-pasted"));
    await vscode.workspace.fs.createDirectory(dir);
    // The original name, so the agent sees what was dropped. A collision is the
    // only case that gets a suffix, so a second paste of the same name never
    // overwrites the one already mentioned.
    let target = vscode.Uri.joinPath(dir, file.name);
    let n = 1;
    while (await fileExists(target)) {
      target = vscode.Uri.joinPath(dir, numberedName(file.name, n));
      n += 1;
    }
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
        const uri = resolveWorkspacePath(message.path);
        if (!uri) break;
        // Without a spot to land on, vscode.open picks the right viewer for
        // anything, images included, where a text document would refuse.
        if (!message.needle && !message.line) {
          await vscode.commands.executeCommand("vscode.open", uri);
          break;
        }
        const doc = await vscode.workspace.openTextDocument(uri);
        const editor = await vscode.window.showTextDocument(doc);
        // A needle (the text an edit matched) or a line lands the reader on
        // the spot, the way a review finding does.
        const at = message.needle ? doc.getText().indexOf(message.needle) : -1;
        const position =
          at >= 0
            ? doc.positionAt(at)
            : new vscode.Position(Math.min(Math.max((message.line ?? 1) - 1, 0), doc.lineCount - 1), 0);
        editor.selection = new vscode.Selection(position, position);
        editor.revealRange(
          new vscode.Range(position, position),
          vscode.TextEditorRevealType.InCenter,
        );
        break;
      }
      case "openExternal": {
        // A file link, or one with no scheme at all, is a path the reply
        // cited: the OS has no application for it, the editor does.
        const file = fileLink(message.url);
        if (file) {
          await this.onMessage({ type: "openFile", ...file }, origin);
          break;
        }
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
      case "dismissAnnouncements":
        // --dismiss accepts comma-separated ids
        try {
          await runCli(["announce", "--dismiss", message.ids.join(",")], workspaceRoot() ?? ".");
        } catch {
          // Silent — user already dismissed locally.
        }
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
        const uri = resolveWorkspacePath(message.path);
        let file: { path: string; lang?: string; content: string; truncated: boolean; image?: string; doc?: string; size?: number } | null = null;
        if (uri) {
          try {
            const ext = message.path.split(".").pop()?.toLowerCase();
            const mime = IMAGE_MIME[ext ?? ""];
            const docMime = DOC_MIME[ext ?? ""];
            const bytes = await vscode.workspace.fs.readFile(uri);
            if (mime && bytes.byteLength <= 8 * 1024 * 1024) {
              file = {
                path: message.path,
                content: "",
                truncated: false,
                image: `data:${mime};base64,${Buffer.from(bytes).toString("base64")}`,
              };
            } else if (docMime && bytes.byteLength <= 8 * 1024 * 1024) {
              file = {
                path: message.path,
                content: "",
                truncated: false,
                doc: `data:${docMime};base64,${Buffer.from(bytes).toString("base64")}`,
                size: bytes.byteLength,
              };
            } else {
              const text = new TextDecoder().decode(bytes);
              const lines = text.split("\n");
              const content = lines.slice(0, 200).join("\n").slice(0, 32000);
              file = {
                path: message.path,
                lang: message.path.split(".").pop(),
                content,
                truncated: content.length < text.length,
              };
            }
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
        // The catalog ships in the binary, so it reads fine before a folder is open.
        const root = workspaceRoot() ?? os.homedir();
        this.post({
          type: "providers",
          providers: await info.providers(root, this.env()).catch(() => []),
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
      case "connect":
        await this.connect(message.baseUrl, message.model, message.auth, origin);
        break;
      case "installCli": {
        const storage = this.context.globalStorageUri.fsPath;
        this.postTo(origin, { type: "installCliProgress", message: "Starting install..." });
        try {
          const binary = await installCli(storage, (msg) =>
            this.postTo(origin, { type: "installCliProgress", message: msg })
          );
          // Re-check and re-init so the panel transitions to the greeting state
          this.postTo(origin, { type: "installCliDone", ok: true, message: binary });
          await this.sendInit(origin);
        } catch (err) {
          this.postTo(origin, {
            type: "installCliDone",
            ok: false,
            message: err instanceof Error ? err.message : String(err),
          });
        }
        break;
      }
      case "installCliTerminal":
        await this.installInTerminal(origin);
        break;
      case "locateCli":
        await this.locateCli(origin);
        break;
    }
  }

  /** The install script in a real terminal. The editor's own network settings
   *  are not the shell's: launched from the Dock it has no proxy variables and
   *  no shell profile, which is what makes the built-in download fail where a
   *  terminal succeeds. The user also sees the script's own output. */
  private async installInTerminal(origin: vscode.Webview): Promise<void> {
    const terminal = vscode.window.createTerminal({ name: "Install Aster" });
    terminal.show();
    terminal.sendText(INSTALL_CMD, true);
    this.postTo(origin, {
      type: "installCliProgress",
      message: "Running the installer in a terminal. Watch it there.",
    });
    // The script leaves the terminal open, so the binary appearing is the only
    // reliable signal that it finished.
    const deadline = Date.now() + 5 * 60_000;
    while (Date.now() < deadline) {
      await new Promise((done) => setTimeout(done, 2000));
      const found = await this.findInstalledCli();
      if (!found) continue;
      await vscode.workspace
        .getConfiguration("aster")
        .update("binaryPath", found, vscode.ConfigurationTarget.Global);
      this.postTo(origin, { type: "installCliDone", ok: true, message: found });
      await this.sendInit(origin);
      return;
    }
    this.postTo(origin, {
      type: "installCliDone",
      ok: false,
      message: "the terminal has not produced an aster binary yet.",
    });
  }

  /** Where the install script puts the binary, plus whatever is already on the
   *  PATH the editor was started with. */
  private async findInstalledCli(): Promise<string | undefined> {
    const candidates = [
      cliConfig().binary,
      path.join(os.homedir(), ".local", "bin", "aster"),
      path.join(os.homedir(), ".cargo", "bin", "aster"),
      "/usr/local/bin/aster",
      "/opt/homebrew/bin/aster",
    ];
    for (const candidate of candidates) {
      if (candidate && (await checkBinary(candidate))) return candidate;
    }
    return undefined;
  }

  /** Already installed somewhere the editor cannot see: point at it directly
   *  rather than sending the user to a settings field to type a path. */
  private async locateCli(origin: vscode.Webview): Promise<void> {
    const picked = await vscode.window.showOpenDialog({
      canSelectFiles: true,
      canSelectFolders: false,
      canSelectMany: false,
      openLabel: "Use this binary",
      title: "Locate the aster binary",
      defaultUri: vscode.Uri.file(path.join(os.homedir(), ".local", "bin")),
    });
    const chosen = picked?.[0]?.fsPath;
    if (!chosen) return;
    if (!(await checkBinary(chosen))) {
      this.postTo(origin, {
        type: "installCliDone",
        ok: false,
        message: `${chosen} did not run. Pick the aster binary itself.`,
      });
      return;
    }
    await vscode.workspace
      .getConfiguration("aster")
      .update("binaryPath", chosen, vscode.ConfigurationTarget.Global);
    this.postTo(origin, { type: "installCliDone", ok: true, message: chosen });
    await this.sendInit(origin);
  }

  /** Onboarding from the panel: the endpoint is checked with the key it is
   *  about to be given before anything is written, then made current. A fresh
   *  init follows, which is what turns the card into the greeting. */
  private async connect(
    baseUrl: string,
    model: string,
    auth: ConnectAuth,
    origin: vscode.Webview
  ): Promise<void> {
    const cwd = workspaceRoot() ?? os.homedir();
    const env = this.env();
    const failed = (message: string) =>
      this.postTo(origin, { type: "connectDone", ok: false, message });
    const catalog = await info.providers(cwd, env).catch(() => []);
    const provider = catalog.find((p) => p.base_url === baseUrl);
    if (!provider) {
      failed("That provider is not in the catalog.");
      return;
    }
    const chosen = model || provider.example_model;
    try {
      if (auth.kind === "login") {
        await info.useProvider(cwd, provider.base_url, chosen);
        await this.context.globalState.update(MODEL_KEY, chosen);
        await this.runLogin(auth.target, origin);
        return;
      }
      const key = auth.kind === "key" ? auth.value.trim() : null;
      const outcome = await probe(cwd, provider, key, env);
      if (!outcome.ok) {
        failed(outcome.message);
        return;
      }
      await persist(cwd, provider, chosen, key, env);
      await this.context.globalState.update(MODEL_KEY, chosen);
      await this.sendInit(origin);
      this.postTo(origin, { type: "connectDone", ok: true, message: "Connected." });
    } catch (err) {
      failed(describe(err));
    }
  }

  private async runLogin(target: string, origin: vscode.Webview): Promise<void> {
    const root = workspaceRoot() ?? os.homedir();
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
      // The fresh init goes first, so the panel already carries the new
      // provider and model by the time it acts on the result.
      if (code === 0) await this.sendInit(origin);
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

  private async sendInfo(id: string, topic: "status" | "memory" | "diff" | "mom"): Promise<void> {
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
      if (topic === "mom") {
        this.post({ type: "infoCard", id, title: "Model policy", rows: await info.momPolicy(root) });
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
    const announcements = await this.fetchAnnouncements(root);
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
      setup: await info.setupNeeded(root ?? os.homedir(), this.env()).catch(() => null),
      announcements: announcements.length > 0 ? announcements : undefined,
    });
  }

  private async fetchAnnouncements(root: string | null | undefined): Promise<{ id: string; text: string }[]> {
    try {
      const { stdout, code } = await runCli(["announce", "--json"], root ?? ".");
      if (code !== 0) return [];
      const parsed = JSON.parse(stdout);
      if (parsed?.items && Array.isArray(parsed.items)) {
        return parsed.items.filter((i: unknown) => i && typeof i === "object" && typeof (i as { id: string }).id === "string");
      }
    } catch {
      // Silent — announcements are never worth an error.
    }
    return [];
  }

  private async runChat(message: Extract<ToHost, { type: "chat" }>, origin: vscode.Webview): Promise<void> {
    const root = workspaceRoot();
    if (!root) {
      this.post({ type: "chatError", id: message.id, message: "Open a folder to chat with Aster." });
      return;
    }
    const chat = this.getChatRunner(origin);
    // A queued turn flushes the instant the webview sees `done`, which beats
    // the CLI child exiting. Give the slot a moment to free before rejecting.
    const slotFreeBy = Date.now() + 3000;
    while (chat.running && Date.now() < slotFreeBy) {
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
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


  private html(webview: vscode.Webview, surface: Surface): string {
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
    <div id="root" data-surface="${surface}"></div>
    <script type="module" nonce="${nonce}" src="${asset("webview", "index.js")}"></script>
  </body>
</html>`;
  }
}

/** A tab gets the editor's own find widget; the sidebar cannot, so the webview
 *  brings its own there. */
type Surface = "sidebar" | "tab";

const SCHEME = /^[a-z][a-z0-9+.-]*:(?!\d)/i;
const LINE = /(?:#L|:)(\d+)(?:[-:,]L?\d+)*$/;

function fileLink(url: string): { path: string; line?: number } | undefined {
  let target: string;
  if (url.startsWith("file://")) {
    target = decodeURIComponent(url.slice("file://".length).replace(/^\/\/[^/]*/, ""));
  } else if (!SCHEME.test(url) && !url.startsWith("#")) {
    target = decodeURIComponent(url);
  } else {
    return undefined;
  }
  const line = LINE.exec(target);
  const path = line ? target.slice(0, line.index) : target;
  return path ? { path, line: line ? Number(line[1]) : undefined } : undefined;
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

async function fileExists(uri: vscode.Uri): Promise<boolean> {
  try {
    await vscode.workspace.fs.stat(uri);
    return true;
  } catch {
    return false;
  }
}

function numberedName(name: string, n: number): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? `${name.slice(0, dot)}-${n}${name.slice(dot)}` : `${name}-${n}`;
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

/** A pasted file lives outside the workspace, so an absolute or `~` path is
 *  taken as itself; only relative ones resolve against the root. */
function resolveWorkspacePath(raw: string): vscode.Uri | undefined {
  const expanded = raw === "~" || raw.startsWith("~/") ? path.join(os.homedir(), raw.slice(1)) : raw;
  if (path.isAbsolute(expanded)) return vscode.Uri.file(expanded);
  const root = workspaceRoot();
  return root ? vscode.Uri.joinPath(vscode.Uri.file(root), expanded) : undefined;
}
