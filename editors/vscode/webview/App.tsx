import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type {
  ChatStreamEvent,
  Effort,
  InfoRow,
  McpServer,
  PermissionMode,
  Provider,
  ReviewSource,
  SessionSummary,
  SetupInfo,
  SkillCommand,
  ToWebview,
} from "../src/protocol";
import { Composer } from "./components/Composer";
import { EmptyState } from "./components/EmptyState";
import { HistoryPanel } from "./components/HistoryPanel";
import { InfoModal } from "./components/InfoModal";
import { FilePreview } from "./components/FilePreview";
import { FindBar } from "./components/FindBar";
import { Thread } from "./components/Thread";
import { Toolbar } from "./components/Toolbar";
import { inEditor, nativeFind, onHostMessage, persist, post, restore } from "./lib/host";
import { type LoginState, loginLine } from "./lib/login";
import { modelShort, recentsFor } from "./lib/model";
import { closePlan, onPlanAnswer } from "./lib/plan-tab";
import {
  appendAgentActivity,
  appendCall,
  appendGoalVerdict,
  appendInjected,
  appendReasoning,
  appendReasoningDelta,
  appendText,
  buildMessages,
  emptyReview,
  finishReasoning,
  parseDiffFiles,
  hydrate,
  newTurn,
  patchCall,
  restoreTurn,
  setGoal,
  stopUnfinished,
  upsertAgentState,
  type AssistantTurn,
  type InfoCardData,
  type ReviewData,
  type Turn,
} from "./lib/thread";


let counter = 0;
const nextId = () => `t${++counter}`;

const newSessionId = () => `vsc-${Date.now().toString(36)}`;

interface Persisted {
  turns: Turn[];
  session: string;
}

interface Init {
  repoName: string | null;
  branch: string | null;
  model: string | null;
  models: string[];
  recommended: string[];
  recent: string[];
  contextBudget: number;
  binaryOk: boolean;
  skills: SkillCommand[];
  setup: SetupInfo | null;
}

export function App() {
  const saved = restore<Persisted>();
  const [init, setInit] = useState<Init | null>(null);
  const [login, setLogin] = useState<LoginState | null>(null);
  const [permissionMode, setPermissionMode] = useState<PermissionMode>("edit");
  const [effort, setEffort] = useState<Effort | null>(null);
  const [turns, setTurns] = useState<Turn[]>(() => hydrate(saved?.turns));
  const [session, setSession] = useState(saved?.session ?? newSessionId());
  const [sessionTitle, setSessionTitle] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [fileResults, setFileResults] = useState<string[]>([]);
  const [pendingMention, setPendingMention] = useState<{
    text: string;
    mentions?: string[];
  } | null>(null);
  const [queued, setQueued] = useState<string[]>([]);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [showHistory, setShowHistory] = useState(false);
  const [info, setInfo] = useState<InfoCardData | null>(null);
  const [mcpServers, setMcpServers] = useState<McpServer[]>([]);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [openMenu, setOpenMenu] = useState(false);
  const closeMenuRequest = useCallback(() => setOpenMenu(false), []);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelsError, setModelsError] = useState<string | undefined>();
  const [catalog, setCatalog] = useState<string[] | null>(null);
  const refreshModels = useCallback(() => {
    setModelsLoading(true);
    setModelsError(undefined);
    post({ type: "fetchModels" });
  }, []);

  // Ids are regenerated each mount, so restart the counter past the restored
  // turns to avoid colliding with them.
  useEffect(() => {
    counter = Math.max(counter, saved?.turns.length ?? 0);
  }, []);

  // Read by the message handler, which is mounted once and cannot close over
  // the current turns.
  const turnsRef = useRef<Turn[]>(turns);

  useEffect(() => {
    turnsRef.current = turns;
    persist({ turns, session });
  }, [turns, session]);

  // The in-flight request id, so a stale event after a cancel is ignored.
  const activeRef = useRef<string | null>(null);
  const fileRequestRef = useRef<string>("");
  const planApprovalRef = useRef(false);

  const modelRef = useRef<string | null>(null);
  const markModelChange = useCallback((next: string | null) => {
    if (modelRef.current === next) return;
    modelRef.current = next;
    setTurns((prev) => {
      if (prev.length === 0) return prev;
      const divider = { id: nextId(), role: "divider" as const, label: modelShort(next) };
      return prev[prev.length - 1].role === "divider"
        ? [...prev.slice(0, -1), divider]
        : [...prev, divider];
    });
  }, []);

  // A stopped review is final: the CLI's own exit event arrives after the kill
  // and would otherwise overwrite "Stopped" with an exit-code error.
  const patchReview = (id: string, patch: (data: ReviewData) => ReviewData) =>
    setTurns((prev) =>
      prev.some((turn) => turn.role === "review" && turn.id === id)
        ? prev.map((turn) =>
            turn.role === "review" && turn.id === id && turn.data.status !== "stopped"
              ? { ...turn, data: patch(turn.data) }
              : turn
          )
        : [...prev, { id, role: "review", data: patch(emptyReview()) }]
    );

  const patchAssistant = (id: string, patch: (turn: AssistantTurn) => AssistantTurn) =>
    setTurns((prev) =>
      prev.map((turn) =>
        turn.role === "assistant" && turn.id === id ? patch(turn) : turn
      )
    );

  const handle = useCallback((message: ToWebview) => {
    switch (message.type) {
      case "init":
        setInit({
          repoName: message.repoName,
          branch: message.branch,
          model: message.model,
          models: message.models,
          recommended: message.recommended,
          recent: message.recent,
          contextBudget: message.contextBudget,
          binaryOk: message.binaryOk,
          skills: message.skills,
          setup: message.setup ?? null,
        });
        setPermissionMode(message.permissionMode);
        setEffort(message.effort);
        modelRef.current = message.model;
        break;

      case "chatEvent":
        if (activeRef.current !== message.id) break;
        applyChatEvent(message.id, message.event);
        break;

      case "loginOutput":
        setLogin((prev) => loginLine(prev, message.line));
        break;

      case "loginDone":
        setLogin((prev) => ({ ...(prev ?? { lines: [] }), done: true, ok: message.ok, message: message.message }));
        break;

      case "chatError":
        if (activeRef.current !== message.id) break;
        activeRef.current = null;
        setBusy(false);
        patchAssistant(message.id, (turn) => ({
          ...turn,
          errorMsg: message.message,
          pending: false,
          error: true,
          approval: undefined,
          question: undefined,
        }));
        break;

      case "fileResults":
        if (fileRequestRef.current === message.requestId) {
          setFileResults(message.paths);
        }
        break;

      case "reviewStarted":
        activeRef.current = message.id;
        setBusy(true);
        patchReview(message.id, (data) => data);
        break;

      case "reviewEvent":
        patchReview(message.id, (data) => applyReviewEvent(data, message.event));
        break;

      case "reviewDone":
        if (activeRef.current === message.id) {
          activeRef.current = null;
          setBusy(false);
        }
        patchReview(message.id, (data) => ({ ...data, status: "done" }));
        break;

      case "reviewError":
        if (activeRef.current === message.id) {
          activeRef.current = null;
          setBusy(false);
        }
        patchReview(message.id, (data) => ({
          ...data,
          status: "error",
          errorMsg: message.message,
        }));
        break;

      case "newConversation":
        activeRef.current = null;
        setBusy(false);
        setTurns([]);
        setQueued([]);
        setSession(newSessionId());
        setSessionTitle(null);
        break;

      case "sessionLoaded":
        activeRef.current = null;
        setBusy(false);
        setQueued([]);
        setSession(message.id);
        setSessionTitle(message.title);
        setTurns(
          message.turns.map((turn) =>
            turn.role === "user"
              ? { id: nextId(), role: "user", text: turn.content }
              : restoreTurn(nextId(), turn.content, turn.reasoning, turn.toolCalls)
          )
        );
        break;

      case "insertMention":
        setPendingMention({ text: message.text, mentions: message.mentions });
        break;

      case "openCommandMenu":
        setOpenMenu(true);
        break;

      case "scratch":
        setInfo({
          id: nextId(),
          title: message.title || "Snippet",
          body: message.content,
          lang: message.lang,
          doc: message.doc,
        });
        break;

      // The host is the source of truth for whether anything is running, so a
      // surface can never be stuck busy or wrongly re-enable its buttons.
      case "runState": {
        const running = message.review || message.chat;
        setBusy(running);
        if (message.id) activeRef.current = message.id;
        // A surface that loaded mid-prompt never saw the original event, so
        // the host re-sends whatever the turn is blocked on.
        if (message.pending && message.id) {
          const event = message.pending;
          const open = [...turnsRef.current]
            .reverse()
            .find((turn) => turn.role === "assistant" && turn.pending);
          const target = open?.id ?? nextId();
          if (!open) setTurns((prev) => [...prev, newTurn(target)]);
          reapplyPending(target, event);
        }
        if (!running) {
          activeRef.current = null;
          // Nothing is in flight, so any turn still spinning never will be:
          // a cancel, a crash, or an error that raced this message.
          setTurns(stopUnfinished);
        }
        break;
      }

      case "sessions":
        setSessions(message.sessions);
        break;
      case "log":
        break;
      // Merged, not replaced: the vetted list and any hand-typed id have to
      // survive an endpoint that answers with a catalog of its own.
      case "modelsLoaded":
        setModelsLoading(false);
        setModelsError(message.error);
        setCatalog(message.models.length ? message.models : null);
        setInit((prev) =>
          prev ? { ...prev, models: [...new Set([...prev.models, ...message.models])] } : prev
        );
        break;

      case "mcpServers":
        setMcpServers(message.servers);
        break;
      case "providers":
        setProviders(message.providers);
        break;
      case "providerChanged":
        setCatalog(message.models.length ? message.models : null);
        setInit((prev) =>
          prev
            ? {
                ...prev,
                model: message.model,
                models: message.models.length ? message.models : prev.models,
              }
            : prev
        );
        markModelChange(message.model);
        setProviders((prev) =>
          prev.map((p) => ({ ...p, current: p.name === message.provider }))
        );
        break;

      case "infoCard": {
        const { type: _type, id, ...card } = message;
        setInfo((open) => {
          if (!open || open.id !== id) return open;
          // The CLI has no view of a conversation it is not running, so the
          // session's own rows are filled in here.
          const rows =
            card.rows && open.title === "Status"
              ? [...card.rows, ...sessionRows(turnsRef.current)]
              : card.rows;
          return { ...open, ...card, rows, pending: false };
        });
        break;
      }
      // The thread is left alone: the placeholder becomes a seam, everything
      // above it stays readable, and from here on the folded history is what
      // the next turn sends in its place.
      case "compacted":
        activeRef.current = null;
        setBusy(false);
        setTurns((prev) =>
          prev.map((turn) =>
            turn.id === message.id
              ? {
                  id: message.id,
                  role: "compaction",
                  summary: message.summary,
                  folded: message.folded,
                  messages: message.messages.map(({ role, content }) => ({ role, content })),
                }
              : turn
          )
        );
        break;
    }
  }, []);

  const reapplyPending = (id: string, event: ChatStreamEvent) => {
    if (event.type === "approval_request") {
      planApprovalRef.current = event.kind === "plan";
      patchAssistant(id, (turn) => ({
        ...turn,
        approval: {
          preview: event.preview,
          markdown: event.markdown,
          scope: event.scope,
          kind: event.kind,
        },
      }));
    } else if (event.type === "question") {
      patchAssistant(id, (turn) => ({
        ...turn,
        question: {
          header: event.header,
          question: event.question,
          options: event.options,
        },
      }));
    }
  };

  const applyChatEvent = (id: string, event: ChatStreamEvent) => {
    switch (event.type) {
      case "token":
        patchAssistant(id, (turn) => ({ ...appendText(turn, event.content), streamed: true }));
        break;
      case "text":
        patchAssistant(id, (turn) => appendText(turn, event.content, "\n\n"));
        break;
      case "reasoning_delta":
        patchAssistant(id, (turn) => appendReasoningDelta(turn, event.content, event.tokens));
        break;
      case "reasoning_done":
        patchAssistant(id, (turn) => finishReasoning(turn, event.tokens, event.duration_ms));
        break;
      case "reasoning":
        patchAssistant(id, (turn) => appendReasoning(turn, event.content, event.tokens, event.duration_ms));
        break;
      case "tool_call":
        patchAssistant(id, (turn) =>
          appendCall(turn, { id: event.id, name: event.name, arguments: event.arguments })
        );
        break;
      case "tool_result":
        patchAssistant(id, (turn) => ({
          ...patchCall(turn, event.id, { result: event.result, error: event.error }),
          approval: undefined,
          question: undefined,
        }));
        break;
      case "approval_request":
        planApprovalRef.current = event.kind === "plan";
        patchAssistant(id, (turn) => ({
          ...turn,
          approval: {
            preview: event.preview,
            markdown: event.markdown,
            scope: event.scope,
            kind: event.kind,
          },
        }));
        break;
      case "question":
        patchAssistant(id, (turn) => ({
          ...turn,
          question: {
            header: event.header,
            question: event.question,
            options: event.options,
          },
        }));
        break;
      case "agent_activity":
        patchAssistant(id, (turn) =>
          appendAgentActivity(turn, event.call_id, event.agent, event.task, event.line)
        );
        break;
      case "goal_set":
        patchAssistant(id, (turn) => setGoal(turn, event.condition, event.max_turns));
        break;
      case "goal_verdict":
        patchAssistant(id, (turn) =>
          appendGoalVerdict(
            turn,
            { verdict: event.verdict, reason: event.reason, turn: event.turn },
            event.final
          )
        );
        break;
      case "compacted":
        // The CLI folded the history mid-turn; everything above this seam is
        // dropped from future sends but stays readable.
        setTurns((prev) => [
          {
            id: nextId(),
            role: "compaction",
            summary: event.summary,
            folded: event.folded,
            messages: event.messages.map(({ role, content }) => ({ role, content })),
          },
          ...prev,
        ]);
        break;
      case "agent_status":
        patchAssistant(id, (turn) =>
          upsertAgentState(turn, {
            callId: event.call_id,
            agent: event.agent,
            task: event.task,
            status: event.status,
            report: event.report,
            error: event.error,
            done: event.done,
            total: event.total,
            ...(event.status === "running" ? { startedAt: Date.now() } : { endedAt: Date.now() }),
          })
        );
        break;
      case "notice":
        // Reads as part of the turn, since it is the harness explaining what it
        // just did to that turn.
        patchAssistant(id, (turn) => appendText(turn, `_${event.message}_`, "\n\n"));
        break;
      case "injected": {
        // The turn absorbed a queued message; move it from the queue into the
        // transcript at the spot it actually landed.
        const content = event.content;
        setQueued((prev) => {
          const at = prev.indexOf(content);
          return at === -1 ? prev : prev.filter((_, i) => i !== at);
        });
        patchAssistant(id, (turn) => appendInjected(turn, content));
        break;
      }
      case "title":
        setSessionTitle(event.title);
        break;
      case "done":
        activeRef.current = null;
        setBusy(false);
        if (event.context_budget) {
          setEffectiveBudget(event.context_budget);
        }
        patchAssistant(id, (turn) => ({
          ...(turn.streamed ? turn : appendText(turn, event.reply, "\n\n")),
          edits: event.edits,
          usage: event.usage,
          pending: false,
          approval: undefined,
          question: undefined,
        }));
        break;
      case "error":
        activeRef.current = null;
        setBusy(false);
        if (event.setup) {
          setLogin(null);
        }
        patchAssistant(id, (turn) => ({
          ...turn,
          errorMsg: event.message,
          setup: event.setup,
          pending: false,
          error: true,
          approval: undefined,
          question: undefined,
        }));
        break;
    }
  };

  useEffect(() => {
    const unsubscribe = onHostMessage(handle);
    post({ type: "ready" });
    return unsubscribe;
  }, [handle]);

  const choosePermissionMode = (mode: PermissionMode) => {
    setPermissionMode(mode);
    post({ type: "setPermissionMode", mode });
  };

  const answerApproval = (allow: boolean, always?: boolean) => {
    post({ type: "approval", allow, always });
    const plan = planApprovalRef.current;
    planApprovalRef.current = false;
    // The plan's own tab, if one is open, has nothing left to decide.
    if (plan) closePlan();
    // Only `plan` is unlocked by the approval; every other mode already edits,
    // so promoting would narrow yolo rather than free anything.
    if (plan && allow && permissionMode === "plan") {
      choosePermissionMode("edit");
    }
  };

  useEffect(
    () =>
      onPlanAnswer((answer) => {
        if (planApprovalRef.current) answerApproval(answer.allow);
      }),
    [permissionMode]
  );

  const redirectApproval = (instead: string) => {
    answerApproval(false);
    setQueued((prev) => [...prev, instead]);
    post({ type: "inject", text: instead });
  };

  const startTurn = (text: string) => {
    const id = nextId();
    const history: Turn[] = [
      ...turns,
      { id: nextId(), role: "user", text },
      newTurn(id),
    ];
    setTurns(history);
    setBusy(true);
    activeRef.current = id;
    post({
      type: "chat",
      id,
      messages: buildMessages(history),
      model: init?.model ?? null,
      permissionMode,
      effort,
      session,
    });
  };

  // Edit, rewind, and fork all cut the transcript at a user message. Trimming
  // here is display-level: the CLI keeps the full transcript on disk, and the
  // next `chat` post simply carries the shorter history.
  const cutAt = (id: string): { history: Turn[]; at: number } | null => {
    const at = turns.findIndex((turn) => turn.id === id && turn.role === "user");
    if (at === -1) return null;
    return { history: turns.slice(0, at + 1), at };
  };

  const resendFrom = (id: string, text: string) => {
    if (busy) return;
    setEditing(null);
    const cut = cutAt(id);
    if (!cut) return;
    const replaced: Turn[] = [...cut.history];
    replaced[cut.at] = { id, role: "user", text };
    const tid = nextId();
    const history: Turn[] = [...replaced, newTurn(tid)];
    setTurns(history);
    setBusy(true);
    activeRef.current = tid;
    post({
      type: "chat",
      id: tid,
      messages: buildMessages(history),
      model: init?.model ?? null,
      permissionMode,
      effort,
      session,
    });
  };

  const rewindTo = (id: string) => {
    if (busy) return;
    setEditing(null);
    const cut = cutAt(id);
    if (!cut) return;
    setQueued([]);
    setTurns(cut.history);
  };

  const forkFrom = (id: string) => {
    if (busy) return;
    setEditing(null);
    const cut = cutAt(id);
    if (!cut) return;
    setQueued([]);
    setSession(newSessionId());
    setSessionTitle(null);
    setTurns(cut.history);
  };

  const [editing, setEditing] = useState<string | null>(null);

  // Flushed one at a time so history stays ordered.
  const onSend = (text: string) => {
    if (busy) {
      // Queue locally and offer it to the running turn; the CLI absorbs it at
      // the next round boundary and answers with an `injected` event. Anything
      // not absorbed by turn end flushes as its own turn below.
      setQueued((prev) => [...prev, text]);
      post({ type: "inject", text });
      return;
    }
    startTurn(text);
  };

  useEffect(() => {
    if (busy || queued.length === 0) return;
    const [next, ...rest] = queued;
    setQueued(rest);
    startTurn(next);
  }, [busy, queued]);

  const startReview = (source: ReviewSource) => {
    const id = nextId();
    setTurns((prev) => [...prev, { id, role: "review", data: emptyReview() }]);
    setBusy(true);
    activeRef.current = id;
    post({ type: "review", id, source });
  };

  const askInfo = (topic: "status" | "memory" | "diff" | "mom") => {
    const id = nextId();
    setInfo({ id, title: infoTitle(topic), pending: true });
    post({ type: "info", id, topic });
  };

  const onCommand = (name: string) => {
    switch (name) {
      case "review":
        startReview({ kind: "working" });
        break;
      case "status":
      case "memory":
      case "diff":
        askInfo(name);
        break;
      // Loading like any other turn: the placeholder is a pending assistant
      // turn, so a failure lands on it as an error the same way too.
      case "compact": {
        const id = nextId();
        setTurns((prev) => [...prev, newTurn(id)]);
        setBusy(true);
        activeRef.current = id;
        post({ type: "compact", id, messages: buildMessages(turns, Infinity) });
        break;
      }
      case "review-range":
        post({ type: "runCommand", command: "aster.reviewRange" });
        break;
      case "review-pr":
        post({ type: "runCommand", command: "aster.reviewPr" });
        break;
      case "resume":
        post({ type: "listSessions" });
        setShowHistory(true);
        break;
      case "new":
        post({ type: "runCommand", command: "aster.newConversation" });
        break;
      case "clear":
        setTurns([]);
        setQueued([]);
        break;
      case "mom":
        askInfo("mom");
        break;
      // Answered here rather than by the host: the command list is the
      // webview's own menu, so nothing needs to leave the panel for it.
      case "help":
        setInfo({ id: nextId(), title: "Commands", body: HELP_BODY, doc: true });
        break;
    }
  };

  // The host cancels whichever run is live and echoes the real state back, so
  // the webview no longer has to guess which kind is in flight. The thread is
  // closed out here rather than on the echo, so Stop reads as instant, and the
  // queue is dropped because flushing it would start a new turn right away.
  const onCancel = () => {
    activeRef.current = null;
    setBusy(false);
    setQueued([]);
    setTurns(stopUnfinished);
    post({ type: "cancelReview" });
  };

  const onSearchFiles = (query: string) => {
    const requestId = nextId();
    fileRequestRef.current = requestId;
    post({ type: "searchFiles", query, requestId });
  };

  // The effective budget the CLI reported for this session (compact budget
  // minus the system prompt's share), once a turn has told us.
  const [effectiveBudget, setEffectiveBudget] = useState<number | null>(null);
  const contextUsed = useMemo(
    () => buildMessages(turns).reduce((n, m) => n + m.content.length, 0),
    [turns]
  );

  const title = useMemo(() => {
    if (sessionTitle) return sessionTitle;
    const first = turns.find((t) => t.role === "user");
    return first ? first.text.slice(0, 45).replace(/\s+$/, "") : "Aster";
  }, [turns, sessionTitle]);

  return (
    <div className="app">
      <Toolbar
        title={title}
        onNewChat={() => post({ type: "runCommand", command: "aster.newConversation" })}
        onHistory={() => {
          post({ type: "listSessions" });
          setShowHistory(true);
        }}
      />
      {turns.length === 0 ? (
        <EmptyState
          repoName={init?.repoName ?? null}
          branch={init?.branch ?? null}
          binaryOk={init?.binaryOk ?? true}
          setup={init?.setup ?? null}
          login={login}
        />
      ) : (
        <Thread
          turns={turns}
          queued={queued}
          login={login}
          onApproval={answerApproval}
          onRedirect={redirectApproval}
          onAnswer={(choice) => {
            post({ type: "answer", choice });
            if (activeRef.current) {
              patchAssistant(activeRef.current, (turn) => ({ ...turn, question: undefined }));
            }
          }}
          onUnqueue={(index) => setQueued((prev) => prev.filter((_, i) => i !== index))}
          editing={editing}
          onEditStart={(id) => setEditing(id)}
          onEditCancel={() => setEditing(null)}
          onEditSend={resendFrom}
          onFork={forkFrom}
          onRewind={rewindTo}
        />
      )}
      <Composer
        busy={busy}
        model={init?.model ?? null}
        models={init?.models ?? []}
        recommended={init?.recommended ?? []}
        recent={recentsFor(init?.recent ?? [], catalog)}
        modelsLoading={modelsLoading}
        modelsError={modelsError}
        onRefreshModels={refreshModels}
        permissionMode={permissionMode}
        effort={effort}
        contextUsed={contextUsed}
        contextBudget={effectiveBudget ?? init?.contextBudget ?? 0}
        skills={init?.skills ?? []}
        mcpServers={mcpServers}
        providers={providers}
        fileResults={fileResults}
        openMenu={openMenu}
        onMenuOpened={closeMenuRequest}
        insertText={pendingMention?.text ?? null}
        insertMentions={pendingMention?.mentions}
        onInserted={() => setPendingMention(null)}
        onSearchFiles={onSearchFiles}
        onSend={onSend}
        onCommand={onCommand}
        onCancel={onCancel}
        onReview={() => startReview({ kind: "working" })}
        onPermissionMode={choosePermissionMode}
        onEffort={(next) => {
          setEffort(next);
          post({ type: "setEffort", effort: next });
        }}
        onProvider={(provider) =>
          post({ type: "setProvider", baseUrl: provider.base_url, model: provider.example_model })
        }
        onToggleMcp={(name, disabled) => post({ type: "toggleMcp", name, disabled })}
        onModel={(model) => {
          setInit((prev) =>
            prev
              ? {
                  ...prev,
                  model,
                  // A hand-typed id joins the list here and is persisted by the
                  // host, so it survives the next reload too.
                  models:
                    model && !prev.models.includes(model)
                      ? [...prev.models, model]
                      : prev.models,
                  // The host persists this too; kept live here so the picker's
                  // Recent section moves without a reload.
                  recent: model
                    ? [model, ...prev.recent.filter((m) => m !== model)].slice(0, 5)
                    : prev.recent,
                }
              : prev
          );
          markModelChange(model);
          post({ type: "setModel", model });
        }}
      />
      {info && <InfoModal card={info} onClose={() => setInfo(null)} />}
      {!inEditor && <FilePreview />}
      {!nativeFind && <FindBar />}

      {showHistory && (
        <HistoryPanel
          sessions={sessions}
          activeId={session}
          onPick={(id) => post({ type: "loadSession", id })}
          onRename={(id, title) => post({ type: "renameSession", id, title })}
          onDelete={(id) => post({ type: "deleteSession", id })}
          onClose={() => setShowHistory(false)}
        />
      )}
    </div>
  );
}

function sessionRows(turns: Turn[]): InfoRow[] {
  const messages = buildMessages(turns, Infinity);
  const chars = messages.reduce((n, m) => n + m.content.length, 0);
  const rows: InfoRow[] = [
    {
      label: "session",
      // Same units as the CLI's context row, which this sits under.
      value: `${messages.length} messages · ${chars >= 1000 ? `${Math.round(chars / 1000)}k` : chars} chars`,
    },
  ];

  const usage = [...turns]
    .reverse()
    .find((t): t is AssistantTurn => t.role === "assistant" && Boolean(t.usage))?.usage;
  if (usage) {
    const approx = usage.estimated ? "~" : "";
    const cost = usage.estimated_cost_usd;
    rows.push({
      label: "last turn",
      value: `${approx}${usage.total_tokens} tokens${
        cost != null ? ` · ${approx}$${cost.toFixed(4)}` : ""
      }`,
    });
  }
  return rows;
}

function infoTitle(topic: "status" | "memory" | "diff" | "mom"): string {
  return topic === "diff"
    ? "Uncommitted changes"
    : topic === "memory"
      ? "Memory"
      : topic === "mom"
        ? "Model policy"
        : "Status";
}

const HELP_BODY = `| Command | What it does |
| --- | --- |
| /new | Start a new conversation |
| /clear | Clear this conversation |
| /goal <condition> | Keep going until a judge says it is met |
| /compact | Fold earlier turns down to free context |
| /resume | Reopen a saved session |
| /model | Switch the active model |
| /provider | Switch the endpoint |
| /effort | Set the reasoning budget |
| /mode | Choose what the agent may do |
| /review | Review the working tree |
| /review-pr | Review a GitHub PR |
| /diff | Show uncommitted changes |
| /status | Show model and token usage |
| /memory | List what Aster remembers |
| /mom | Show the mom.yaml model policy |
| /mcp | Turn MCP servers on and off |
| /mention | Attach a file to the next message |`;

type ReviewEvent = Extract<ToWebview, { type: "reviewEvent" }>["event"];

function applyReviewEvent(data: ReviewData, event: ReviewEvent): ReviewData {
  switch (event.type) {
    case "phase":
      return { ...data, phase: event.name };
    case "hypothesized":
      return { ...data, candidates: event.count };
    case "verifying":
      return {
        ...data,
        verify: { index: event.index, total: event.total, title: event.title },
      };
    case "diff":
      return { ...data, files: parseDiffFiles(event.content) };
    case "finding": {
      const { type: _type, ...finding } = event;
      return { ...data, findings: [...data.findings, finding] };
    }
    case "refuted":
      return {
        ...data,
        refuted: [...data.refuted, { title: event.title, reason: event.reason }],
      };
    case "done":
      return { ...data, summary: event.summary, usage: event.usage };
    default:
      return data;
  }
}
