import { useEffect, useMemo, useRef, useState, type ReactElement, type ReactNode } from "react";
import type {
  Effort,
  McpServer,
  PastedFile,
  PermissionMode,
  Provider,
  SkillCommand,
} from "../../src/protocol";
import { applyTrigger, dropTrigger, triggersAt, type Trigger } from "../lib/trigger";
import { inEditor, onHostMessage, post } from "../lib/host";
import { fileUrisFromTransfer, filesFromTransfer } from "../lib/dataTransfer";
import { useListNav } from "../lib/listnav";
import { EFFORT_OPTIONS, effortShort } from "../lib/effort";
import { modelChip, modelShort } from "../lib/model";
import { ApprovalPicker, permissionIcon, permissionLabel } from "./ApprovalPicker";
import { displayName, fileIconKind, fileUrl, splitMentions } from "./UserText";
import { Autocomplete, type Suggestion } from "./Autocomplete";
import { CommandMenu, type MenuItem, type MenuSection } from "./CommandMenu";
import { ContextMeter } from "./ContextMeter";
import { McpPicker } from "./McpPicker";
import { ModelMenu } from "./ModelMenu";
import { Popover } from "./Popover";
import { QueuedList } from "./QueuedTurn";
import { IconMorphGlyph, sendStop } from "../interior/icon-morph";
import {
  ActivityIcon,
  AtIcon,
  BookIcon,
  BrainIcon,
  CloudIcon,
  CommandIcon,
  CubeIcon,
  DiffIcon,
  FileIcon,
  FileTypeIcon,
  GaugeIcon,
  GearIcon,
  GitCommitIcon,
  GitPullRequestIcon,
  HistoryIcon,
  MinimizeIcon,
  NewChatIcon,
  PlugIcon,
  PlusIcon,
  ReviewIcon,
  ShieldIcon,
  TargetIcon,
  TrashIcon,
  UploadIcon,
  XIcon,
} from "./icons";

const MAX_ROWS = 10;

type Menu = "none" | "add" | "commands" | "permission" | "settings" | "model" | "provider" | "mcp";

const ICONS: Record<string, ReactElement> = {
  new: <NewChatIcon />,
  clear: <TrashIcon />,
  compact: <MinimizeIcon />,
  resume: <HistoryIcon />,
  mention: <AtIcon />,
  model: <CubeIcon />,
  provider: <CloudIcon />,
  effort: <GaugeIcon />,
  mode: <ShieldIcon />,
  review: <ReviewIcon />,
  settings: <GearIcon />,
  "review-range": <GitCommitIcon />,
  "review-pr": <GitPullRequestIcon />,
  diff: <DiffIcon />,
  status: <ActivityIcon />,
  memory: <BrainIcon />,
  thinking: <BrainIcon />,
  mcp: <PlugIcon />,
  skill: <BookIcon />,
};

const SKILLS_SHOWN = 5;

const IMAGE = /\.(png|jpe?g|gif|webp|bmp|svg|avif)$/i;

const MAX_THUMB_BYTES = 4 * 1024 * 1024;

interface Attachment {
  mention: string;
  name: string;
  thumb?: string;
}

export function Composer({
  busy,
  model,
  models,
  recommended,
  recent,
  modelsLoading,
  modelsError,
  onRefreshModels,
  permissionMode,
  effort,
  contextUsed,
  contextBudget,
  skills,
  mcpServers,
  providers,
  fileResults,
  openMenu,
  onMenuOpened,
  insertText,
  insertMentions,
  onInserted,
  onSearchFiles,
  onSend,
  onCommand,
  onCancel,
  onReview,
  onPermissionMode,
  onModel,
  onEffort,
  onProvider,
  onToggleMcp,
  queued,
  onSteerQueued,
  onEditQueued,
  onReorderQueued,
  onUnqueue,
}: {
  busy: boolean;
  model: string | null;
  models: string[];
  recommended: string[];
  recent: string[];
  modelsLoading: boolean;
  modelsError?: string;
  onRefreshModels: () => void;
  permissionMode: PermissionMode;
  effort: Effort | null;
  contextUsed: number;
  contextBudget: number;
  skills: SkillCommand[];
  mcpServers: McpServer[];
  providers: Provider[];
  fileResults: string[];
  openMenu: boolean;
  onMenuOpened: () => void;
  insertText: string | null;
  insertMentions?: string[];
  onInserted: () => void;
  onSearchFiles: (query: string) => void;
  onSend: (text: string) => void;
  onCommand: (name: string) => void;
  onCancel: () => void;
  onReview: () => void;
  onPermissionMode: (mode: PermissionMode) => void;
  onModel: (model: string) => void;
  onEffort: (effort: Effort | null) => void;
  onProvider: (provider: Provider) => void;
  onToggleMcp: (name: string, disabled: boolean) => void;
  queued: { id: string; text: string }[];
  onSteerQueued: (id: string) => void;
  onEditQueued: (id: string, text: string) => void;
  onReorderQueued: (from: number, to: number) => void;
  onUnqueue: (id: string) => void;
}) {
  const [text, setText] = useState("");
  const [caret, setCaret] = useState(0);
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const thumbs = useRef(new Map<string, string>());
  const [menu, setMenu] = useState<Menu>("none");
  const [dismissed, setDismissed] = useState(false);
  const [dropping, setDropping] = useState(false);
  const areaRef = useRef<HTMLTextAreaElement>(null);
  const addRef = useRef<HTMLButtonElement>(null);
  const chipRef = useRef<HTMLButtonElement>(null);
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
  const command: Trigger | null =
    menu === "none" && !dismissed ? triggers.command : null;

  useEffect(() => {
    if (trigger) onSearchFiles(trigger.query);
  }, [trigger?.query]);

  const typingCommand = triggers.command !== null;
  useEffect(() => {
    if (!typingCommand) setDismissed(false);
  }, [typingCommand]);

  useEffect(() => {
    if (openMenu) {
      setMenu("commands");
      onMenuOpened();
    }
  }, [openMenu, onMenuOpened]);

  const suggestions: Suggestion[] = useMemo(
    () =>
      trigger
        ? fileResults.slice(0, 10).map((path) => {
            const dir = path.endsWith("/");
            const trimmed = dir ? path.slice(0, -1) : path;
            const name = dir ? `${basename(trimmed)}/` : basename(trimmed);
            return { value: `@${name}`, label: name, detail: dirname(trimmed), full: path, dir };
          })
        : [],
    [trigger, fileResults]
  );

  const files = useListNav<HTMLButtonElement>({
    count: suggestions.length,
    resetOn: trigger?.query,
    tabCompletes: true,
    onPick: (index) => pick(suggestions[index]),
  });

  const pendingThumbs = useRef(new Set<string>());

  // A dropped image has no bytes in the webview yet: pasted ones arrive with
  // their data, dropped ones are only a path. In a browser the server hands
  // the file over; in the editor the host reads it and answers with a data URI.
  const requestThumb = (mention: string, path: string) => {
    if (pendingThumbs.current.has(mention)) return;
    pendingThumbs.current.add(mention);
    const put = (src: string) => {
      thumbs.current.set(path, src);
      setAttachments((prev) =>
        prev.map((a) => (a.mention === mention && !a.thumb ? { ...a, thumb: src } : a))
      );
    };
    if (!inEditor) {
      put(fileUrl(path));
      return;
    }
    const requestId = `thumb-${Math.random().toString(36).slice(2)}`;
    const off = onHostMessage((message) => {
      if (message.type === "filePreview" && message.requestId === requestId) {
        if (message.file?.image) put(message.file.image);
        off();
      }
    });
    post({ type: "readFile", path, requestId });
  };

  // Images go above the box as chips; everything else lands in the text as a
  // mention, the way it always has.
  useEffect(() => {
    if (!insertText) return;
    const words: string[] = [];
    const pictures: Attachment[] = [];
    const parts = insertMentions?.length
      ? insertMentions.map((mention) => ({ kind: "image" as const, path: mention.slice(1) }))
      : splitMentions(insertText);
    for (const part of parts) {
      if (part.kind === "text") {
        for (const token of part.text.split(" ")) {
          const picture = attachmentFor(token, thumbs.current);
          if (picture) pictures.push(picture);
          else if (token) words.push(token);
        }
      } else {
        const mention = `@${part.path}`;
        const picture = attachmentFor(mention, thumbs.current);
        if (picture) pictures.push(picture);
        else words.push(mention);
      }
    }
    for (const picture of pictures) {
      const path = picture.mention.slice(1);
      if (!picture.thumb && IMAGE.test(path)) {
        requestThumb(picture.mention, path);
      }
    }
    if (pictures.length) {
      setAttachments((prev) => [
        ...prev,
        ...pictures.filter((p) => !prev.some((a) => a.mention === p.mention)),
      ]);
    }
    if (words.length) {
      const joined = words.join(" ");
      setText((prev) => (prev && !prev.endsWith(" ") ? `${prev} ${joined} ` : `${prev}${joined} `));
    }
    onInserted();
    requestAnimationFrame(() => areaRef.current?.focus());
  }, [insertText, insertMentions, onInserted]);

  useEffect(() => {
    const area = areaRef.current;
    if (!area) return;
    area.style.height = "auto";
    const max = parseFloat(getComputedStyle(area).lineHeight) * MAX_ROWS;
    area.style.height = `${Math.min(area.scrollHeight, max)}px`;
    // Typing at the bottom auto-scrolls the textarea without a scroll event.
    syncScroll();
  }, [text]);

  const sync = (el: HTMLTextAreaElement) => {
    setText(el.value);
    setCaret(el.selectionStart);
  };

  const sendPasted = (files: File[]) =>
    void Promise.all(files.map(readPasted)).then((pasted) => {
      for (const file of pasted) {
        if (file.data && file.type.startsWith("image/") && file.size <= MAX_THUMB_BYTES) {
          thumbs.current.set(file.name, `data:${file.type};base64,${file.data}`);
        }
      }
      post({ type: "pasteFiles", files: pasted.map(({ type: _type, ...rest }) => rest) });
    });

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setDropping(false);
    const uris = editorFileUris(e.dataTransfer);
    if (uris.length) {
      post({ type: "dropFiles", uris });
      return;
    }
    // Prefer URIs over bytes: an OS drag carries both, and only the URI has
    // the original path, so it must win or the file gets re-staged to tmp.
    const fileUris = fileUrisFromTransfer(e.dataTransfer);
    if (fileUris.length) {
      post({ type: "dropFiles", uris: fileUris });
      return;
    }
    const files = filesFromTransfer(e.dataTransfer);
    if (files.length) {
      sendPasted(files);
      return;
    }
    // No files at all: dropped text, which preventDefault kept out of the
    // textarea, so it goes in by hand.
    const dropped = e.dataTransfer.getData("text/plain");
    if (dropped) {
      setText((prev) => `${prev}${dropped}`);
      requestAnimationFrame(() => areaRef.current?.focus());
    }
  };

  const onPaste = (e: React.ClipboardEvent) => {
    const uris = editorFileUris(e.clipboardData);
    if (uris.length) {
      e.preventDefault();
      return post({ type: "dropFiles", uris });
    }

    const files = filesFromTransfer(e.clipboardData);
    if (files.length === 0) return;
    e.preventDefault();
    sendPasted(files);
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

  // Sending while busy is allowed: App queues it and flushes when the run ends.
  const send = () => {
    const trimmed = text.trim();
    if (!trimmed && attachments.length === 0) return;
    const pictures = attachments.map((a) => a.mention).join(" ");
    onSend([expandMentions(trimmed, mentions.current), pictures].filter(Boolean).join(" "));
    setText("");
    setCaret(0);
    setAttachments([]);
  };

  const canSend = text.trim().length > 0 || attachments.length > 0;

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

  // Only while the menu is up: mapping every skill on each streamed token
  // would be a real cost for a list nobody is looking at.
  const menuOpen = menu === "commands" || command !== null;
  const sections: MenuSection[] = useMemo(() => {
    if (!menuOpen) return [];
    const enabled = mcpServers.filter((s) => !s.disabled).length;
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
          action("clear", "Clear conversation"),
          {
            kind: "action" as const,
            id: "goal",
            label: "Run to a goal…",
            hint: "Keep going until a judge says it is met",
            icon: <TargetIcon />,
            slash: "/goal",
            // The CLI parses `/goal <condition>` from the last message, so this
            // only has to land in the box and go out as a normal send.
            takesArg: true,
            run: (rest: string) => compose("/goal", rest),
          },
          action("compact", "Compact conversation"),
          action("resume", "Resume a session…"),
          {
            kind: "action" as const,
            id: "mention",
            label: "Mention a file…",
            icon: ICONS.mention,
            run: (rest: string) => compose("@", rest),
          },
          {
            kind: "action" as const,
            id: "settings",
            label: "Settings",
            icon: ICONS.settings,
            run: () => post({ type: "runCommand", command: "aster.openSettings" }),
          },
          action("help", "List commands"),
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
          opens("mode", "Mode…", "permission", permissionLabel(permissionMode)),
        ],
      },
      {
        title: "Repository",
        items: [
          action("review", "Review the working tree"),
          action("review-range", "Review a git range…"),
          action("review-pr", "Review a GitHub PR…"),
          action("diff", "Show uncommitted changes"),
          action("status", "Status"),
          action("memory", "Memory"),
          action("mom", "Model policy"),
          opens(
            "mcp",
            "MCP servers…",
            "mcp",
            mcpServers.length ? `${enabled} of ${mcpServers.length} on` : undefined
          ),
        ],
      },
      {
        title: "Skills",
        limit: SKILLS_SHOWN,
        items: skills.map((skill) => ({
          kind: "action" as const,
          id: `skill:${skill.name}`,
          label: `/${skill.name}`,
          icon: ICONS.skill,
          slash: `/${skill.name}`,
          detail: skill.plugin ? `${skill.plugin} · ${skill.detail}` : skill.detail,
          // The name stays in the box, as a command reads; what it means to the
          // agent is settled on the way out, in `expandSkills`.
          takesArg: true,
          run: () => {},
        })),
      },
    ];
  }, [menuOpen, model, providers, effort, permissionMode, mcpServers, skills, onCommand, onEffort]);

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // The command menu reads its own keys off the document, ahead of this, so
    // Enter on a highlighted row must not also send the line.
    if (command) return;
    if (suggestions.length > 0 && files.onKey(e)) return;
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  };

  const popup = menuOpen ? (
    <CommandMenu sections={sections} query={command ? command.query : null} onRun={runItem} />
  ) : menu === "add" ? (
    <div className="picker" role="menu" aria-label="Add a file">
      <button
        className="picker-row"
        role="menuitem"
        onClick={() => {
          closeMenu();
          post({ type: "attachFiles" });
        }}
      >
        <UploadIcon />
        <span className="picker-body">
          <span className="picker-label">Upload from this computer</span>
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
      onSelect={onPermissionMode}
      onClose={closeMenu}
      onReview={onReview}
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
      onEffort={onEffort}
      onProvider={onProvider}
      onClose={closeMenu}
    />
  ) : menu === "mcp" ? (
    <McpPicker servers={mcpServers} onToggle={onToggleMcp} />
  ) : null;

  const anchor =
    menu === "add"
      ? addRef
      : menu === "settings" || menu === "model" || menu === "provider"
        ? chipRef
        : undefined;

  return (
    <div className="composer-wrap">
      {popup ? (
        <Popover onClose={closeMenu} anchor={anchor}>
          {popup}
        </Popover>
      ) : (
        <Autocomplete items={suggestions} active={files.active} onPick={pick} />
      )}

      <div
        className="composer"
        data-permission-mode={permissionMode}
        data-dropping={dropping}
        // Without a handled dragover the browser refuses the drop outright.
        onDragOver={(e) => {
          e.preventDefault();
          e.dataTransfer.dropEffect = "copy";
          setDropping(true);
        }}
        // Only when the pointer really left: dragging across a child fires
        // dragleave on the parent, which would flicker the highlight off.
        onDragLeave={(e) => {
          if (!e.currentTarget.contains(e.relatedTarget as Node)) setDropping(false);
        }}
        onDrop={onDrop}
      >
        <QueuedList
          queued={queued}
          onSteer={onSteerQueued}
          onEdit={onEditQueued}
          onRemove={onUnqueue}
          onReorder={onReorderQueued}
        />
        {attachments.length > 0 && (
          <div className="composer-files">
            {attachments.map((file) => (
              <span key={file.mention} className="file-chip" title={file.mention}>
                {file.thumb ? (
                  <img className="file-chip-thumb" src={file.thumb} alt="" />
                ) : (
                  <FileTypeIcon kind={fileIconKind(file.mention)} />
                )}
                <span className="file-chip-name">{file.name}</span>
                <button
                  className="ghost file-chip-remove"
                  onClick={() =>
                    setAttachments((prev) => prev.filter((a) => a.mention !== file.mention))
                  }
                  title="Remove"
                  aria-label={`Remove ${file.name}`}
                >
                  <XIcon />
                </button>
              </span>
            ))}
          </div>
        )}
        <div className="input-wrap">
          <div className="input-mirror" aria-hidden="true" ref={mirrorRef}>
            {renderMentions(text)}
          </div>
          <textarea
            ref={areaRef}
            className="composer-input"
            rows={1}
            value={text}
            placeholder={
              busy ? "Queue a follow-up…" : "Ask Aster, @ for files, / for commands"
            }
            onChange={(e) => sync(e.currentTarget)}
            onKeyUp={(e) => setCaret(e.currentTarget.selectionStart)}
            onClick={(e) => setCaret(e.currentTarget.selectionStart)}
            onKeyDown={onKeyDown}
            onPaste={onPaste}
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
            title={effort ? `${model ?? "Model"} · ${effort} effort` : (model ?? "Model")}
            aria-haspopup="menu"
            aria-expanded={menu === "settings"}
          >
            <span className="model-label">{modelChip(model)}</span>
            {effort && <span className="model-effort">{effortShort(effort)}</span>}
          </button>

          <ContextMeter used={contextUsed} budget={contextBudget} onCompact={() => onCommand("compact")} />

          <span className="grow" />

          <button
            className="ghost mode-btn"
            onMouseDown={toggle("permission")}
            title="Mode"
            aria-expanded={menu === "permission"}
          >
            {permissionIcon(permissionMode)}
            {permissionLabel(permissionMode)}
          </button>

          {/* One button whose glyph morphs between send and stop, so the swap
              reads as the same control changing job rather than a re-render.
              Queueing a follow-up mid-run stays on Enter. */}
          <button
            className={busy ? "send stop" : "send"}
            onClick={busy ? onCancel : send}
            disabled={!busy && !canSend}
            title={busy ? "Stop" : "Send"}
            aria-label={busy ? "Stop" : "Send"}
          >
            <IconMorphGlyph shapes={sendStop} active={busy ? 1 : 0} />
          </button>
        </div>
      </div>
    </div>
  );
}

function editorFileUris(data: DataTransfer): string[] {
  const resources = data.getData("resourceurls");
  if (resources) {
    try {
      const uris = (JSON.parse(resources) as string[]).map(decodeURIComponent);
      if (uris.length) return uris;
    } catch {
      // Fall through to the standard flavours below.
    }
  }

  // An editor tab dragged out of its group writes this one instead, and nothing
  // else: without it, dropping the file you are looking at does nothing.
  const editors = data.getData("codeeditors");
  if (editors) {
    try {
      const uris = (JSON.parse(editors) as { resource?: { external?: string; fsPath?: string } }[])
        .map(({ resource }) =>
          resource?.external ?? (resource?.fsPath ? `file://${encodeURI(resource.fsPath)}` : "")
        )
        .filter((uri) => uri.startsWith("file://"));
      if (uris.length) return uris;
    } catch {
      // Fall through to the standard flavours below.
    }
  }

  return [];
}

const MAX_PASTE_BYTES = 10 * 1024 * 1024;

// Images get downscaled by the host before they are sent, so a large
// screenshot is worth reading; only a genuinely huge one is refused.
const MAX_IMAGE_PASTE_BYTES = 64 * 1024 * 1024;

function attachmentFor(token: string, thumbs: Map<string, string>): Attachment | null {
  if (!token.startsWith("@") || token.includes("#")) return null;
  const path = token.slice(1);
  const name = displayName(path);
  if (IMAGE.test(path)) {
    // Host thumbs are keyed by the full path; staged pastes by the name they
    // were given, which may be all the token has.
    return { mention: token, name, thumb: thumbs.get(path) ?? thumbs.get(name) };
  }
  return { mention: token, name };
}

async function readPasted(file: File): Promise<PastedFile & { type: string }> {
  const limit = file.type.startsWith("image/") ? MAX_IMAGE_PASTE_BYTES : MAX_PASTE_BYTES;
  if (file.size > limit) {
    return { name: file.name, size: file.size, type: file.type };
  }
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = "";
  // Chunked: spreading megabytes of bytes into one call overflows the stack.
  for (let at = 0; at < bytes.length; at += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(at, at + 0x8000));
  }
  return { name: file.name, data: btoa(binary), size: file.size, type: file.type };
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
  // Space-aware the way the real renderer is, so a macOS path like
  // ".../Screenshot 2026-09-01 at 1.33.55 PM.png" stays one mention.
  const pattern = /(^|\s)@([^\s@]+(?: [^\s@]+)*?\.(?:[a-z0-9]+))(?=\s|$)/gi;
  let cursor = 0;
  let match: RegExpExecArray | null;
  let key = 0;

  while ((match = pattern.exec(text)) !== null) {
    const at = match.index + match[1].length;
    if (at > cursor) {
      nodes.push(text.slice(cursor, at));
    }
    nodes.push(
      <span key={key++} className="mention">
        {`@${match[2]}`}
      </span>
    );
    cursor = at + match[2].length + 1;
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
