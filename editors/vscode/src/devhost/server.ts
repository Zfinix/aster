import { exec } from "node:child_process";
import * as fs from "node:fs";
import * as http from "node:http";
import * as path from "node:path";
import { checkBinary, cliConfig, cliEnv, ProviderOverride, runCli } from "../asterCli";
import { ChatRunner } from "../chatRunner";
import { skillCommands } from "../commands";
import * as info from "../info";
import { MODELS, RECOMMENDED } from "../models";
import {
  ChatMessage,
  Effort,
  PermissionMode,
  SettingsToHost,
  SettingsToWebview,
  ToHost,
  ToWebview,
} from "../protocol";
import { currentBranch, repoName } from "../repo";
import { ReviewRunner } from "../reviewRunner";
import { deleteSession, listSessions, loadSession, renameSession } from "../sessions";
import * as settingsData from "../settingsData";
import * as asterConfig from "../asterConfig";
import { shell } from "./shell";

interface State {
  model: string | null;
  permissionMode: PermissionMode;
  effort: Effort | null;
  customModels: string[];
  recentModels: string[];
  provider?: ProviderOverride;
}

export function start(root: string, port: number): void {
  const state: State = {
    model: process.env.ASTER_MODEL || null,
    permissionMode: "manual",
    effort: null,
    customModels: [],
    recentModels: [],
  };
  const chat = new ChatRunner();
  const review = new ReviewRunner();
  const clients = new Set<http.ServerResponse>();

  const post = (message: ToWebview | SettingsToWebview) => {
    const frame = `data: ${JSON.stringify(message)}\n\n`;
    for (const client of clients) {
      try {
        if (client.writableEnded || client.destroyed) {
          throw new Error("gone");
        }
        client.write(frame);
      } catch {
        clients.delete(client);
      }
    }
  };
  const env = () => cliEnv(state.provider);
  const runState = () => post({ type: "runState", review: review.running, chat: chat.running });
  const noEditor = (what: string) => post({ type: "log", line: `[devhost] ${what} needs the editor` });

  const server = http.createServer(async (req, res) => {
    const url = (req.url ?? "/").split("?")[0];

    if (url === "/events") {
      res.writeHead(200, {
        "content-type": "text/event-stream",
        "cache-control": "no-cache",
        connection: "keep-alive",
      });
      res.write("\n");
      // One runner means one panel. A second tab would share this host's chat
      // and be told a turn is already running whenever either of them sent, so
      // the newest panel takes over and the older ones are retired.
      for (const stale of clients) {
        stale.write(`data: ${JSON.stringify({ type: "displaced" })}\n\n`);
        stale.end();
      }
      clients.clear();
      clients.add(res);
      res.on("error", () => clients.delete(res));
      req.on("close", () => clients.delete(res));
      return;
    }

    if (url === "/settings-message" && req.method === "POST") {
      const body = await read(req);
      res.writeHead(204).end();
      try {
        await onSettings(JSON.parse(body) as SettingsToHost);
      } catch (err) {
        console.error("[devhost]", err);
      }
      return;
    }

    if (url === "/settings" ) {
      res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      res.end(shell(repoName(root) ?? "aster", "settings"));
      return;
    }

    if (url === "/message" && req.method === "POST") {
      const body = await read(req);
      res.writeHead(204).end();
      try {
        await onMessage(JSON.parse(body) as ToHost);
      } catch (err) {
        console.error("[devhost]", err);
      }
      return;
    }

    if (url.startsWith("/webview/")) {
      const file = path.join(__dirname, "..", "..", "media", "webview", path.basename(url));
      if (!fs.existsSync(file)) {
        res.writeHead(404).end("run `bun run build:webview` first");
        return;
      }
      res.writeHead(200, {
        "content-type": file.endsWith(".css") ? "text/css" : "text/javascript",
        "cache-control": "no-store",
      });
      fs.createReadStream(file).pipe(res);
      return;
    }

    res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
    res.end(shell(repoName(root) ?? "aster"));
  });

  const sendSettings = async () =>
    post({ type: "settings", snapshot: await settingsData.snapshot(root) });

  async function onSettings(message: SettingsToHost): Promise<void> {
    try {
      switch (message.type) {
        case "ready":
        case "reload":
          break;
        case "setKey":
          await asterConfig.set(root, message.key, message.value, message.scope);
          break;
        case "unsetKey":
          await asterConfig.unset(root, message.key, message.scope);
          break;
        case "toggleMcp":
          await info.toggleMcp(root, message.name, message.disabled);
          break;
        case "setEditor":
        case "openConfigFile":
          noEditor(message.type);
          break;
      }
    } catch (err) {
      const key = message.type === "toggleMcp" ? message.name : (message as { key?: string }).key;
      post({ type: "settingsError", key, message: String(err) });
      return;
    }
    await sendSettings();
  }

  async function onMessage(message: ToHost): Promise<void> {
    switch (message.type) {
      case "ready":
        post({
          type: "init",
          workspaceRoot: root,
          repoName: repoName(root),
          branch: await currentBranch(root),
          model: state.model,
          models: [...MODELS, ...state.customModels.filter((m) => !MODELS.includes(m))],
          recommended: [...RECOMMENDED],
          recent: state.recentModels,
          contextBudget: await info.contextBudget(root, env()).catch(() => 0),
          permissionMode: state.permissionMode,
          effort: state.effort,
          binaryOk: await checkBinary(cliConfig().binary),
          skills: await skillCommands(root),
        });
        runState();
        break;

      case "chat": {
        if (chat.running) {
          post({ type: "chatError", id: message.id, message: "A turn is already running." });
          break;
        }
        let sawTerminal = false;
        try {
          const running = chat.run({
            messages: message.messages,
            cwd: root,
            model: message.model,
            permissionMode: message.permissionMode,
            effort: message.effort,
            env: env(),
            session: message.session,
            onEvent: (event) => {
              sawTerminal ||= event.type === "done" || event.type === "error";
              post({ type: "chatEvent", id: message.id, event });
            },
            onStderr: (line) => post({ type: "log", line }),
          });
          runState();
          const code = await running;
          if (!sawTerminal) {
            post({ type: "chatError", id: message.id, message: `aster chat exited with ${code}` });
          }
        } catch (err) {
          post({ type: "chatError", id: message.id, message: describe(err) });
        }
        runState();
        break;
      }

      case "review": {
        if (review.running) {
          post({ type: "reviewError", id: message.id, message: "A review is already running." });
          break;
        }
        post({ type: "reviewStarted", id: message.id, source: message.source });
        try {
          const running = review.run({
            cwd: root,
            source: message.source,
            env: env(),
            onEvent: (event) => post({ type: "reviewEvent", id: message.id, event }),
            onStderr: (line) => post({ type: "log", line }),
          });
          runState();
          await running;
          post({ type: "reviewDone", id: message.id });
        } catch (err) {
          post({ type: "reviewError", id: message.id, message: describe(err) });
        }
        runState();
        break;
      }

      case "cancelChat":
      case "cancelReview":
        chat.cancel();
        review.cancel();
        runState();
        break;

      case "approval":
        chat.approve(message.allow);
        break;
      case "answer":
        chat.answer(message.choice);
        break;
      case "inject":
        chat.inject(message.text);
        break;

      case "setPermissionMode":
        state.permissionMode = message.mode;
        break;
      case "setEffort":
        state.effort = message.effort;
        break;
      case "setModel":
        state.model = message.model || null;
        if (message.model && !MODELS.includes(message.model) && !state.customModels.includes(message.model)) {
          state.customModels.push(message.model);
        }
        if (message.model) {
          state.recentModels = [message.model, ...state.recentModels.filter((m) => m !== message.model)].slice(0, 5);
        }
        break;

      case "searchFiles":
        post({
          type: "fileResults",
          requestId: message.requestId,
          paths: await trackedFiles(root, message.query),
        });
        break;

      case "listSessions":
        post({ type: "sessions", sessions: await listSessions(root).catch(() => []) });
        break;
      case "loadSession":
        try {
          const title = (await listSessions(root)).find((s) => s.id === message.id)?.title ?? null;
          post({ type: "sessionLoaded", id: message.id, title, turns: await loadSession(root, message.id) });
        } catch (err) {
          post({ type: "log", line: describe(err) });
        }
        break;

      case "deleteSession":
      case "renameSession":
        try {
          if (message.type === "deleteSession") {
            await deleteSession(root, message.id);
          } else {
            await renameSession(root, message.id, message.title);
          }
        } catch (err) {
          post({ type: "log", line: describe(err) });
        }
        post({ type: "sessions", sessions: await listSessions(root).catch(() => []) });
        break;

      case "info":
        await sendInfo(message.id, message.topic);
        break;

      case "fetchModels":
        try {
          const { stdout, stderr, code } = await runCli(["models", "--json"], root, undefined, env());
          const parsed = JSON.parse(stdout) as string[] | { error?: string };
          if (code === 0 && Array.isArray(parsed)) {
            post({ type: "modelsLoaded", models: parsed });
          } else {
            const error = Array.isArray(parsed) ? stderr.trim() : parsed.error;
            post({ type: "modelsLoaded", models: [], error: error || "This endpoint did not list its models." });
          }
        } catch (err) {
          post({ type: "modelsLoaded", models: [], error: describe(err) });
        }
        break;

      case "listMcp":
        post({ type: "mcpServers", servers: await info.mcpServers(root).catch(() => []) });
        break;
      case "toggleMcp":
        await info.toggleMcp(root, message.name, message.disabled).catch(() => undefined);
        post({ type: "mcpServers", servers: await info.mcpServers(root).catch(() => []) });
        break;

      case "listProviders":
        post({ type: "providers", providers: await info.providers(root, env()).catch(() => []) });
        break;
      case "setProvider": {
        const catalog = await info.providers(root, env()).catch(() => []);
        const picked = catalog.find((p) => p.base_url === message.baseUrl);
        state.provider = { baseUrl: message.baseUrl, keyEnv: picked?.key_env ?? [] };
        state.model = message.model;
        post({
          type: "providerChanged",
          provider: picked?.name ?? message.baseUrl,
          model: message.model,
          models: await info.modelsFor(root, message.model, env()).catch(() => []),
        });
        break;
      }

      case "compact":
        try {
          post({ type: "compacted", id: message.id, ...(await compactOf(message.messages)) });
        } catch (err) {
          post({ type: "chatError", id: message.id, message: describe(err) });
        }
        break;

      // A dropped file is only path arithmetic, so it works here too.
      case "dropFiles":
        post({
          type: "insertMention",
          text: message.uris
            .map((uri) => path.relative(root, decodeURIComponent(uri.replace(/^file:\/\//, ""))))
            .map((rel) => `@${rel}`)
            .join(" "),
        });
        break;

      default:
        noEditor(message.type);
    }
  }

  async function sendInfo(id: string, topic: "status" | "memory" | "diff"): Promise<void> {
    try {
      if (topic === "status") {
        post({ type: "infoCard", id, title: "Status", rows: await info.status(root, env()) });
        return;
      }
      if (topic === "memory") {
        const rows = await info.memoryBlocks(root);
        post({
          type: "infoCard",
          id,
          title: "Memory",
          rows,
          note: rows.length ? undefined : "Nothing remembered yet.",
        });
        return;
      }
      const diff = await info.workingDiff(root);
      post({
        type: "infoCard",
        id,
        title: "Uncommitted changes",
        body: diff.trim() || undefined,
        lang: "diff",
        note: diff.trim() ? undefined : "No uncommitted changes.",
      });
    } catch (err) {
      post({ type: "infoCard", id, title: topic, note: describe(err), error: true });
    }
  }

  const compactOf = (messages: ChatMessage[]) => info.compact(root, messages, state.model, env());

  server.on("error", (err: NodeJS.ErrnoException) => {
    if (err.code !== "EADDRINUSE") {
      throw err;
    }
    console.error(`port ${port} is already serving a panel — stop it, or pass --port`);
    process.exit(1);
  });
  server.listen(port, () => {
    console.log(`aster panel on http://127.0.0.1:${port}  (repo: ${root})`);
  });
}

function read(req: http.IncomingMessage): Promise<string> {
  return new Promise((resolve) => {
    let body = "";
    req.setEncoding("utf8");
    req.on("data", (chunk: string) => (body += chunk));
    req.on("end", () => resolve(body));
  });
}

function trackedFiles(root: string, query: string): Promise<string[]> {
  return new Promise((resolve) => {
    exec("git ls-files", { cwd: root, maxBuffer: 32 * 1024 * 1024 }, (err, stdout) => {
      if (err) {
        resolve([]);
        return;
      }
      const q = query.toLowerCase();
      const files = stdout.split("\n").filter(Boolean);
      const dirs = new Set<string>();
      for (const file of files) {
        const parts = file.split("/");
        for (let i = 1; i < parts.length; i++) {
          dirs.add(`${parts.slice(0, i).join("/")}/`);
        }
      }
      const hits = [...files, ...dirs].filter((p) => !q || p.toLowerCase().includes(q));
      hits.sort((a, b) => a.split("/").length - b.split("/").length || a.localeCompare(b));
      resolve(hits.slice(0, 50));
    });
  });
}

function describe(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
