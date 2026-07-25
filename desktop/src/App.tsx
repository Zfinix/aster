import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  applyFix,
  authStatus,
  chat,
  pickDiff,
  pickRepo,
  runReview,
  saveProvider,
  startupInfo,
  type AuthStatus,
  type ChatMessage,
} from "./lib/aster";
import { parseUnifiedDiff } from "./lib/diff";
import type { Finding, ReviewOpts, SourceKind, StreamEvent } from "./lib/types";
import { findingKey } from "./lib/match";
import {
  DEFAULT_MODEL,
  RESEARCH_MODELS,
  SOURCE_DEFAULT_VALUE,
  emptyReview,
  latestReview,
  nextId,
  repoNameOf,
  type Conversation,
  type ReviewData,
  type Turn,
} from "./lib/session";
import { severityOf } from "./lib/severity";
import { Dropdown, ToastProvider, useTheme, useToast } from "./components/chrome";
import { AppSidebar } from "./components/AppSidebar";
import { DiffPanel } from "./components/DiffPanel";
import { HomeView } from "./views/HomeView";
import { ThreadView } from "./views/ThreadView";
import type { ComposerBinding } from "./components/Composer";
import { Composer } from "./components/Composer";
import { DotsIcon, SidebarIcon } from "./components/icons";
import { Mark } from "./components/Mark";

type View = "home" | "thread";

const INSTALL_CMD = "cargo install --path crates/aster-cli";

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
    <div className="install-scrim">
      <div className="install-card" role="alertdialog" aria-label="Aster CLI required">
        <Mark px={2} />
        <h2 className="install-title">Aster CLI not found</h2>
        <p className="install-body">
          The desktop app runs reviews through the <code>aster</code> command-line
          binary. Install it, then relaunch Aster.
        </p>
        <button className="install-cmd" onClick={copy} title="Copy to clipboard">
          <span className="install-prompt-glyph">$</span>
          <code>{INSTALL_CMD}</code>
          <span className="install-copy">Copy</span>
        </button>
        <p className="install-hint">
          Already installed elsewhere? Set <code>ASTER_BIN</code> to its path.
        </p>
      </div>
    </div>
  );
}

/** A review turn's contribution to the chat context: the actual findings, so
 *  the agent can answer follow-ups about them ("why is finding 2 critical?"). */
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
  const [customModels, setCustomModels] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem("aster.customModels") || "[]");
    } catch {
      return [];
    }
  });
  const models = [
    ...RESEARCH_MODELS,
    ...customModels.filter((m) => !RESEARCH_MODELS.includes(m)),
  ];
  const [prompt, setPrompt] = useState("");
  const [view, setView] = useState<View>("home");
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const [reviewing, setReviewing] = useState(false);
  const [binPath, setBinPath] = useState<string | null>(null);
  const [repos, setRepos] = useState<{ value: string; label: string }[]>([]);
  const [auth, setAuth] = useState<AuthStatus | null>(null);

  const unlistenRef = useRef<UnlistenFn | null>(null);
  const activeReviewRef = useRef<{ convoId: string; turnId: string } | null>(null);

  useEffect(() => {
    document.body.dataset.state = view;
  }, [view]);

  const refreshAuth = useCallback(() => {
    authStatus().then(setAuth).catch(() => setAuth(null));
  }, []);

  useEffect(() => {
    startupInfo().then((info) => {
      setBinPath(info.binPath);
      if (info.defaultRepo) {
        setOpts({ repoPath: info.defaultRepo });
        addRepo(info.defaultRepo);
      }
    });
    refreshAuth();
    return () => unlistenRef.current?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const addRepo = (path: string) =>
    setRepos((rs) =>
      rs.some((r) => r.value === path)
        ? rs
        : [...rs, { value: path, label: repoNameOf(path) }],
    );

  const onSaveProvider = useCallback(
    async (fields: { apiKey?: string; baseUrl?: string | null; model?: string | null }) => {
      await saveProvider(fields);
      refreshAuth();
    },
    [refreshAuth],
  );

  /* ---- conversation helpers ---- */

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

  /* ---- chat ---- */

  const onAsk = useCallback(async () => {
    const text = prompt.trim();
    if (!text || busy) return;

    const userTurn: Turn = { id: nextId(), role: "user", text };
    const asstId = nextId();
    const asstTurn: Turn = { id: asstId, role: "assistant", text: "", pending: true };

    const isNew = activeId == null;
    const convoId = activeId ?? nextId();
    const priorTurns = isNew
      ? []
      : (conversations.find((c) => c.id === convoId)?.turns ?? []);

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
            c.id === convoId ? { ...c, turns: [...c.turns, userTurn, asstTurn] } : c,
          ),
    );
    setActiveId(convoId);
    setView("thread");
    setPrompt("");
    setBusy(true);

    const history = buildMessages(priorTurns);
    history.push({ role: "user", content: text });
    try {
      const res = await chat(history, opts.repoPath || null, model, true);
      patchTurn(convoId, asstId, { text: res.reply, pending: false });
      if (res.edits.length > 0) {
        toast(`Edited ${res.edits.join(", ")}`);
      }
    } catch (e) {
      patchTurn(convoId, asstId, { text: String(e), pending: false, error: true });
    } finally {
      setBusy(false);
    }
  }, [prompt, busy, activeId, conversations, opts.repoPath, model, patchTurn, toast]);

  /* ---- review ---- */

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
    async (reviewOpts: ReviewOpts) => {
      if (!reviewOpts.repoPath || busy) return;
      unlistenRef.current?.();

      const text = prompt.trim();
      const isNew = activeId == null;
      const convoId = activeId ?? nextId();
      const reviewId = nextId();

      const newTurns: Turn[] = [];
      if (text) newTurns.push({ id: nextId(), role: "user", text });
      newTurns.push({ id: reviewId, role: "review", data: emptyReview() });

      setConversations((cs) =>
        isNew
          ? [
              {
                id: convoId,
                title: text || `Review ${repoNameOf(reviewOpts.repoPath)}`,
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
      setPrompt("");
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
    [busy, activeId, prompt, model, handleEvent, finalizeReview],
  );

  const onReview = useCallback(() => startReview(opts), [startReview, opts]);

  /* ---- repo / source / model pickers ---- */

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

  const onModel = useCallback((value: string) => {
    localStorage.setItem("aster.model", value);
    setModel(value);
  }, []);

  const onAddModel = useCallback(
    (value: string) => {
      setCustomModels((ms) => {
        const next = ms.includes(value) ? ms : [...ms, value];
        localStorage.setItem("aster.customModels", JSON.stringify(next));
        return next;
      });
      onModel(value);
    },
    [onModel],
  );

  /* ---- derived ---- */

  const activeConvo = conversations.find((c) => c.id === activeId) ?? null;
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

  const onFixBrief = useCallback(async () => {
    if (!activeConvo) return;
    const review = latestReview(activeConvo);
    if (!review || review.findings.length === 0) {
      toast("No findings to brief");
      return;
    }
    const md = [
      `# Fix brief · ${activeConvo.repoName}`,
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
  }, [activeConvo, toast]);

  const composer: ComposerBinding = useMemo(
    () => ({
      prompt,
      setPrompt,
      onAsk,
      onReview,
      busy,
      reviewing,
      canReview: !!opts.repoPath,
      opts,
      repoName: repoNameOf(opts.repoPath),
      repoOptions: repos,
      onRepo,
      onSource,
      model,
      models,
      onModel,
      onAddModel,
      onAttach,
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [prompt, onAsk, onReview, busy, reviewing, opts, repos, onRepo, onSource, model, customModels, onModel, onAddModel, onAttach],
  );

  return (
    <div className={`shell ${sidebarOpen ? "" : "side-collapsed"}`}>
      <AppSidebar
        conversations={conversations}
        activeId={activeId}
        onNewReview={() => {
          setActiveId(null);
          setView("home");
          setPrompt("");
        }}
        onOpen={(id) => {
          setActiveId(id);
          setView("thread");
        }}
        onCollapse={toggleSidebar}
        theme={theme}
        setTheme={setTheme}
        minConfidence={opts.minConfidence}
        onSetConfidence={onSetConfidence}
        model={model}
        models={models}
        onModel={onModel}
        onAddModel={onAddModel}
        analyzers={opts.analyzers}
        onToggleAnalyzer={onToggleAnalyzer}
        auth={auth}
        onSaveProvider={onSaveProvider}
      />

      <div className="center">
        {view === "thread" && activeConvo ? (
          <header className="c-head" data-tauri-drag-region>
            {!sidebarOpen && (
              <button
                className="ghost-icon side-reopen"
                aria-label="Expand sidebar"
                title="Expand sidebar"
                onClick={toggleSidebar}
              >
                <SidebarIcon />
              </button>
            )}
            <span className="c-title" data-tauri-drag-region>{activeConvo.title}</span>
            <span className="c-sub" data-tauri-drag-region>
              {activeConvo.repoName} · {activeConvo.whenLabel}
            </span>
            <Dropdown
              trigger={() => (
                <span className="ghost-icon" aria-label="Thread options">
                  <DotsIcon />
                </span>
              )}
              options={[{ value: "delete", label: "Delete conversation" }]}
              onSelect={(v) => v === "delete" && onDeleteThread()}
              direction="down"
            />
            <span className="grow" data-tauri-drag-region />
            {activeFiles.length > 0 && !showDiff && (
              <button className="btn btn-ghost" onClick={() => setShowDiff(true)}>
                Show diff
              </button>
            )}
          </header>
        ) : (
          <header className="c-head" data-tauri-drag-region>
            {!sidebarOpen && (
              <button
                className="ghost-icon side-reopen"
                aria-label="Expand sidebar"
                title="Expand sidebar"
                onClick={toggleSidebar}
              >
                <SidebarIcon />
              </button>
            )}
            <span className="grow" data-tauri-drag-region />
          </header>
        )}

        <div className="c-body">
          {view === "home" && (
            <HomeView
              composer={composer}
              conversations={conversations}
              onOpen={(id) => {
                setActiveId(id);
                setView("thread");
              }}
            />
          )}
          {view === "thread" && activeConvo && (
            <ThreadView
              conversation={activeConvo}
              onOpenDiff={() => setShowDiff((s) => !s)}
              onFocusFinding={onFocusFinding}
              onRetry={onReview}
            />
          )}
        </div>

        {view === "thread" && (
          <div className="c-foot">
            <Composer variant="foot" {...composer} />
          </div>
        )}
      </div>

      {view === "thread" && showDiff && activeFiles.length > 0 && (
        <DiffPanel
          files={activeFiles}
          findings={activeFindings}
          focus={focusFinding}
          onReverify={onReview}
          onApplyFix={onApplyFix}
          onFixBrief={onFixBrief}
          onClose={() => setShowDiff(false)}
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
