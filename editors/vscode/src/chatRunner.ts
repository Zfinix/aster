import { ChildProcess, spawn } from "child_process";
import { cliConfig, missingBinaryMessage } from "./asterCli";
import { ChatMessage, ChatStreamEvent, Effort, PermissionMode } from "./protocol";
import { ToolCallWire, contentText, editedPath, toolResult } from "./acpWire";

export interface ChatOptions {
  messages: ChatMessage[];
  cwd: string;
  model: string | null;
  permissionMode: PermissionMode;
  effort: Effort | null;
  env: NodeJS.ProcessEnv;
  session: string | null;
  onEvent: (event: ChatStreamEvent) => void;
  onStderr: (line: string) => void;
}

interface PendingCall {
  resolve: (result: unknown) => void;
  reject: (err: Error) => void;
}

interface JsonRpcError {
  code: number;
  message: string;
}

interface PermissionOptionWire {
  optionId: string;
  name: string;
  kind: string;
}

interface PendingPrompt {
  options: ChatOptions;
  reply: string;
  edits: string[];
  toolNames: Map<string, string>;
  reasoningChars: number;
  permission:
    | { id: number; question: boolean; options: PermissionOptionWire[] }
    | undefined;
}

/** Owns one long-lived `aster acp` child speaking the Agent Client Protocol on
 *  stdio, so the agent boots once per panel instead of once per turn. The
 *  session lives in the agent across turns; only the newest user message is
 *  sent each time. */
export class ChatRunner {
  private agent: ChildProcess | undefined;
  private agentBuffer = "";
  private pending = new Map<number, PendingCall>();
  private nextId = 0;
  private turn: PendingPrompt | undefined;
  private lastOptions: ChatOptions | undefined;
  private sessionId: string | undefined;
  private loadedSession: string | null | undefined;
  private synced: ChatMessage[] = [];
  private injected: string[] = [];
  private appliedModel: string | null = null;
  private appliedEffort: Effort | null = null;
  private appliedMode: PermissionMode | null = null;
  private loading = false;

  /** The agent's last words. A crash reports its exit through a dead pipe, so
   *  the reason only exists on stderr. */
  private lastStderr: string[] = [];

  /** Counts turns from the moment `run` is called, not from when the chained
   *  turn actually starts. The host broadcasts the run state on the same tick
   *  it calls `run`, and a queued turn that read as idle there would have the
   *  webview close the thread out as stopped before it ever began. */
  private queued = 0;

  get running(): boolean {
    return this.queued > 0;
  }

  private chain: Promise<unknown> = Promise.resolve();

  /** Turns serialize here instead of rejecting: a message sent mid-turn
   *  queues and runs when the current one finishes. */
  run(options: ChatOptions): Promise<number> {
    // A pending approval or question blocks the agent until it is answered, so
    // a message sent instead of an answer would queue behind a turn that can
    // never end. Sending is the answer: drop the prompt and let the turn close.
    this.releasePermission();
    this.queued += 1;
    const result = this.chain.then(() => this.startTurn(options));
    this.chain = result.catch(() => undefined);
    return result.finally(() => {
      this.queued -= 1;
    });
  }

  private startTurn(options: ChatOptions): Promise<number> {
    const turn = {
      options,
      reply: "",
      edits: [],
      toolNames: new Map<string, string>(),
      permission: undefined as PendingPrompt["permission"],
      reasoningChars: 0,
    };
    this.turn = turn;
    this.lastOptions = options;
    return this.prompt(options).then(
      () => 0,
      (err) => {
        // A cancelled turn was already torn down by `cancel`; surfacing the
        // kill as an error would read as a failure.
        const live = this.turn === turn;
        // The slot must free on failure too, or every later message queues
        // behind a turn that already died.
        if (live) {
          this.releasePermission();
          this.turn = undefined;
          turn.options.onEvent({
            type: "error",
            message: err instanceof Error ? err.message : String(err),
          });
        }
        return 1;
      }
    );
  }

  /** The messages the agent has not seen yet, or null when this transcript
   *  cannot be lined up against the last one and the session has to be rebuilt.
   *  The webview caps the history it sends, so the transcript slides forward
   *  from the front: a cursor counted in messages stops moving once the cap is
   *  reached and every later message reads as nothing new. Aligning the tail of
   *  the last sync against the head of this one keeps the delta honest however
   *  much has fallen off. */
  private unsent(messages: ChatMessage[]): ChatMessage[] | null {
    const same = (a: ChatMessage, b: ChatMessage) =>
      a.role === b.role && a.content === b.content;
    if (this.synced.length === 0) {
      return messages;
    }
    for (let dropped = 0; dropped < this.synced.length; dropped++) {
      const kept = this.synced.slice(dropped);
      if (kept.length > messages.length) {
        continue;
      }
      if (kept.every((message, i) => same(message, messages[i]))) {
        return messages.slice(kept.length);
      }
    }
    return null;
  }

  private async prompt(options: ChatOptions): Promise<void> {
    await this.ensureAgent(options);
    const wanted = options.session ?? null;
    const last = options.messages[options.messages.length - 1];
    const bound =
      this.sessionId !== undefined &&
      this.loadedSession === wanted &&
      !(options.messages.length === 1 && this.synced.length >= 1);
    let unsent = bound ? this.unsent(options.messages) : null;
    // A message the agent never saw must never end as a silent no-op turn:
    // rebuilding the session resyncs the history and sends it.
    if (unsent === null || (unsent.length === 0 && last?.role === "user")) {
      await this.bindSession(options, wanted);
      unsent = this.unsent(options.messages) ?? options.messages;
    }
    await this.applyConfig(options);

    // Only the messages the agent has not seen go over the wire. A compacted
    // history rebuilds the session above, so `fresh` there is the whole
    // transcript and only its user messages can be replayed as prompt text.
    const fresh = unsent
      .filter((m) => m.role === "user")
      .map((m) => ({ type: "text", text: m.content }));
    const injected = this.injected.splice(0);
    if (injected.length > 0) {
      fresh.unshift({ type: "text", text: injected.join("\n\n") });
    }
    if (fresh.length === 0) {
      this.synced = [...options.messages];
      this.finishTurn(undefined);
      return;
    }

    const result = (await this.call("session/prompt", {
      sessionId: this.sessionId,
      prompt: fresh,
    })) as { stopReason?: string };
    // Only mark the history as seen once the agent accepted it, so a failed
    // prompt resends the message instead of silently dropping it.
    this.synced = [...options.messages];
    this.finishTurn(result?.stopReason);
  }

  private finishTurn(stopReason: string | undefined): void {
    const turn = this.turn;
    this.turn = undefined;
    if (!turn) {
      return;
    }
    if (stopReason === "refusal") {
      turn.options.onEvent({
        type: "error",
        message: "The model refused to continue this turn.",
      });
      return;
    }
    turn.options.onEvent({
      type: "done",
      reply: turn.reply,
      edits: turn.edits,
    });
  }

  approve(allow: boolean): void {
    const permission = this.turn?.permission;
    if (!permission || permission.question) {
      return;
    }
    this.respondPermission(permission.id, permission.options, allow ? "allow" : "reject");
  }

  answer(choice: string | null): void {
    const permission = this.turn?.permission;
    if (!permission || !permission.question) {
      return;
    }
    const picked = choice
      ? permission.options.find((o) => o.name === choice)?.optionId
      : undefined;
    this.respondPermission(permission.id, permission.options, picked ?? "skip");
  }

  private respondPermission(
    id: number,
    options: PermissionOptionWire[],
    wanted: string
  ): void {
    if (this.turn?.permission?.id === id) {
      this.turn.permission = undefined;
    }
    const picked =
      options.find((o) => o.optionId === wanted) ??
      options.find((o) => o.kind === "allow_once" || o.kind === "allow_always") ??
      options.find((o) => o.kind === "reject_once") ??
      options[0];
    this.respond(id, {
      outcome: picked
        ? { outcome: "selected", optionId: picked.optionId }
        : { outcome: "cancelled" },
    });
  }

  /** The agent blocks on its permission request until a reply lands, so every
   *  path out of one has to write a response back. */
  private respond(id: number, result: unknown): void {
    this.agent?.stdin?.write(
      `${JSON.stringify({ jsonrpc: "2.0", id, result })}\n`
    );
  }

  private respondError(id: number, message: string): void {
    const error = { code: -32601, message };
    this.agent?.stdin?.write(
      `${JSON.stringify({ jsonrpc: "2.0", id, error })}\n`
    );
  }

  private releasePermission(): void {
    const permission = this.turn?.permission;
    if (!permission) {
      return;
    }
    this.turn!.permission = undefined;
    this.respond(permission.id, { outcome: { outcome: "cancelled" } });
  }

  /** Mid-turn the text goes over the wire as a prompt: the agent joins it to
   *  the running turn at the next round boundary. Idle, it prepends to the
   *  next prompt instead. */
  inject(text: string): void {
    if (this.turn && this.agent && this.sessionId !== undefined) {
      this.call("session/prompt", {
        sessionId: this.sessionId,
        prompt: [{ type: "text", text }],
      }).catch(() => {
        // The agent died taking the steer; carry it into the next turn.
        this.injected.push(text);
      });
      return;
    }
    this.injected.push(text);
  }

  cancel(): void {
    if (this.turn) {
      // ACP requires the client to answer every outstanding permission request
      // when it cancels, or the agent's turn never unwinds.
      this.releasePermission();
      if (this.agent && this.sessionId !== undefined) {
        this.notify("session/cancel", { sessionId: this.sessionId });
      }
    }
    // Take the agent down either way: a cancel that the agent fails to unwind
    // would otherwise leave the pending prompt call and the turn chain blocked
    // forever, so every later message queues behind a turn that already died.
    this.turn = undefined;
    this.agent?.kill();
    this.failAll(new Error("aster acp was cancelled. The next message restarts it."));
  }

  private async bindSession(options: ChatOptions, wanted: string | null): Promise<void> {
    const params = { cwd: options.cwd, mcpServers: [] };
    let sessionId: string;
    if (wanted) {
      this.loading = true;
      try {
        const loaded = (await this.call("session/load", {
          ...params,
          sessionId: wanted,
        })) as { sessionId?: string };
        sessionId = loaded?.sessionId ?? wanted;
      } finally {
        this.loading = false;
      }
      // The transcript was rendered from the store already; the new user
      // message at the end is the only part the agent has not seen.
      const last = options.messages[options.messages.length - 1];
      this.synced = options.messages.slice(
        0,
        options.messages.length - (last?.role === "user" ? 1 : 0)
      );
    } else {
      const created = (await this.call("session/new", params)) as {
        sessionId?: string;
      };
      sessionId = created?.sessionId ?? "";
      this.synced = [];
    }
    this.sessionId = sessionId;
    this.loadedSession = wanted;
    this.appliedModel = this.appliedEffort = this.appliedMode = null;
  }

  private async applyConfig(options: ChatOptions): Promise<void> {
    const session = this.sessionId;
    if (!session) {
      return;
    }
    const set = (configId: string, value: string) =>
      this.call("session/set_config_option", { sessionId: session, configId, value });
    if (options.model && options.model !== this.appliedModel) {
      await set("model", options.model);
      this.appliedModel = options.model;
    }
    if (options.effort && options.effort !== this.appliedEffort) {
      await set("effort", options.effort);
      this.appliedEffort = options.effort;
    }
    if (options.permissionMode !== this.appliedMode) {
      await this.call("session/set_mode", {
        sessionId: session,
        modeId: options.permissionMode,
      });
      this.appliedMode = options.permissionMode;
    }
  }

  private async ensureAgent(options: ChatOptions): Promise<ChildProcess> {
    if (this.agent && this.agent.exitCode === null) {
      return this.agent;
    }
    const { binary } = cliConfig();
    const child = spawn(binary, ["acp"], {
      cwd: options.cwd,
      env: options.env,
      stdio: ["pipe", "pipe", "pipe"],
    });
    this.agent = child;
    this.agentBuffer = "";
    this.lastStderr = [];
    this.sessionId = undefined;
    this.loadedSession = undefined;
    this.appliedModel = this.appliedEffort = this.appliedMode = null;

    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => this.onAgentData(chunk));
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk: string) => {
      for (const line of chunk.split("\n")) {
        if (line.trim()) {
          this.rememberStderr(line);
          (this.turn?.options ?? this.lastOptions)?.onStderr(line);
        }
      }
    });
    child.on("error", (err: NodeJS.ErrnoException) => {
      const message =
        err.code === "ENOENT" ? missingBinaryMessage(binary) : String(err);
      this.failAll(new Error(message));
    });
    child.on("close", (code, signal) => {
      this.failAll(new Error(this.crashMessage(code, signal)));
    });

    await this.call("initialize", {
      protocolVersion: 1,
      clientCapabilities: { fs: { readTextFile: false, writeTextFile: false } },
      clientInfo: { name: "aster-vscode", title: "Aster", version: "0.5.0" },
    });
    return child;
  }

  private rememberStderr(line: string): void {
    this.lastStderr.push(line);
    if (this.lastStderr.length > 8) {
      this.lastStderr.shift();
    }
  }

  /** Quotes what the agent printed before it died, so the panel explains the
   *  failure instead of pointing at the output channel. */
  private crashMessage(code: number | null, signal: NodeJS.Signals | null): string {
    const how = signal
      ? `was killed by ${signal}`
      : `exited with code ${code ?? 1}`;
    const said = this.lastStderr.join("\n").trim();
    this.lastStderr = [];
    return said
      ? `aster acp ${how}: ${said}`
      : `aster acp ${how} without printing a reason. The next message restarts it.`;
  }

  private call(method: string, params: unknown): Promise<unknown> {
    const agent = this.agent;
    if (!agent || agent.exitCode !== null || !agent.stdin?.writable) {
      return Promise.reject(new Error(`aster acp is not running for ${method}`));
    }
    const id = ++this.nextId;
    const stdin = agent.stdin;
    if (!stdin) {
      return Promise.reject(new Error(`aster acp is not running for ${method}`));
    }
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
    });
  }

  private notify(method: string, params: unknown): void {
    const payload = JSON.stringify({ jsonrpc: "2.0", method, params });
    this.agent?.stdin?.write(`${payload}\n`);
  }

  private resolvePending(id: number, result: unknown): void {
    this.pending.get(id)?.resolve(result);
    this.pending.delete(id);
  }

  private rejectPending(id: number, err: Error): void {
    this.pending.get(id)?.reject(err);
    this.pending.delete(id);
  }

  private failAll(err: Error): void {
    for (const id of [...this.pending.keys()]) {
      this.rejectPending(id, err);
    }
    this.agent = undefined;
    this.sessionId = undefined;
  }

  private onAgentData(chunk: string): void {
    this.agentBuffer += chunk;
    let newline;
    while ((newline = this.agentBuffer.indexOf("\n")) !== -1) {
      const line = this.agentBuffer.slice(0, newline).trim();
      this.agentBuffer = this.agentBuffer.slice(newline + 1);
      if (!line) {
        continue;
      }
      let message: {
        id?: number;
        method?: string;
        params?: unknown;
        result?: unknown;
        error?: JsonRpcError;
      };
      try {
        message = JSON.parse(line);
      } catch {
        (this.turn?.options ?? this.lastOptions)?.onStderr(line);
        continue;
      }
      if (message.method && message.id !== undefined) {
        this.handleServerRequest(message.id, message.method, message.params);
      } else if (message.id !== undefined) {
        if (message.error) {
          this.rejectPending(message.id, new Error(message.error.message));
        } else {
          this.resolvePending(message.id, message.result);
        }
      } else if (message.method === "session/update") {
        this.handleUpdate(message.params);
      }
    }
  }

  private handleServerRequest(id: number, method: string, params: unknown): void {
    if (method !== "session/request_permission") {
      // No fs or terminal capabilities were declared, so the agent should
      // never ask; anything here gets a clean refusal rather than a hang.
      this.respondError(id, `${method} is not supported`);
      return;
    }
    const request = params as {
      toolCall?: ToolCallWire;
      options?: PermissionOptionWire[];
    };
    if (!this.turn) {
      this.respond(id, { outcome: { outcome: "cancelled" } });
      return;
    }
    const options = request.options ?? [];
    const call = request.toolCall;
    const question = call?.kind === "think";
    this.turn.permission = { id, question, options };
    const body = contentText(call?.content);
    if (question) {
      this.turn.options.onEvent({
        type: "question",
        header: call?.title ?? "A question",
        question: body,
        options: options.filter((o) => o.kind === "allow_once").map((o) => o.name),
      });
    } else {
      this.turn.options.onEvent({
        type: "approval_request",
        kind: call?.kind === "switch_mode" ? "plan" : "action",
        preview: call?.title ?? "Approve this action",
        markdown: body || null,
        scope: null,
      });
    }
  }

  private handleUpdate(params: unknown): void {
    const turn = this.turn;
    const update = (params as { update?: Record<string, unknown> })?.update;
    if (!update) {
      return;
    }
    if (this.loading) {
      // A loaded session replays its history as updates; the webview already
      // rendered that transcript from the store.
      return;
    }
    const kind = update["sessionUpdate"];
    if (kind === "agent_message_chunk") {
      const text = contentText([update["content"]]);
      if (turn) {
        turn.reply += text;
      }
      this.emit({ type: "token", content: text });
    } else if (kind === "agent_thought_chunk") {
      const chunk = contentText([update["content"]]);
      // ACP thought chunks carry no token count, so estimate from the
      // accumulated text the same way the CLI does (chars / 4).
      turn && (turn.reasoningChars += chunk.length);
      this.emit({
        type: "reasoning_delta",
        content: chunk,
        tokens: Math.ceil((turn?.reasoningChars ?? chunk.length) / 4),
      });
    } else if (kind === "tool_call") {
      const id = String(update["toolCallId"] ?? "");
      const name = String(update["name"] ?? update["title"] ?? "tool");
      turn?.toolNames.set(id, name);
      const raw = update["rawInput"];
      const edited = editedPath(update as ToolCallWire);
      if (edited && turn && !turn.edits.includes(edited)) {
        turn.edits.push(edited);
      }
      this.emit({
        type: "tool_call",
        id,
        name,
        arguments: typeof raw === "string" ? raw : JSON.stringify(raw ?? {}),
      });
    } else if (kind === "tool_call_update") {
      // Sub-agent progress rides in meta as the raw stream event; it arrives
      // while the call is still in progress, so it must be read before the
      // completed-only tool result check.
      const meta = (update["_meta"] ?? update["meta"]) as
        | { agent_event?: ChatStreamEvent }
        | undefined;
      const agentEvent = meta?.agent_event;
      if (agentEvent?.type === "agent_status" || agentEvent?.type === "agent_activity") {
        this.emit(agentEvent);
        return;
      }
      const result = toolResult(update as ToolCallWire);
      if (!result) {
        return;
      }
      this.emit({
        type: "tool_result",
        id: result.id,
        name: turn?.toolNames.get(result.id) ?? "",
        result: result.result,
        error: result.error,
      });
    } else if (kind === "session_info_update") {
      const title = update["title"];
      if (typeof title === "string" && title) {
        this.emit({ type: "title", title });
      }
    } else if (kind === "current_mode_update") {
      const modeId = update["currentModeId"];
      if (typeof modeId === "string") {
        this.appliedMode = modeId as PermissionMode;
      }
    }
    // Plan and available-commands updates have no webview equivalent, so they
    // are dropped here rather than smuggled through as text.
  }

  private emit(event: ChatStreamEvent): void {
    if (!this.loading) {
      // The title is named on the agent's turn (spawned detached, so it can
      // land after the turn closed); the last options still own its surface.
      (this.turn?.options ?? this.lastOptions)?.onEvent(event);
    }
  }
}
