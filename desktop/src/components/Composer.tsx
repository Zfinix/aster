import { useEffect, useMemo, useRef, useState, type ReactElement, type ReactNode } from "react";
import type { Effort, PermissionMode, Provider, ReviewOpts, SourceKind } from "../lib/types";
import { SOURCE_LABELS } from "../lib/session";
import { listRepoFiles } from "../lib/aster";
import { applyTrigger, dropTrigger, triggersAt, type Trigger } from "../lib/trigger";
import { useListNav } from "../lib/listnav";
import { EFFORT_OPTIONS, effortShort } from "../lib/effort";
import { modelChip, modelShort } from "../lib/model";
import { ApprovalPicker, permissionIcon, permissionLabel } from "./ApprovalPicker";
import { Autocomplete, type Suggestion } from "./Autocomplete";
import { ChoiceList } from "./ChoiceList";
import { CommandMenu, type MenuItem, type MenuSection } from "./CommandMenu";
import { ModelMenu } from "./ModelMenu";
import { Popover } from "./Popover";
import { IconMorphGlyph, sendStop } from "../interior/icon-morph";
import {
  AtIcon,
  BrainIcon,
  CloudIcon,
  CommandIcon,
  CubeIcon,
  DiffIcon,
  FileIcon,
  FolderIcon,
  GaugeIcon,
  GearIcon,
  GitCommitIcon,
  GitPullRequestIcon,
  NewChatIcon,
  PlusIcon,
  ReviewIcon,
  ShieldIcon,
  TargetIcon,
  UploadIcon,
} from "./icons";

const MAX_ROWS = 10;

type Menu = "none" | "add" | "commands" | "permission" | "settings" | "model" | "provider" | "project" | "source";

const ICONS: Record<string, ReactElement> = {
  new: <NewChatIcon />,
  goal: <TargetIcon />,
  "new-review": <ReviewIcon />,
  mention: <AtIcon />,
  model: <CubeIcon />,
  provider: <CloudIcon />,
  effort: <GaugeIcon />,
  mode: <ShieldIcon />,
  review: <ReviewIcon />,
  settings: <GearIcon />,
  project: <FolderIcon />,
  "review-range": <GitCommitIcon />,
  "review-pr": <GitPullRequestIcon />,
  diff: <DiffIcon />,
  thinking: <BrainIcon />,
};

export type ComposerBinding = Omit<Props, "variant">;

interface Props {
  variant: "home" | "foot";
  busy: boolean;
  intent: "review" | "chat";
  onIntent: (intent: "review" | "chat") => void;
  opts: ReviewOpts;
  repoName: string;
  repoOptions: { value: string; label: string }[];
  onRepo: (value: string) => void;
  onSource: (kind: SourceKind) => void;
  onAttach: () => void;
  canReview: boolean;
  reviewing: boolean;
  model: string;
  models: string[];
  recommended: string[];
  recent: string[];
  modelsLoading: boolean;
  modelsError?: string;
  onRefreshModels: () => void;
  permissionMode: PermissionMode;
  effort: Effort | null;
  providers: Provider[];
  onRefreshProviders: () => void;
  onSend: (text: string) => void;
  onReview: (text: string) => void;
  onCommand: (name: string) => void;
  onCancel: () => void;
  onPermissionMode: (mode: PermissionMode) => void;
  onModel: (model: string) => void;
  onEffort: (effort: Effort | null) => void;
  onProvider: (provider: Provider) => void;
  onOpenSettings: () => void;
}

function rankFiles(files: string[], query: string): string[] {
  const q = query.toLowerCase();
  const scored: { path: string; score: number }[] = [];
  for (const path of files) {
    const p = path.toLowerCase();
    const base = p.slice(p.lastIndexOf("/") + 1);
    let score;
    if (!q) score = 2;
    else if (base.startsWith(q)) score = 0;
    else if (base.includes(q)) score = 1;
    else if (p.includes(q)) score = 2;
    else continue;
    scored.push({ path, score });
  }
  scored.sort((a, b) => a.score - b.score || a.path.length - b.path.length);
  return scored.slice(0, 10).map((s) => s.path);
}

export function Composer({
  variant,
  busy,
  intent,
  onIntent,
  opts,
  repoName,
  repoOptions,
  onRepo,
  onSource,
  onAttach,
  canReview,
  reviewing,
  model,
  models,
  recommended,
  recent,
  modelsLoading,
  modelsError,
  onRefreshModels,
  permissionMode,
  effort,
  providers,
  onRefreshProviders,
  onSend,
  onReview,
  onCommand,
  onCancel,
  onPermissionMode,
  onModel,
  onEffort,
  onProvider,
  onOpenSettings,
}: Props) {
  const [text, setText] = useState("");
  const [caret, setCaret] = useState(0);
  const [menu, setMenu] = useState<Menu>("none");
  const [dismissed, setDismissed] = useState(false);
  const [repoFiles, setRepoFiles] = useState<string[]>([]);
  const filesFor = useRef<string | null>(null);
  const areaRef = useRef<HTMLTextAreaElement>(null);
  const addRef = useRef<HTMLButtonElement>(null);
  const chipRef = useRef<HTMLButtonElement>(null);
  const projectRef = useRef<HTMLButtonElement>(null);
  const sourceRef = useRef<HTMLButtonElement>(null);
  const mirrorRef = useRef<HTMLDivElement>(null);
  const mentions = useRef(new Map<string, string>());
  const lastEffort = useRef<Effort | null>(null);
  if (effort !== "off") lastEffort.current = effort;

  const syncScroll = () => {
    const area = areaRef.current;
    const mirror = mirrorRef.current;
    if (area && mirror) mirror.scrollTop = area.scrollTop;
  };

  // A popup opened from a button owns the composer; otherwise what the caret
  // sits on decides which one is up, the way typing `@` or `/` reads.
  const triggers = triggersAt(text, caret);
  const trigger: Trigger | null = menu === "none" ? triggers.mention : null;
  const command: Trigger | null = menu === "none" && !dismissed ? triggers.command : null;

  useEffect(() => {
    const repo = opts.repoPath;
    if (!trigger || !repo || filesFor.current === repo) return;
    filesFor.current = repo;
    listRepoFiles(repo)
      .then(setRepoFiles)
      .catch(() => setRepoFiles([]));
  }, [trigger, opts.repoPath]);

  const typingCommand = triggers.command !== null;
  useEffect(() => {
    if (!typingCommand) setDismissed(false);
  }, [typingCommand]);

  const suggestions: Suggestion[] = useMemo(
    () =>
      trigger
        ? rankFiles(repoFiles, trigger.query).map((path) => ({
            value: `@${basename(path)}`,
            label: basename(path),
            detail: dirname(path),
            full: path,
          }))
        : [],
    [trigger, repoFiles],
  );

  const files = useListNav<HTMLButtonElement>({
    count: suggestions.length,
    resetOn: trigger?.query,
    tabCompletes: true,
    onPick: (index) => pick(suggestions[index]),
  });

  useEffect(() => {
    const area = areaRef.current;
    if (!area) return;
    area.style.height = "auto";
    const max = parseFloat(getComputedStyle(area).lineHeight) * MAX_ROWS;
    area.style.height = `${Math.min(area.scrollHeight, max)}px`;
    syncScroll();
  }, [text]);

  const sync = (el: HTMLTextAreaElement) => {
    setText(el.value);
    setCaret(el.selectionStart);
  };

  const pick = (item: Suggestion) => {
    if (!trigger) return;
    if (item.full) {
      mentions.current.set(item.value, `@${item.full}`);
    }
    write(applyTrigger(text, trigger, item.value), trigger.start + item.value.length + 1);
  };

  const write = (next: string, at = next.length) => {
    setText(next);
    setCaret(at);
    focusInput(at);
  };

  const send = () => {
    const trimmed = text.trim();
    if (busy) return;
    if (intent === "review") {
      if (!canReview) return;
      onReview(expandMentions(trimmed, mentions.current));
    } else {
      if (!trimmed) return;
      onSend(expandMentions(trimmed, mentions.current));
    }
    setText("");
    setCaret(0);
  };

  const canSend = intent === "review" ? canReview : text.trim().length > 0;

  const focusInput = (at?: number) =>
    requestAnimationFrame(() => {
      const area = areaRef.current;
      if (!area) return;
      area.focus();
      if (at !== undefined) area.setSelectionRange(at, at);
    });

  const toggle = (next: Menu) => (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setMenu((open) => (open === next ? "none" : next));
  };

  const closeMenu = () => {
    setMenu("none");
    setDismissed(true);
    focusInput();
  };

  const compose = (value: string, base = text) => {
    const head = base.replace(/\s*$/, "");
    write(head ? `${head} ${value}` : value);
  };

  const runItem = (item: MenuItem, complete: boolean) => {
    if (item.kind !== "action") return;
    if (!command) {
      item.run(text);
      if (!item.keepOpen) closeMenu();
      return;
    }
    if (item.slash && (complete || item.takesArg || text.slice(0, command.start).trim())) {
      write(applyTrigger(text, command, item.slash), command.start + item.slash.length + 1);
      return;
    }
    const rest = dropTrigger(text, command);
    write(rest, command.start);
    item.run(rest);
  };

  const menuOpen = menu === "commands" || command !== null;
  const sections: MenuSection[] = useMemo(() => {
    if (!menuOpen) return [];
    const provider = providers.find((p) => p.current);
    const action = (id: string, label: string, hint?: string) => ({
      kind: "action" as const,
      id,
      label,
      hint,
      icon: ICONS[id],
      slash: `/${id}`,
      run: () => onCommand(id),
    });
    const opens = (id: string, label: string, next: Menu, hint?: string) => ({
      kind: "action" as const,
      id,
      label,
      hint,
      icon: ICONS[id],
      keepOpen: true,
      run: () => setMenu(next),
    });

    return [
      {
        items: [
          action("new", "New conversation"),
          action("new-review", "New review"),
          {
            ...action("goal", "Set goal…"),
            takesArg: true,
            run: (rest: string) => compose("/goal ", rest),
          },
          {
            kind: "action" as const,
            id: "mention",
            label: "Mention a file…",
            icon: ICONS.mention,
            run: (rest: string) => compose("@", rest),
          },
          opens("project", "Switch project…", "project", repoName || undefined),
          {
            kind: "action" as const,
            id: "settings",
            label: "Settings",
            icon: ICONS.settings,
            run: () => onOpenSettings(),
          },
        ],
      },
      {
        title: "Model",
        items: [
          opens("model", "Switch model…", "model", modelShort(model)),
          opens("provider", "Switch provider…", "provider", provider?.name),
          {
            kind: "choice" as const,
            id: "effort",
            label: "Effort",
            icon: ICONS.effort,
            value: effort ?? "",
            options: EFFORT_OPTIONS,
            onSelect: (value: string) => onEffort((value || null) as Effort | null),
          },
          {
            kind: "toggle" as const,
            id: "thinking",
            label: "Thinking",
            icon: ICONS.thinking,
            on: effort !== "off",
            onToggle: (on: boolean) => onEffort(on ? lastEffort.current : "off"),
          },
          opens("mode", "Mode…", "permission", intent === "review" ? "Review" : permissionLabel(permissionMode)),
        ],
      },
      {
        title: "Repository",
        items: [
          action("review", "Review the working tree"),
          action("review-range", "Review a git range…"),
          action("review-pr", "Review a GitHub PR…"),
          action("diff", "Review a diff file…"),
        ],
      },
    ];
  }, [menuOpen, model, providers, effort, permissionMode, intent, repoName, onCommand, onEffort, onOpenSettings]);

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (command) return;
    if (suggestions.length > 0 && files.onKey(e)) return;
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  };

  const sourceLabel =
    opts.sourceKind === "diff" && opts.sourceValue
      ? opts.sourceValue.split("/").pop()
      : opts.sourceKind === "range" && opts.sourceValue
        ? opts.sourceValue
        : SOURCE_LABELS[opts.sourceKind];

  const popup: ReactNode = menuOpen ? (
    <CommandMenu sections={sections} query={command ? command.query : null} onRun={runItem} />
  ) : menu === "add" ? (
    <div className="picker" role="menu" aria-label="Add a file">
      <button
        className="picker-row"
        role="menuitem"
        onClick={() => {
          closeMenu();
          onAttach();
        }}
      >
        <UploadIcon />
        <span className="picker-body">
          <span className="picker-label">Attach a diff file</span>
          <span className="picker-detail">Review a patch from disk</span>
        </span>
      </button>
      <button
        className="picker-row"
        role="menuitem"
        onClick={() => {
          setMenu("none");
          compose("@");
        }}
      >
        <FileIcon />
        <span className="picker-body">
          <span className="picker-label">Mention a file in this repo</span>
        </span>
      </button>
    </div>
  ) : menu === "permission" ? (
    <ApprovalPicker
      mode={permissionMode}
      reviewing={intent === "review"}
      onSelect={(mode) => {
        onIntent("chat");
        onPermissionMode(mode);
      }}
      onClose={closeMenu}
      onReview={() => onIntent("review")}
    />
  ) : menu === "settings" || menu === "model" || menu === "provider" ? (
    <ModelMenu
      pane={menu === "settings" ? null : menu}
      model={model}
      models={models}
      recommended={recommended}
      recent={recent}
      loading={modelsLoading}
      error={modelsError}
      effort={effort}
      providers={providers}
      onSelect={onModel}
      onRefresh={onRefreshModels}
      onRefreshProviders={onRefreshProviders}
      onEffort={onEffort}
      onProvider={onProvider}
      onClose={closeMenu}
    />
  ) : menu === "project" ? (
    <ChoiceList
      label="Project"
      choices={[
        ...repoOptions.map((r) => ({
          key: r.value,
          label: r.label,
          title: r.value,
          checked: r.value === opts.repoPath,
          onSelect: () => {
            onRepo(r.value);
            closeMenu();
          },
        })),
        {
          key: "__browse__",
          label: "Open a folder…",
          checked: false,
          onSelect: () => {
            onRepo("__browse__");
            closeMenu();
          },
        },
      ]}
    />
  ) : menu === "source" ? (
    <ChoiceList
      label="What to review"
      choices={(Object.keys(SOURCE_LABELS) as SourceKind[]).map((k) => ({
        key: k,
        label: SOURCE_LABELS[k],
        checked: k === opts.sourceKind,
        onSelect: () => {
          if (k === "diff") onAttach();
          else onSource(k);
          closeMenu();
        },
      }))}
    />
  ) : null;

  const anchor =
    menu === "add"
      ? addRef
      : menu === "settings" || menu === "model" || menu === "provider"
        ? chipRef
        : menu === "project"
          ? projectRef
          : menu === "source"
            ? sourceRef
            : undefined;

  const placeholder =
    intent === "review"
      ? reviewing
        ? "Reviewing…"
        : "Add a focus for the review, or press Enter to run it"
      : busy
        ? "Aster is working…"
        : variant === "foot"
          ? "Ask a follow-up, @ for files, / for commands"
          : "Ask Aster, @ for files, / for commands";

  return (
    <div className="composer-wrap" data-variant={variant}>
      {popup ? (
        <Popover onClose={closeMenu} anchor={anchor}>
          {popup}
        </Popover>
      ) : (
        <Autocomplete items={suggestions} active={files.active} onPick={pick} />
      )}

      <div className="composer" data-permission-mode={permissionMode} data-intent={intent}>
        <div className="input-wrap">
          <div className="input-mirror" aria-hidden="true" ref={mirrorRef}>
            {renderMentions(text)}
          </div>
          <textarea
            ref={areaRef}
            className="composer-input"
            rows={1}
            value={text}
            spellCheck={false}
            placeholder={placeholder}
            aria-label="Message Aster"
            onChange={(e) => sync(e.currentTarget)}
            onKeyUp={(e) => setCaret(e.currentTarget.selectionStart)}
            onClick={(e) => setCaret(e.currentTarget.selectionStart)}
            onKeyDown={onKeyDown}
            onScroll={syncScroll}
          />
        </div>
        <div className="composer-foot">
          <button
            ref={addRef}
            className="ghost foot-btn"
            onMouseDown={toggle("add")}
            title="Add a file"
            aria-label="Add a file"
            aria-haspopup="menu"
            aria-expanded={menu === "add"}
          >
            <PlusIcon />
          </button>

          <button
            className="ghost foot-btn"
            onMouseDown={toggle("commands")}
            title="Show command menu (/)"
            aria-label="Show command menu"
            aria-haspopup="dialog"
            aria-expanded={menu === "commands"}
          >
            <CommandIcon />
          </button>

          <button
            ref={chipRef}
            className="ghost model-btn"
            onMouseDown={toggle("settings")}
            title={effort ? `${model} · ${effort} effort` : model}
            aria-haspopup="menu"
            aria-expanded={menu === "settings"}
          >
            <span className="model-label">{modelChip(model)}</span>
            {effort && <span className="model-effort">{effortShort(effort)}</span>}
          </button>

          <button
            ref={projectRef}
            className="ghost mode-btn mode-btn-project"
            onMouseDown={toggle("project")}
            title={opts.repoPath || "Pick a project"}
            aria-haspopup="menu"
            aria-expanded={menu === "project"}
          >
            <FolderIcon />
            <span>{repoName || "Project"}</span>
          </button>

          {intent === "review" && (
            <button
              ref={sourceRef}
              className="ghost mode-btn"
              onMouseDown={toggle("source")}
              title="What to review"
              aria-haspopup="menu"
              aria-expanded={menu === "source"}
            >
              <span>{sourceLabel}</span>
            </button>
          )}

          <span className="grow" />

          <button
            className="ghost mode-btn"
            onMouseDown={toggle("permission")}
            title="Mode"
            aria-expanded={menu === "permission"}
          >
            {intent === "review" ? <ReviewIcon /> : permissionIcon(permissionMode)}
            <span>{intent === "review" ? "Review" : permissionLabel(permissionMode)}</span>
          </button>

          <button
            className={busy ? "send stop" : "send"}
            onClick={busy ? onCancel : send}
            disabled={!busy && !canSend}
            title={busy ? "Stop" : intent === "review" ? "Run the review" : "Send"}
            aria-label={busy ? "Stop" : intent === "review" ? "Run the review" : "Send"}
          >
            <IconMorphGlyph shapes={sendStop} active={busy ? 1 : 0} />
          </button>
        </div>
      </div>
    </div>
  );
}

function basename(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

function dirname(path: string): string | undefined {
  const at = path.lastIndexOf("/");
  return at === -1 ? undefined : path.slice(0, at);
}

function renderMentions(text: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  const pattern = /@[^\s]+/g;
  let cursor = 0;
  let match: RegExpExecArray | null;
  let key = 0;

  while ((match = pattern.exec(text)) !== null) {
    if (match.index > cursor) {
      nodes.push(text.slice(cursor, match.index));
    }
    nodes.push(
      <span key={key++} className="mention">
        {match[0]}
      </span>,
    );
    cursor = match.index + match[0].length;
  }
  nodes.push(`${text.slice(cursor)}​`);
  return nodes;
}

function expandMentions(text: string, mentions: Map<string, string>): string {
  let out = text;
  for (const [shown, full] of mentions) {
    out = out.split(shown).join(full);
  }
  return out;
}
