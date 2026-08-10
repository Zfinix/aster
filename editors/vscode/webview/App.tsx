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
  SkillCommand,
  ToWebview,
} from "../src/protocol";
import { Composer } from "./components/Composer";
import { EmptyState } from "./components/EmptyState";
import { HistoryPanel } from "./components/HistoryPanel";
import { Thread } from "./components/Thread";
import { Toolbar } from "./components/Toolbar";
import { onHostMessage, persist, post, restore } from "./lib/host";
import {
  appendCall,
  appendInjected,
  appendText,
  buildMessages,
  emptyReview,
  hydrate,
  newTurn,
  patchCall,
  stopUnfinished,
  upsertAgentState,
  type AssistantTurn,
  type ReviewData,
  type Turn,
} from "./lib/thread";


let counter = 0;
const nextId = () => `t${++counter}`;

/** Session ids namespace the CLI transcript; keep them distinct from turn ids. */
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
  binaryOk: boolean;
  skills: SkillCommand[];
}

export function App() {
  const saved = restore<Persisted>();
  const [init, setInit] = useState<Init | null>(null);
  const [permissionMode, setPermissionMode] = useState<PermissionMode>("edit");
  const [effort, setEffort] = useState<Effort | null>(null);
  const [turns, setTurns] = useState<Turn[]>(() => hydrate(saved?.turns));
  const [session, setSession] = useState(saved?.session ?? newSessionId());
  const [busy, setBusy] = useState(false);
  const [fileResults, setFileResults] = useState<string[]>([]);
  const [pendingMention, setPendingMention] = useState<string | null>(null);
  /** Messages typed while a turn was running, flushed in order once it ends. */
  const [queued, setQueued] = useState<string[]>([]);
  /** Saved sessions for the history overlay. */
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [showHistory, setShowHistory] = useState(false);
  /** Backing data for the `/mcp` and `/provider` pickers; each refreshes itself
   *  when it opens, since both read config that changes outside this panel. */
  const [mcpServers, setMcpServers] = useState<McpServer[]>([]);
  const [providers, setProviders] = useState<Provider[]>([]);
  /** Raised when the host asks for the command menu; the composer owns it. */
  const [openMenu, setOpenMenu] = useState(false);
  const closeMenuRequest = useCallback(() => setOpenMenu(false), []);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelsError, setModelsError] = useState<string | undefined>();
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

  useEffect(() => {
    persist({ turns, session });
  }, [turns, session]);

  // The in-flight request id, so a stale event after a cancel is ignored.
  const activeRef = useRef<string | null>(null);
  const fileRequestRef = useRef<string>("");
  // The open approval's kind. A plan approval promotes the CLI's mode for the
  // rest of the turn, and the mode is a launch flag, so the next turn has to
  // start in `edit` or the approved plan is unrunnable again.
  const planApprovalRef = useRef(false);

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
          binaryOk: message.binaryOk,
          skills: message.skills,
        });
        setPermissionMode(message.permissionMode);
        setEffort(message.effort);
        break;

      case "chatEvent":
        if (activeRef.current !== message.id) break;
        applyChatEvent(message.id, message.event);
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
        break;

      case "sessionLoaded":
        activeRef.current = null;
        setBusy(false);
        setQueued([]);
        setSession(message.id);
        setTurns(
          message.turns.map((turn) =>
            turn.role === "user"
              ? { id: nextId(), role: "user", text: turn.content }
              : { ...appendText(newTurn(nextId()), turn.content), pending: false }
          )
        );
        break;

      case "insertMention":
        setPendingMention(message.text);
        break;

      case "openCommandMenu":
        setOpenMenu(true);
        break;

      // The host is the source of truth for whether anything is running, so a
      // surface can never be stuck busy or wrongly re-enable its buttons.
      case "runState": {
        const running = message.review || message.chat;
        setBusy(running);
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
        setInit((prev) =>
          prev
            ? {
                ...prev,
                model: message.model,
                models: message.models.length ? message.models : prev.models,
              }
            : prev
        );
        setProviders((prev) =>
          prev.map((p) => ({ ...p, current: p.name === message.provider }))
        );
        break;

      case "infoCard": {
        const { type: _type, id, ...card } = message;
        setTurns((prev) =>
          prev.map((turn) => {
            if (turn.role !== "info" || turn.id !== id) return turn;
            // The CLI has no view of a conversation it is not running, so the
            // session's own rows are filled in here.
            const rows = card.rows && turn.title === "Status"
              ? [...card.rows, ...sessionRows(prev)]
              : card.rows;
            return { ...turn, ...card, rows, pending: false };
          })
        );
        break;
      }
      // The shorter history replaces the thread outright: the summary comes
      // back as the assistant message it will be sent as, so the next turn
      // carries it the way the CLI's own auto-compaction would.
      case "compacted":
        setQueued([]);
        setTurns([
          ...message.messages.map((turn) =>
            turn.role === "user"
              ? ({ id: nextId(), role: "user", text: turn.content } as Turn)
              : ({ ...appendText(newTurn(nextId()), turn.content), pending: false } as Turn)
          ),
          {
            id: message.id,
            role: "info",
            title: "Compacted",
            note: `Folded ${message.folded} earlier ${
              message.folded === 1 ? "message" : "messages"
            } into the summary above.`,
          },
        ]);
        break;
    }
  }, []);

  const applyChatEvent = (id: string, event: ChatStreamEvent) => {
    switch (event.type) {
      case "token":
        patchAssistant(id, (turn) => ({ ...appendText(turn, event.content), streamed: true }));
        break;
      case "text":
        patchAssistant(id, (turn) => appendText(turn, event.content, "\n\n"));
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
        patchAssistant(id, (turn) => ({ ...turn, approval: event.preview }));
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
      case "done":
        activeRef.current = null;
        setBusy(false);
        patchAssistant(id, (turn) => ({
          // Streaming already built the full answer; appending it again would
          // double it. Without deltas, `reply` is the only copy of the answer.
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
        patchAssistant(id, (turn) => ({
          ...turn,
          errorMsg: event.message,
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

  /** Picked in the composer, or promoted by approving a plan. */
  const choosePermissionMode = (mode: PermissionMode) => {
    setPermissionMode(mode);
    post({ type: "setPermissionMode", mode });
  };

  const answerApproval = (allow: boolean) => {
    post({ type: "approval", allow });
    const plan = planApprovalRef.current;
    planApprovalRef.current = false;
    if (plan && allow) {
      choosePermissionMode("edit");
    }
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

  /** A locally-answered command opens its card straight away, so the thread
   *  shows the ask before the host gets back with the answer. */
  const askInfo = (topic: "status" | "memory" | "diff") => {
    const id = nextId();
    setTurns((prev) => [...prev, { id, role: "info", title: infoTitle(topic), pending: true }]);
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
      case "compact": {
        const id = nextId();
        setTurns((prev) => [...prev, { id, role: "info", title: "Compacting", pending: true }]);
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

  const title = useMemo(() => {
    const first = turns.find((t) => t.role === "user");
    return first ? first.text.slice(0, 45).replace(/\s+$/, "") : "Aster";
  }, [turns]);

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
        />
      ) : (
        <Thread
          turns={turns}
          queued={queued}
          onApproval={answerApproval}
          onAnswer={(choice) => {
            post({ type: "answer", choice });
            if (activeRef.current) {
              patchAssistant(activeRef.current, (turn) => ({ ...turn, question: undefined }));
            }
          }}
          onUnqueue={(index) => setQueued((prev) => prev.filter((_, i) => i !== index))}
        />
      )}
      <Composer
        busy={busy}
        model={init?.model ?? null}
        models={init?.models ?? []}
        recommended={init?.recommended ?? []}
        modelsLoading={modelsLoading}
        modelsError={modelsError}
        onRefreshModels={refreshModels}
        permissionMode={permissionMode}
        effort={effort}
        skills={init?.skills ?? []}
        mcpServers={mcpServers}
        providers={providers}
        fileResults={fileResults}
        openMenu={openMenu}
        onMenuOpened={closeMenuRequest}
        insertText={pendingMention}
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
                }
              : prev
          );
          post({ type: "setModel", model });
        }}
      />
      {showHistory && (
        <HistoryPanel
          sessions={sessions}
          activeId={session}
          onPick={(id) => post({ type: "loadSession", id })}
          onClose={() => setShowHistory(false)}
        />
      )}
    </div>
  );
}

/** The `/status` rows only this panel knows: how much conversation it is
 *  carrying, and what the last turn cost. */
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

function infoTitle(topic: "status" | "memory" | "diff"): string {
  return topic === "diff" ? "Uncommitted changes" : topic === "memory" ? "Memory" : "Status";
}

type ReviewEvent = Extract<ToWebview, { type: "reviewEvent" }>["event"];

function applyReviewEvent(data: ReviewData, event: ReviewEvent): ReviewData {
  switch (event.type) {
    case "phase":
      return { ...data, phase: event.name };
    case "hypothesized":
      return {
        ...data,
        phase: `${event.count} candidate${event.count === 1 ? "" : "s"} to verify`,
      };
    case "verifying":
      return { ...data, phase: `Verifying ${event.index}/${event.total} · ${event.title}` };
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
