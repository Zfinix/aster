import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  applyFix,
  answerApproval,
  authStatus,
  cancelChat,
  deleteSession,
  listModels,
  listProviders,
  listSessions,
  persistModel,
  pickDiff,
  pickRepo,
  runChat,
  runReview,
  saveProvider,
  showSession,
  startupInfo,
  useProvider,
  type AuthStatus,
  type ChatMessage,
} from "./lib/aster";
import { parseUnifiedDiff } from "./lib/diff";
import type {
  ChatStreamEvent,
  Effort,
  Finding,
  PermissionMode,
  Provider,
  ReviewOpts,
  SourceKind,
  StreamEvent,
} from "./lib/types";
import { findingKey } from "./lib/match";
import {
  DEFAULT_MODEL,
  RESEARCH_MODELS,
  SOURCE_DEFAULT_VALUE,
  defaultReviewTitle,
  emptyReview,
  latestReview,
  nextId,
  repoNameOf,
  stepLabel,
  timeAgo,
  turnsFromEvents,
  type Conversation,
  type ReviewData,
  type Turn,
  upsertAgent,
} from "./lib/session";
import { branchAtTurn } from "./lib/turn-history";
import { severityOf } from "./lib/severity";
import { ToastProvider, useTheme, useToast } from "./components/chrome";
import { Dropdown } from "./components/Dropdown";
import { Modal } from "./components/Modal";
import { SettingsPage } from "./components/SettingsPage";
import { Toolbar } from "./components/Toolbar";
import { AppSidebar } from "./components/AppSidebar";
import { DiffPanel } from "./components/DiffPanel";
import { HomeView } from "./views/HomeView";
import { ThreadView } from "./views/ThreadView";
import type { ComposerBinding } from "./components/Composer";
import { Composer } from "./components/Composer";
import { DiffIcon, MoreIcon } from "./components/icons";
import { Mark } from "./components/Mark";

type View = "home" | "thread";

const INSTALL_CMD = "cargo install --path crates/aster-cli";

const RESTORED_SESSIONS = 20;

function InstallPrompt() {
  const toast = useToast();
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(INSTALL_CMD);
      toast("Command copied");
    } catch {
      toast("Copy failed");
    }
  };
  return (
    <Modal label="Aster CLI required" className="setup-card" align="center">
      <Mark px={2} />
      <h2 className="setup-card-title">Aster CLI not found</h2>
      <p className="setup-card-body">
        The desktop app runs on the <code>aster</code> command-line binary. Install it, then relaunch Aster.
      </p>
      <button type="button" className="setup-card-cmd" onClick={copy} title="Copy to clipboard">
        <code>{INSTALL_CMD}</code>
        <span>Copy</span>
      </button>
      <p className="setup-card-hint">
        Already installed elsewhere? Set <code>ASTER_BIN</code> to its path.
      </p>
    </Modal>
  );
}

function reviewContext(d: ReviewData): string {
  if (d.status === "error")
    return `A code review was attempted but failed: ${d.errorMsg ?? "unknown error"}.`;
  if (d.status === "running") return "A code review is currently running.";
  if (d.findings.length === 0)
    return "I ran a code review and found no issues; the diff is clean.";

  const lines = d.findings.slice(0, 25).map((f, i) => {
    const sev = String(f.severity).toUpperCase();
    const desc = f.description ? ` ${f.description.replace(/\s+/g, " ").slice(0, 240)}` : "";
    const conf = f.confidence != null ? ` [confidence ${f.confidence.toFixed(2)}]` : "";
    const fix = f.suggestion ? ` Fix: ${f.suggestion.replace(/\s+/g, " ").slice(0, 160)}` : "";
    return `${i + 1}. ${sev} — ${f.title} (${f.file_path}:${f.line})${conf}:${desc}${fix}`;
  });
  const refuted = d.refuted.length
    ? `\nRefuted by the verifier (not real): ${d.refuted.map((r) => r.title).join("; ")}.`
    : "";
  const summary = d.summary ? `${d.summary}\n` : "";
  return `Here are the findings from the code review I ran (${d.findings.length} total):\n${summary}${lines.join("\n")}${refuted}`;
}

function buildMessages(turns: Turn[]): ChatMessage[] {
  const msgs: ChatMessage[] = [];
  for (const t of turns) {
    if (t.role === "user") msgs.push({ role: "user", content: t.text });
    else if (t.role === "assistant" && t.text && !t.pending && !t.error)
      msgs.push({ role: "assistant", content: t.text });
    else if (t.role === "review")
      msgs.push({ role: "assistant", content: reviewContext(t.data) });
  }
  const recent = msgs.slice(-12);

  // Keep the most recent review's findings in context even if it fell outside
  // the trimmed window, so the agent can always discuss the latest review.
  for (let i = turns.length - 1; i >= 0; i--) {
    const t = turns[i];
    if (t.role !== "review") continue;
    const ctx = reviewContext(t.data);
    if (!recent.some((m) => m.content === ctx)) {
      recent.unshift({ role: "assistant", content: ctx });
    }
    break;
  }
  return recent;
}

function applyEvent(d: ReviewData, ev: StreamEvent): ReviewData {
  switch (ev.type) {
    case "diff":
      return {
        ...d,
        files: parseUnifiedDiff(ev.content),
        baseBranch: ev.base_branch || d.baseBranch,
      };
    case "phase":
      return { ...d, phase: ev.name };
    case "hypothesized":
      return { ...d, phase: `${ev.count} candidate${ev.count === 1 ? "" : "s"} to verify` };
    case "verifying":
      return { ...d, phase: `Verifying ${ev.index}/${ev.total} · ${ev.title}` };
    case "finding": {
      const { type: _t, ...finding } = ev;
      return { ...d, findings: [...d.findings, finding as Finding] };
    }
    case "refuted":
      return { ...d, refuted: [...d.refuted, { title: ev.title, reason: ev.reason }] };
    case "done":
      return { ...d, summary: ev.summary, usage: ev.usage };
    default:
      return d;
  }
}

function App() {
  const toast = useToast();
  const [theme, setTheme] = useTheme();

  const [opts, setOptsState] = useState<ReviewOpts>(() => ({
    repoPath: "",
    sourceKind: "working",
    sourceValue: null,
    minConfidence: (() => {
      const v = Number(localStorage.getItem("aster.minConfidence"));
      return Number.isFinite(v) ? v : 0;
    })(),
    noIndex: false,
    model: null,
    apiKey: null,
    analyzers: (() => {
      try {
        return JSON.parse(localStorage.getItem("aster.analyzers") || '["ast-grep"]');
      } catch {
        return ["ast-grep"];
      }
    })(),
  }));
  const setOpts = (patch: Partial<ReviewOpts>) =>
    setOptsState((o) => ({ ...o, ...patch }));

  const onToggleAnalyzer = useCallback((name: string, on: boolean) => {
    setOptsState((o) => {
      const analyzers = on
        ? [...new Set([...o.analyzers, name])]
        : o.analyzers.filter((a) => a !== name);
      localStorage.setItem("aster.analyzers", JSON.stringify(analyzers));
      return { ...o, analyzers };
    });
  }, []);

  const onSetConfidence = useCallback((v: number) => {
    localStorage.setItem("aster.minConfidence", String(v));
    setOptsState((o) => ({ ...o, minConfidence: v }));
  }, []);

  const [model, setModel] = useState(
    () => localStorage.getItem("aster.model") || DEFAULT_MODEL,
  );
  const [permissionMode, setPermissionMode] = useState<PermissionMode>(
    () => {
      const m = localStorage.getItem("aster.permissionMode");
      return m === "plan" || m === "auto" || m === "edit" || m === "yolo" ? m : "manual";
    },
  );
  const onPermissionMode = useCallback((m: PermissionMode) => {
    localStorage.setItem("aster.permissionMode", m);
    setPermissionMode(m);
  }, []);
  const [customModels, setCustomModels] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem("aster.customModels") || "[]");
    } catch {
      return [];
    }
  });
  const [catalog, setCatalog] = useState<string[] | null>(null);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelsError, setModelsError] = useState<string | undefined>();
  const models = [
    ...(catalog ?? RESEARCH_MODELS),
    ...customModels.filter((m) => !(catalog ?? RESEARCH_MODELS).includes(m)),
  ];
  const [recent, setRecent] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem("aster.recentModels") || "[]");
    } catch {
      return [];
    }
  });
  const [effort, setEffort] = useState<Effort | null>(() => {
    const e = localStorage.getItem("aster.effort");
    return e ? (e as Effort) : null;
  });
  const onEffort = useCallback((e: Effort | null) => {
    if (e) localStorage.setItem("aster.effort", e);
    else localStorage.removeItem("aster.effort");
    setEffort(e);
  }, []);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [view, setView] = useState<View>("home");
  const [homeIntent, setHomeIntent] = useState<"review" | "chat">("chat");
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const [reviewing, setReviewing] = useState(false);
  const [binPath, setBinPath] = useState<string | null>(null);
  const [repos, setRepos] = useState<{ value: string; label: string }[]>([]);
  const [auth, setAuth] = useState<AuthStatus | null>(null);

  const unlistenRef = useRef<UnlistenFn | null>(null);
  const chatUnlistenRef = useRef<UnlistenFn | null>(null);
  const activeReviewRef = useRef<{ convoId: string; turnId: string } | null>(null);
  const activeChatRef = useRef<{ convoId: string; turnId: string } | null>(null);
  const [approval, setApproval] = useState<{
    preview: string;
    markdown?: string;
    plan: boolean;
  } | null>(null);

  useEffect(() => {
    document.body.dataset.state = view;
  }, [view]);

  // A turn runs as its own process with `--permission-mode`, so an approved
  // plan has to move the app's mode or the next turn relaunches in `plan` and
  // refuses the plan it just approved. Only `plan` moves: the other modes
  // already edit, and promoting them would narrow yolo.
  const onRespondApproval = useCallback(
    (allow: boolean) => {
      if (allow && approval?.plan && permissionMode === "plan") {
        onPermissionMode("edit");
      }
      setApproval(null);
      answerApproval(allow).catch(() => {});
    },
    [approval, permissionMode, onPermissionMode],
  );

  const refreshAuth = useCallback(() => {
    authStatus().then(setAuth).catch(() => setAuth(null));
  }, []);

  const refreshProviders = useCallback(() => {
    listProviders(opts.repoPath || null)
      .then(setProviders)
      .catch(() => setProviders([]));
  }, [opts.repoPath]);

  // Not every endpoint lists its models, so the failure is shown in the
  // picker, which still takes an id typed by hand.
  const refreshModels = useCallback(() => {
    setModelsLoading(true);
    setModelsError(undefined);
    listModels(opts.repoPath || null)
      .then((list) => setCatalog(list.length ? list : null))
      .catch((e) => {
        setCatalog(null);
        setModelsError(String(e));
      })
      .finally(() => setModelsLoading(false));
  }, [opts.repoPath]);

  // The CLI config is the source of truth for the model; localStorage only
  // seeds the first paint before auth_status answers.
  useEffect(() => {
    if (auth?.model) {
      localStorage.setItem("aster.model", auth.model);
      setModel(auth.model);
    }
  }, [auth?.model]);

  useEffect(() => {
    startupInfo().then((info) => {
      setBinPath(info.binPath);
      if (info.defaultRepo) {
        setOpts({ repoPath: info.defaultRepo });
        addRepo(info.defaultRepo);
        void restoreSessions(info.defaultRepo);
      }
    });
    refreshAuth();
    return () => {
      unlistenRef.current?.();
      chatUnlistenRef.current?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const addRepo = (path: string) =>
    setRepos((rs) =>
      rs.some((r) => r.value === path)
        ? rs
        : [...rs, { value: path, label: repoNameOf(path) }],
    );

  const restoreSessions = useCallback(async (repoPath: string) => {
    let summaries;
    try {
      summaries = await listSessions(repoPath);
    } catch {
      return;
    }
    // Newest first from the CLI. Each one costs its own transcript read, so
    // only the recent stretch is rebuilt; the rest stay reachable by id.
    const recent = summaries.slice(0, RESTORED_SESSIONS);
    const restored = await Promise.all(
      recent.map(async (s): Promise<Conversation | null> => {
        try {
          const { events } = await showSession(s.id, repoPath);
          const turns = turnsFromEvents(events, `${s.id}-`);
          if (turns.length === 0) return null;
          const created = Date.parse(s.created_at);
          return {
            id: s.id,
            title: s.title,
            repoName: repoNameOf(repoPath),
            repoPath,
            whenLabel: Number.isNaN(created) ? "earlier" : timeAgo(created),
            turns,
            // Already on disk, so a reply continues it rather than forking.
            sessionId: s.id,
            // Untitled sessions must stay open to the live `title` event.
            renamed: !!s.title,
          };
        } catch {
          return null;
        }
      }),
    );
    const usable = restored.filter((c): c is Conversation => c !== null);
    if (usable.length === 0) return;
    // Anything started while the reads were in flight wins its id.
    setConversations((cs) => [...cs, ...usable.filter((c) => !cs.some((x) => x.id === c.id))]);
    // Reopen the thread that was active last time this repo was open, falling
    // back to the newest session, so a restart lands back in the conversation
    // (and its scroll position) instead of the empty home pane.
    const lastOpen = localStorage.getItem(`aster.lastSession.${repoPath}`);
    const reopen =
      usable.find((c) => c.id === lastOpen) ?? usable[0];
    localStorage.setItem(`aster.lastSession.${repoPath}`, reopen.id);
    setActiveId(reopen.id);
    setView("thread");
  }, []);

  const onSaveProvider = useCallback(
    async (fields: { apiKey?: string; baseUrl?: string | null; model?: string | null }) => {
      await saveProvider(fields);
      refreshAuth();
    },
    [refreshAuth],
  );

  const patchTurn = useCallback(
    (convoId: string, turnId: string, patch: Partial<Extract<Turn, { role: "assistant" }>>) =>
      setConversations((cs) =>
        cs.map((c) =>
          c.id !== convoId
            ? c
            : {
                ...c,
                turns: c.turns.map((t) =>
                  t.id === turnId && t.role === "assistant" ? { ...t, ...patch } : t,
                ),
              },
        ),
      ),
    [],
  );

  const updateReviewData = useCallback(
    (convoId: string, turnId: string, fn: (d: ReviewData) => ReviewData) =>
      setConversations((cs) =>
        cs.map((c) =>
          c.id !== convoId
            ? c
            : {
                ...c,
                turns: c.turns.map((t) =>
                  t.id === turnId && t.role === "review" ? { ...t, data: fn(t.data) } : t,
                ),
              },
        ),
      ),
    [],
  );

  const handleChatEvent = useCallback(
    (ev: ChatStreamEvent) => {
      const ref = activeChatRef.current;
      if (!ref) return;
      const { convoId, turnId } = ref;
      switch (ev.type) {
        case "token":
        case "text":
          patchTurn(convoId, turnId, {});
          setConversations((cs) =>
            cs.map((c) =>
              c.id !== convoId
                ? c
                : {
                    ...c,
                    turns: c.turns.map((t) =>
                      t.id === turnId && t.role === "assistant"
                        ? { ...t, text: t.text + ev.content }
                        : t,
                    ),
                  },
            ),
          );
          break;
        case "reasoning_delta":
          setConversations((cs) =>
            cs.map((c) =>
              c.id !== convoId
                ? c
                : {
                    ...c,
                    turns: c.turns.map((t) =>
                      t.id === turnId && t.role === "assistant"
                        ? {
                            ...t,
                            reasoning: t.reasoning
                              ? t.reasoningDone
                                ? `${t.reasoning}\n\n${ev.content}`
                                : t.reasoning + ev.content
                              : ev.content,
                            reasoningTokens: ev.tokens,
                            reasoningDone: false,
                          }
                        : t,
                    ),
                  },
            ),
          );
          break;
        case "reasoning_done":
          setConversations((cs) =>
            cs.map((c) =>
              c.id !== convoId
                ? c
                : {
                    ...c,
                    turns: c.turns.map((t) =>
                      t.id === turnId && t.role === "assistant"
                        ? {
                            ...t,
                            reasoningTokens: ev.tokens,
                            reasoningDurationMs: ev.duration_ms,
                            reasoningDone: true,
                          }
                        : t,
                    ),
                  },
            ),
          );
          break;
        case "reasoning":
          setConversations((cs) =>
            cs.map((c) =>
              c.id !== convoId
                ? c
                : {
                    ...c,
                    turns: c.turns.map((t) =>
                      t.id === turnId && t.role === "assistant"
                        ? {
                            ...t,
                            reasoning: t.reasoning
                              ? `${t.reasoning}\n\n${ev.content}`
                              : ev.content,
                            reasoningTokens: ev.tokens,
                            reasoningDurationMs: ev.duration_ms,
                            reasoningDone: true,
                          }
                        : t,
                    ),
                  },
            ),
          );
          break;
        case "tool_call": {
          let args: Record<string, unknown> = {};
          try {
            args = JSON.parse(ev.arguments || "{}");
          } catch {
            args = {};
          }
          const step = { id: ev.id, name: ev.name, label: stepLabel(ev.name, args) };
          setConversations((cs) =>
            cs.map((c) =>
              c.id !== convoId
                ? c
                : {
                    ...c,
                    turns: c.turns.map((t) =>
                      t.id === turnId && t.role === "assistant"
                        ? { ...t, steps: [...(t.steps ?? []), step] }
                        : t,
                    ),
                  },
            ),
          );
          break;
        }
        case "tool_result":
          setConversations((cs) =>
            cs.map((c) =>
              c.id !== convoId
                ? c
                : {
                    ...c,
                    turns: c.turns.map((t) =>
                      t.id === turnId && t.role === "assistant"
                        ? {
                            ...t,
                            steps: (t.steps ?? []).map((s) =>
                              s.id === ev.id ? { ...s, output: ev.result } : s,
                            ),
                          }
                        : t,
                    ),
                  },
            ),
          );
          break;
        case "agent_status":
          setConversations((cs) =>
            cs.map((c) =>
              c.id !== convoId
                ? c
                : {
                    ...c,
                    turns: c.turns.map((t) =>
                      t.id === turnId && t.role === "assistant"
                        ? { ...t, agents: upsertAgent(t.agents, ev) }
                        : t,
                    ),
                  },
            ),
          );
          break;
        case "approval_request":
          setApproval({
            preview: ev.preview,
            markdown: ev.markdown,
            plan: ev.kind === "plan",
          });
          break;
        case "notice":
          toast(ev.message);
          break;
        case "title":
          setConversations((cs) =>
            cs.map((c) =>
              c.id === convoId && !c.renamed ? { ...c, title: ev.title } : c,
            ),
          );
          break;
        case "done": {
          activeChatRef.current = null;
          chatUnlistenRef.current?.();
          chatUnlistenRef.current = null;
          patchTurn(convoId, turnId, {
            text: ev.reply,
            pending: false,
            usage: ev.usage ?? null,
          });
          setBusy(false);
          if (ev.edits.length > 0) {
            toast(`Edited ${ev.edits.join(", ")}`);
          }
          break;
        }
        case "error": {
          activeChatRef.current = null;
          chatUnlistenRef.current?.();
          chatUnlistenRef.current = null;
          patchTurn(convoId, turnId, {
            text: ev.message,
            pending: false,
            error: true,
          });
          setBusy(false);
          break;
        }
      }
    },
    [patchTurn, toast],
  );

  const settleChat = useCallback(
    (error: string | null, stopped: boolean) => {
      const ref = activeChatRef.current;
      if (!ref) return;
      activeChatRef.current = null;
      chatUnlistenRef.current?.();
      chatUnlistenRef.current = null;
      setApproval(null);
      patchTurn(ref.convoId, ref.turnId, {
        pending: false,
        error: !!error,
        stopped,
        ...(error ? { text: error } : {}),
      });
      setBusy(false);
    },
    [patchTurn],
  );

  const onStopChat = useCallback(() => {
    cancelChat().catch(() => {});
  }, []);

  const onAsk = useCallback((raw: string, target?: Conversation) => {
    const text = raw.trim();
    if (!text || busy || activeChatRef.current || activeReviewRef.current) return;

    const userTurn: Turn = { id: nextId(), role: "user", text, ts: Date.now() };
    const asstId = nextId();
    const asstTurn: Turn = { id: asstId, role: "assistant", text: "", pending: true, ts: Date.now() };

    const convo = target ?? conversations.find((c) => c.id === activeId) ?? null;
    const isNew = convo == null;
    const convoId = convo?.id ?? nextId();
    const priorTurns = convo?.turns ?? [];

    setConversations((cs) =>
      isNew
        ? [
            {
              id: convoId,
              title: text.slice(0, 80),
              repoName: repoNameOf(opts.repoPath),
              repoPath: opts.repoPath,
              whenLabel: "now",
              turns: [userTurn, asstTurn],
            },
            ...cs,
          ]
        : cs.map((c) =>
            c.id === convoId ? { ...(target ?? c), turns: [...priorTurns, userTurn, asstTurn] } : c,
          ),
    );
    setActiveId(convoId);
    setView("thread");
    setBusy(true);

    const history = buildMessages(priorTurns);
    history.push({ role: "user", content: text });
    activeChatRef.current = { convoId, turnId: asstId };
    runChat(history, (convo?.repoPath ?? opts.repoPath) || null, model, permissionMode, effort, convo?.sessionId ?? null, {
      onEvent: handleChatEvent,
      onLog: () => {},
      onError: (msg) => settleChat(msg, false),
      onExit: (code, error) => {
        if (error) settleChat(error, false);
        else if (code === null) settleChat(null, true);
        // A clean exit after `done` has nothing left to settle.
      },
    })
      .then((unlisten) => {
        chatUnlistenRef.current?.();
        chatUnlistenRef.current = unlisten;
      })
      .catch((e) => settleChat(String(e), false));
  }, [busy, activeId, conversations, opts.repoPath, model, permissionMode, effort, handleChatEvent, settleChat]);

  const handleEvent = useCallback(
    (ev: StreamEvent) => {
      const ref = activeReviewRef.current;
      if (!ref) return;
      updateReviewData(ref.convoId, ref.turnId, (d) => applyEvent(d, ev));
    },
    [updateReviewData],
  );

  const finalizeReview = useCallback(
    (status: "done" | "error", errorMsg?: string) => {
      const ref = activeReviewRef.current;
      if (!ref) return;
      updateReviewData(ref.convoId, ref.turnId, (d) => ({ ...d, status, errorMsg }));
      activeReviewRef.current = null;
      setBusy(false);
      setReviewing(false);
    },
    [updateReviewData],
  );

  // The review body, parameterized on the opts to run. Callers that just
  // changed a source (e.g. attaching a diff) pass the updated opts directly,
  // since a `setOpts` in the same tick would not be visible here yet.
  const startReview = useCallback(
    async (reviewOpts: ReviewOpts, targetId?: string, focus = "") => {
      if (!reviewOpts.repoPath || busy) return;
      unlistenRef.current?.();

      const text = focus.trim();
      const existingId = targetId ?? activeId;
      const isNew = existingId == null;
      const convoId = existingId ?? nextId();
      const reviewId = nextId();

      const newTurns: Turn[] = [];
      if (text) newTurns.push({ id: nextId(), role: "user", text, ts: Date.now() });
      newTurns.push({ id: reviewId, role: "review", data: emptyReview(), ts: Date.now() });

      setConversations((cs) =>
        isNew
          ? [
              {
                id: convoId,
                title:
                  text ||
                  defaultReviewTitle(reviewOpts.sourceKind, reviewOpts.sourceValue),
                repoName: repoNameOf(reviewOpts.repoPath),
                repoPath: reviewOpts.repoPath,
                whenLabel: "now",
                turns: newTurns,
              },
              ...cs,
            ]
          : cs.map((c) => (c.id === convoId ? { ...c, turns: [...c.turns, ...newTurns] } : c)),
      );
      setActiveId(convoId);
      setView("thread");
      setBusy(true);
      setReviewing(true);
      activeReviewRef.current = { convoId, turnId: reviewId };

      unlistenRef.current = await runReview(
        { ...reviewOpts, model },
        {
          onEvent: handleEvent,
          onLog: () => {},
          onError: (msg) => finalizeReview("error", msg),
          onDone: () => finalizeReview("done"),
        },
      );
    },
    [busy, activeId, model, handleEvent, finalizeReview],
  );

  const onReview = useCallback((focus = "") => startReview(opts, undefined, focus), [startReview, opts]);

  const onRepo = useCallback(async (value: string) => {
    if (value === "__browse__") {
      const picked = await pickRepo();
      if (picked) {
        setOpts({ repoPath: picked });
        addRepo(picked);
      }
      return;
    }
    setOpts({ repoPath: value });
  }, []);

  const onSource = useCallback((kind: SourceKind) => {
    setOpts({ sourceKind: kind, sourceValue: SOURCE_DEFAULT_VALUE[kind] });
  }, []);

  const onAttach = useCallback(async () => {
    const picked = await pickDiff();
    if (!picked) return;
    const next: ReviewOpts = { ...opts, sourceKind: "diff", sourceValue: picked };
    setOpts({ sourceKind: "diff", sourceValue: picked });
    if (next.repoPath && !busy) {
      toast(`Reviewing ${picked.split("/").pop()}`);
      startReview(next);
    } else {
      toast(`Attached ${picked.split("/").pop()} — pick a repo, then Review`);
    }
  }, [opts, busy, toast, startReview]);

  const onModel = useCallback(
    (value: string) => {
      localStorage.setItem("aster.model", value);
      setModel(value);
      // An id typed by hand is kept so the picker lists it next time.
      setCustomModels((ms) => {
        if (ms.includes(value) || RESEARCH_MODELS.includes(value)) return ms;
        const next = [...ms, value];
        localStorage.setItem("aster.customModels", JSON.stringify(next));
        return next;
      });
      setRecent((r) => {
        const next = [value, ...r.filter((m) => m !== value)].slice(0, 5);
        localStorage.setItem("aster.recentModels", JSON.stringify(next));
        return next;
      });
      // Written through the CLI so the terminal and editors agree next turn.
      persistModel(value).then(refreshAuth).catch(() => {});
    },
    [refreshAuth],
  );

  const onProvider = useCallback(
    (provider: Provider) => {
      useProvider(provider.base_url, provider.example_model || null, opts.repoPath || null)
        .then(() => {
          setCatalog(null);
          refreshAuth();
          refreshProviders();
          toast(`Switched to ${provider.name}`);
        })
        .catch((e) => toast(`Could not switch provider: ${e}`));
    },
    [opts.repoPath, refreshAuth, refreshProviders, toast],
  );

  const onCommand = useCallback(
    (id: string) => {
      switch (id) {
        case "new":
          setActiveId(null);
          setView("home");
          setHomeIntent("chat");
          break;
        case "new-review":
          setActiveId(null);
          setView("home");
          setHomeIntent("review");
          setOpts({ sourceKind: "working", sourceValue: null });
          break;
        case "review":
          setHomeIntent("review");
          startReview({ ...opts, sourceKind: "working", sourceValue: null });
          break;
        case "review-range":
          setHomeIntent("review");
          setOpts({ sourceKind: "range", sourceValue: SOURCE_DEFAULT_VALUE.range });
          break;
        case "review-pr":
          setHomeIntent("review");
          setOpts({ sourceKind: "pr", sourceValue: SOURCE_DEFAULT_VALUE.pr });
          break;
        case "diff":
          onAttach();
          break;
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [opts, startReview],
  );

  const activeConvo = conversations.find((c) => c.id === activeId) ?? null;
  const onTurnAction = (turnId: string, action: "edit" | "fork" | "rewind", text?: string) => {
    if (!activeConvo || busy || approval || activeChatRef.current || activeReviewRef.current) return;
    if (action === "edit" && !text?.trim()) return;
    const next = branchAtTurn(activeConvo, turnId, action, nextId());
    if (!next) return;
    if (action === "edit") {
      onAsk(text!, next);
      return;
    }
    setConversations((cs) => action === "fork"
      ? [next, ...cs]
      : cs.map((c) => c.id === next.id ? next : c));
    setActiveId(next.id);
    setView("thread");
  };
  const activeFiles = activeConvo ? (latestReview(activeConvo)?.files ?? []) : [];
  const activeFindings = activeConvo ? (latestReview(activeConvo)?.findings ?? []) : [];
  const [showDiff, setShowDiff] = useState(true);
  const [focusFinding, setFocusFinding] = useState<{
    key: string;
    nonce: number;
  } | null>(null);
  const onFocusFinding = useCallback((finding: Finding) => {
    setShowDiff(true);
    setFocusFinding((prev) => ({
      key: findingKey(finding),
      nonce: (prev?.nonce ?? 0) + 1,
    }));
  }, []);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(
    () => localStorage.getItem("sidebar-collapsed") !== "1",
  );
  const toggleSidebar = useCallback(() => {
    setSidebarOpen((open) => {
      localStorage.setItem("sidebar-collapsed", open ? "1" : "0");
      return !open;
    });
  }, []);

  const onDeleteThread = useCallback(() => {
    if (!activeId) return;
    setConversations((cs) => cs.filter((c) => c.id !== activeId));
    setActiveId(null);
    setView("home");
  }, [activeId]);

  const onSaveThread = useCallback(() => {
    if (!activeConvo) return;
    toast(activeConvo.sessionId
      ? "Session saved — this thread now resumes across restarts"
      : "This thread has no saved backend session");
  }, [activeConvo, toast]);

  const onDeleteSession = useCallback(() => {
    if (!activeConvo?.sessionId) return;
    const { sessionId, repoPath } = activeConvo;
    deleteSession(sessionId, repoPath || null)
      .then(() => {
        setConversations((cs) =>
          cs.map((c) =>
            c.id === activeConvo.id ? { ...c, sessionId: undefined } : c,
          ),
        );
        toast("Session deleted");
      })
      .catch((e) => toast(`Delete failed: ${e}`));
  }, [activeConvo, toast]);

  const onDeleteConvo = useCallback(
    (id: string) => {
      setConversations((cs) => cs.filter((c) => c.id !== id));
      if (activeId === id) {
        setActiveId(null);
        setView("home");
      }
    },
    [activeId],
  );

  const onRenameConvo = useCallback((id: string, title: string) => {
    setConversations((cs) =>
      cs.map((c) => (c.id === id ? { ...c, title, renamed: true } : c)),
    );
  }, []);

  const onOpenSaved = useCallback(
    (id: string) => {
      setActiveId(id);
      setView("thread");
      const convo = conversations.find((c) => c.id === id);
      if (convo?.sessionId) {
        localStorage.setItem(`aster.lastSession.${convo.repoPath}`, id);
      }
    },
    [conversations],
  );

  const onRerunConvo = useCallback(
    (id: string) => {
      const c = conversations.find((x) => x.id === id);
      if (!c || busy) return;
      setActiveId(id);
      setView("thread");
      startReview({ ...opts, repoPath: c.repoPath }, id);
    },
    [conversations, busy, opts, startReview],
  );

  const onApplyFix = useCallback(
    async (finding: Finding) => {
      const res = await applyFix(finding, opts.repoPath || null, model);
      if (res.status === "applied") {
        toast(`Fixed: ${res.title}`);
        return true;
      }
      toast(res.reason ? `Could not fix: ${res.reason}` : `Fix ${res.status}`);
      return false;
    },
    [opts.repoPath, model, toast],
  );

  const copyFixBrief = useCallback(
    async (convo: Conversation) => {
      const review = latestReview(convo);
      if (!review || review.findings.length === 0) {
        toast("No findings to brief");
        return;
      }
      const md = [
        `# Fix brief · ${convo.repoName}`,
        "",
        ...review.findings.map((f, i) =>
          [
            `## ${i + 1}. [${severityOf(f.severity).toUpperCase()}] ${f.title}`,
            `**Where:** \`${f.file_path}:${f.line}\``,
            "",
            f.description,
            "",
            f.suggestion ? `**Fix:** ${f.suggestion}` : "",
          ].join("\n"),
        ),
      ].join("\n");
      await navigator.clipboard.writeText(md);
      toast(`Fix brief for ${review.findings.length} findings copied`);
    },
    [toast],
  );

  const onFixBrief = useCallback(() => {
    if (activeConvo) copyFixBrief(activeConvo);
  }, [activeConvo, copyFixBrief]);

  const onCopyBriefConvo = useCallback(
    (id: string) => {
      const c = conversations.find((x) => x.id === id);
      if (c) copyFixBrief(c);
    },
    [conversations, copyFixBrief],
  );

  const composer: ComposerBinding = useMemo(
    () => ({
      busy,
      intent: homeIntent,
      onIntent: setHomeIntent,
      opts,
      repoName: repoNameOf(opts.repoPath),
      repoOptions: repos,
      onRepo,
      onSource,
      onAttach,
      canReview: !!opts.repoPath,
      reviewing,
      model,
      models,
      recommended: RESEARCH_MODELS,
      recent,
      modelsLoading,
      modelsError,
      onRefreshModels: refreshModels,
      permissionMode,
      effort,
      providers,
      onRefreshProviders: refreshProviders,
      onSend: onAsk,
      onReview,
      onCommand,
      onCancel: onStopChat,
      onPermissionMode,
      onModel,
      onEffort,
      onProvider,
      onOpenSettings: () => setSettingsOpen(true),
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [busy, homeIntent, opts, repos, onRepo, onSource, onAttach, reviewing, model, customModels, catalog, recent, modelsLoading, modelsError, refreshModels, permissionMode, effort, providers, refreshProviders, onAsk, onReview, onCommand, onStopChat, onPermissionMode, onModel, onEffort, onProvider],
  );

  const threadOptions = activeConvo
    ? [
        ...(activeConvo.sessionId
          ? [{ value: "delete-session", label: "Delete saved session", danger: true }]
          : [{ value: "save", label: "Save as session" }]),
        { value: "delete", label: "Delete conversation", danger: true },
      ]
    : [];

  return (
    <div className="shell" data-sidebar={sidebarOpen ? "open" : "closed"}>
      <AppSidebar
        conversations={conversations}
        activeId={activeId}
        repoPath={opts.repoPath}
        onNewChat={() => {
          setActiveId(null);
          setView("home");
          setHomeIntent("chat");
          setOpts({ sourceKind: "working", sourceValue: null });
        }}
        onNewReview={() => {
          setActiveId(null);
          setView("home");
          setHomeIntent("review");
          setOpts({ sourceKind: "working", sourceValue: null });
        }}
        onOpen={onOpenSaved}
        onRename={onRenameConvo}
        onDelete={onDeleteConvo}
        onRerun={onRerunConvo}
        onCopyBrief={onCopyBriefConvo}
        onCollapse={toggleSidebar}
        onOpenSettings={() => setSettingsOpen(true)}
      />

      <div className="main">
        {view === "thread" && activeConvo ? (
          <Toolbar
            inset={!sidebarOpen}
            onExpand={toggleSidebar}
            title={activeConvo.title}
            sub={`${activeConvo.repoName} · ${activeConvo.whenLabel}`}
          >
            {activeFiles.length > 0 && !showDiff && (
              <button type="button" className="ghost" onClick={() => setShowDiff(true)}>
                <DiffIcon />
                Show diff
              </button>
            )}
            <Dropdown
              triggerClass="ghost icon-action"
              label="Conversation options"
              trigger={() => <MoreIcon />}
              options={threadOptions}
              dir="down"
              align="right"
              onSelect={(v) => {
                if (v === "delete") onDeleteThread();
                else if (v === "save") onSaveThread();
                else if (v === "delete-session") onDeleteSession();
              }}
            />
          </Toolbar>
        ) : (
          <Toolbar inset={!sidebarOpen} onExpand={toggleSidebar} />
        )}

        {view === "home" && (
          <HomeView composer={composer} intent={homeIntent} conversations={conversations} onOpen={onOpenSaved} />
        )}
        {view === "thread" && activeConvo && (
          <>
            <ThreadView
              conversation={activeConvo}
              actionsDisabled={busy || !!approval}
              onEditSend={(id, text) => onTurnAction(id, "edit", text)}
              onFork={(id) => onTurnAction(id, "fork")}
              onRewind={(id) => onTurnAction(id, "rewind")}
              approval={approval}
              onRespondApproval={onRespondApproval}
              onOpenDiff={() => setShowDiff((s) => !s)}
              onFocusFinding={onFocusFinding}
              onApplyFix={onApplyFix}
              onRetry={() => onReview()}
            />
            <Composer variant="foot" {...composer} />
          </>
        )}
      </div>

      {view === "thread" && showDiff && activeFiles.length > 0 && (
        <DiffPanel
          files={activeFiles}
          findings={activeFindings}
          focus={focusFinding}
          onReverify={() => onReview()}
          onApplyFix={onApplyFix}
          onFixBrief={onFixBrief}
          onClose={() => setShowDiff(false)}
        />
      )}

      {settingsOpen && (
        <SettingsPage
          onClose={() => setSettingsOpen(false)}
          theme={theme}
          setTheme={setTheme}
          minConfidence={opts.minConfidence}
          onSetConfidence={onSetConfidence}
          model={model}
          models={models}
          onModel={onModel}
          analyzers={opts.analyzers}
          onToggleAnalyzer={onToggleAnalyzer}
          repoPath={opts.repoPath}
          auth={auth}
          onSaveProvider={onSaveProvider}
        />
      )}

      {!binPath && <InstallPrompt />}
    </div>
  );
}

export default function AppRoot() {
  return (
    <ToastProvider>
      <App />
    </ToastProvider>
  );
}
