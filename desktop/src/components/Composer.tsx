import { useEffect, useMemo, useRef, useState } from "react";
import { Button, TextArea } from "@heroui/react";
import type { PermissionMode, ReviewOpts, SourceKind } from "../lib/types";
import { listRepoFiles } from "../lib/aster";
import { Dropdown } from "./chrome";
import { MentionMenu } from "./MentionMenu";
import {
  ChevronIcon,
  HandIcon,
  PencilIcon,
  RepoIcon,
  ScrollIcon,
  SendIcon,
  ShieldCheckIcon,
  StopIcon,
} from "./icons";
import { ModelMenu } from "./ModelMenu";
import { PlusMenu } from "./PlusMenu";

export type ComposerBinding = Omit<Props, "variant">;

/** Aster's `permissions.mode` values, passed through as --permission-mode. */
const PERMISSION_MODES: {
  value: PermissionMode;
  label: string;
  hint: string;
  icon: React.ReactNode;
}[] = [
  {
    value: "manual",
    label: "Manual",
    hint: "Approve each edit",
    icon: <HandIcon size={17} />,
  },
  {
    value: "edit",
    label: "Edit",
    hint: "Edit files without asking",
    icon: <PencilIcon size={17} />,
  },
  {
    value: "plan",
    label: "Plan",
    hint: "Present a plan before editing",
    icon: <ScrollIcon size={17} />,
  },
  {
    value: "auto",
    label: "Auto",
    hint: "Apply safe edits, pause on risky ones",
    icon: <ShieldCheckIcon size={17} />,
  },
];

/** The @token being typed at the caret, if any. */
function mentionAt(
  text: string,
  caret: number,
): { start: number; query: string } | null {
  const m = /(^|\s)@([^\s@]*)$/.exec(text.slice(0, caret));
  if (!m) return null;
  return { start: caret - m[2].length - 1, query: m[2] };
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
  return scored.slice(0, 8).map((s) => s.path);
}

interface Props {
  variant: "home" | "foot";
  intent?: "review" | "chat";
  prompt: string;
  setPrompt: (s: string) => void;
  onAsk: () => void;
  onStop: () => void;
  onReview: () => void;
  busy: boolean;
  reviewing: boolean;
  chatRunning: boolean;
  canReview: boolean;
  opts: ReviewOpts;
  repoName: string;
  repoOptions: { value: string; label: string; hint?: string }[];
  onRepo: (value: string) => void;
  onSource: (kind: SourceKind) => void;
  model: string;
  models: string[];
  onModel: (value: string) => void;
  onAddModel: (value: string) => void;
  permissionMode: PermissionMode;
  onPermissionMode: (m: PermissionMode) => void;
  onAttach: () => void;
}

export function Composer(props: Props) {
  const {
    variant,
    intent = "chat",
    prompt,
    setPrompt,
    onAsk,
    onStop,
    onReview,
    busy,
    reviewing,
    chatRunning,
    canReview,
    opts,
    repoName,
    repoOptions,
    onRepo,
    onSource,
    model,
    models,
    onModel,
    onAddModel,
    permissionMode,
    onPermissionMode,
    onAttach,
  } = props;

  const ref = useRef<HTMLTextAreaElement>(null);
  const canAsk = !!prompt.trim() && !busy;

  const [mention, setMention] = useState<{ start: number; query: string } | null>(null);
  const [mentionIdx, setMentionIdx] = useState(0);
  const [files, setFiles] = useState<string[]>([]);
  const filesFor = useRef<string | null>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 140)}px`;
  }, [prompt]);

  useEffect(() => {
    const repo = opts.repoPath;
    if (!mention || !repo || filesFor.current === repo) return;
    filesFor.current = repo;
    listRepoFiles(repo)
      .then(setFiles)
      .catch(() => setFiles([]));
  }, [mention, opts.repoPath]);

  const matches = useMemo(
    () => (mention ? rankFiles(files, mention.query) : []),
    [files, mention],
  );

  const onChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setPrompt(e.target.value);
    setMention(mentionAt(e.target.value, e.target.selectionStart ?? e.target.value.length));
    setMentionIdx(0);
  };

  const insertMention = (path: string) => {
    if (!mention) return;
    const end = mention.start + 1 + mention.query.length;
    setPrompt(`${prompt.slice(0, mention.start)}@${path} ${prompt.slice(end)}`);
    setMention(null);
    const caret = mention.start + path.length + 2;
    requestAnimationFrame(() => {
      const el = ref.current;
      if (el) {
        el.focus();
        el.setSelectionRange(caret, caret);
      }
    });
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (mention && matches.length > 0) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const delta = e.key === "ArrowDown" ? 1 : -1;
        setMentionIdx((i) => (i + delta + matches.length) % matches.length);
        return;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault();
        insertMention(matches[mentionIdx]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setMention(null);
        return;
      }
    }
    if (e.key === "Enter" && !e.shiftKey && canAsk) {
      e.preventDefault();
      onAsk();
    }
  };

  const activeMode =
    PERMISSION_MODES.find((m) => m.value === permissionMode) ?? PERMISSION_MODES[0];

  const modePill = (
    <Dropdown
      trigger={() => (
        <span className="pill">
          {activeMode.icon}
          {activeMode.label}
          <ChevronIcon size={11} />
        </span>
      )}
      options={PERMISSION_MODES}
      value={permissionMode}
      onSelect={(v) => onPermissionMode(v as PermissionMode)}
      direction="up"
    />
  );

  const plusMenu = (
    <PlusMenu
      opts={opts}
      repoOptions={repoOptions}
      onRepo={onRepo}
      onSource={onSource}
      onAttach={onAttach}
      direction="up"
    />
  );

  const modelPill = (
    <ModelMenu
      model={model}
      models={models}
      onModel={onModel}
      onAddModel={onAddModel}
      direction="up"
    />
  );

  return (
    <div className={variant === "home" ? "home-composer" : ""}>
      <div className="composer">
        {mention && (
          <MentionMenu
            items={matches}
            activeIndex={mentionIdx}
            onSelect={insertMention}
            onHover={setMentionIdx}
          />
        )}
        <TextArea
          ref={ref}
          value={prompt}
          rows={1}
          spellCheck={false}
          placeholder={
            variant === "foot"
              ? "Ask a follow-up, or give Aster the next task"
              : intent === "review"
                ? "Set a focus and hit Review, or ask anything"
                : "Ask a question, or describe a change to make"
          }
          aria-label="Message Aster"
          onChange={onChange}
          onKeyDown={onKeyDown}
          onBlur={() => setMention(null)}
        />

        {variant === "home" ? (
          <div className="composer-row">
            {plusMenu}
            {modePill}
            {modelPill}
            <span className="spacer" />
            {chatRunning ? (
              <Button className="btn btn-ghost" onPress={onStop}>
                Stop
              </Button>
            ) : (
              <Button
                className={intent === "chat" ? "btn btn-primary" : "btn btn-ghost"}
                isDisabled={!canAsk}
                onPress={onAsk}
              >
                Ask
              </Button>
            )}
            <Button
              className={intent === "chat" ? "btn btn-ghost" : "btn btn-primary"}
              isDisabled={!canReview || busy}
              onPress={onReview}
            >
              {reviewing ? "Reviewing" : "Review"}
            </Button>
          </div>
        ) : (
          <div className="composer-row">
            {plusMenu}
            {modePill}
            {modelPill}
            <span className="spacer" />
            <Button
              className="btn btn-ghost"
              isDisabled={!canReview || busy}
              onPress={onReview}
            >
              {reviewing ? "Reviewing" : "Review"}
            </Button>
            {chatRunning ? (
              <Button className="send stop" aria-label="Stop turn" onPress={onStop}>
                <StopIcon />
              </Button>
            ) : (
              <Button
                className="send"
                aria-label="Send message"
                isDisabled={!canAsk}
                onPress={onAsk}
              >
                <SendIcon />
              </Button>
            )}
          </div>
        )}
      </div>

      {variant === "foot" && (
        <div className="under-row">
          <span className="u-item">
            <RepoIcon />
            {repoName || "no repo"}
          </span>
          <span className="grow" />
          <span className="u-item mono">{opts.repoPath}</span>
          {/* <span className="u-item mono">
            confidence ≥ {opts.minConfidence.toFixed(2)}
          </span> */}
        </div>
      )}
    </div>
  );
}
