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
import { post } from "../lib/host";
import { EFFORT_OPTIONS, effortShort } from "../lib/effort";
import { modelShort } from "../lib/model";
import { AddMenu } from "./AddMenu";
import { ApprovalPicker, permissionIcon, permissionLabel } from "./ApprovalPicker";
import { Autocomplete, type Suggestion } from "./Autocomplete";
import { CommandMenu, type MenuItem, type MenuSection } from "./CommandMenu";
import { ContextMeter } from "./ContextMeter";
import { McpPicker } from "./McpPicker";
import { ModelMenu } from "./ModelMenu";
import { IconMorphGlyph, sendStop } from "../interior/icon-morph";
import {
  ActivityIcon,
  AtIcon,
  BookIcon,
  BrainIcon,
  CaretUpIcon,
  CloudIcon,
  CommandIcon,
  CubeIcon,
  DiffIcon,
  GaugeIcon,
  GitCommitIcon,
  GitPullRequestIcon,
  HistoryIcon,
  MinimizeIcon,
  NewChatIcon,
  PlugIcon,
  ReviewIcon,
  ShieldIcon,
  TrashIcon,
} from "./icons";

const MAX_ROWS = 10;

/** Only one popup can occupy the slot above the composer. */
type Menu = "none" | "commands" | "permission" | "settings" | "model" | "provider" | "mcp";

/** Unfiltered, the skills list would bury every other action; the filter is one
 *  keystroke away, and the note says so. */
/** One glyph a command, so the list reads as a column of actions rather than a
 *  wall of sentences. */
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
  "review-range": <GitCommitIcon />,
  "review-pr": <GitPullRequestIcon />,
  diff: <DiffIcon />,
  status: <ActivityIcon />,
  memory: <BrainIcon />,
  mcp: <PlugIcon />,
  skill: <BookIcon />,
};

const SKILLS_SHOWN = 5;

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
  /** Characters the next turn would send, against the CLI's compact budget. */
  contextUsed: number;
  contextBudget: number;
  skills: SkillCommand[];
  mcpServers: McpServer[];
  providers: Provider[];
  fileResults: string[];
  /** Set when the host asks for the menu, e.g. from the VS Code palette. */
  openMenu: boolean;
  onMenuOpened: () => void;
  /** Text pushed in from the editor, e.g. an @-mention of the selection. */
  insertText: string | null;
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
}) {
  const [text, setText] = useState("");
  const [caret, setCaret] = useState(0);
  const [menu, setMenu] = useState<Menu>("none");
  /** Escape on a typed `/name` puts the menu away without taking the text with
   *  it; it comes back when the caret next lands on a fresh one. */
  const [dismissed, setDismissed] = useState(false);
  const [active, setActive] = useState(0);
  /** True while a drag hovers the composer, so it reads as a drop target. */
  const [dropping, setDropping] = useState(false);
  const areaRef = useRef<HTMLTextAreaElement>(null);
  const mirrorRef = useRef<HTMLDivElement>(null);
  /** `@basename` shown in the box -> `@full/path` actually sent. */
  const mentions = useRef(new Map<string, string>());

  /** The mirror paints the text, so it must follow the textarea's scroll or
   *  everything past the visible rows types blind. */
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

  useEffect(() => setActive(0), [trigger?.query]);

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

  useEffect(() => {
    if (!insertText) return;
    setText((prev) => (prev && !prev.endsWith(" ") ? `${prev} ${insertText} ` : `${prev}${insertText} `));
    onInserted();
    requestAnimationFrame(() => areaRef.current?.focus());
  }, [insertText, onInserted]);

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

  /** A drop or clipboard that carries no URI (a file dragged from the OS, say)
   *  gives up bytes and a name, so it goes through the paste pipeline: the host
   *  matches it back to the workspace or writes it to storage. */
  const sendPasted = (files: File[]) =>
    void Promise.all(files.map(readPasted)).then((pasted) =>
      post({ type: "pasteFiles", files: pasted })
    );

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setDropping(false);
    const uris = fileUris(e.dataTransfer);
    if (uris.length) {
      post({ type: "dropFiles", uris });
      return;
    }
    const files = Array.from(e.dataTransfer.files);
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

  /**
   * A paste is the drop's poorer cousin: the OS clipboard rarely carries a URI,
   * so the file arrives as bytes and a bare name and the host has to work out
   * which file that was. Plain text is left to the textarea.
   */
  const onPaste = (e: React.ClipboardEvent) => {
    const uris = fileUris(e.clipboardData);
    if (uris.length) {
      e.preventDefault();
      return post({ type: "dropFiles", uris });
    }

    const files = Array.from(e.clipboardData.files);
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

  /** Put `next` in the box and leave the caret where the next word goes. */
  const write = (next: string, at = next.length) => {
    setText(next);
    setCaret(at);
    focusInput(at);
  };

  // Sending while busy is allowed: App queues it and flushes when the run ends.
  const send = () => {
    const trimmed = text.trim();
    if (!trimmed) return;
    onSend(expandMentions(trimmed, mentions.current));
    setText("");
    setCaret(0);
  };

  const focusInput = (at?: number) =>
    requestAnimationFrame(() => {
      const area = areaRef.current;
      if (!area) return;
      area.focus();
      if (at !== undefined) area.setSelectionRange(at, at);
    });

  /** Closing any popup hands the keyboard back, so the next keystroke types
   *  instead of landing on nothing. */
  /**
   * Toggles on mousedown and keeps the event to itself. Every popup closes on a
   * mousedown outside it, so a click that bubbled would shut the menu on the
   * way down and this handler would reopen it on the way back up, which reads
   * as a blink and never as "off".
   */
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

  /** Append to what is already typed, for the rows that start a message rather
   *  than doing something. The caret follows so an inserted `@` opens the file
   *  list straight away. */
  const compose = (value: string, base = text) => {
    const head = base.replace(/\s*$/, "");
    write(head ? `${head} ${value}` : value);
  };

  /**
   * Tab, a row that takes an argument, or a `/name` typed mid-sentence all
   * complete the name into the box so the rest of the line can follow it.
   * Enter on anything else runs the row and takes the name back out, which is
   * what the CLI does with the same keystroke.
   */
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
    /** Mirrors one of the CLI's own commands, under the same name. */
    const action = (id: string, label: string, hint?: string) => ({
      kind: "action" as const,
      id,
      label,
      hint,
      icon: ICONS[id],
      slash: `/${id}`,
      run: () => onCommand(id),
    });
    /** Opens another surface, so it must not be run by completing its name. */
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
          action("compact", "Compact conversation"),
          action("resume", "Resume a session…"),
          {
            kind: "action" as const,
            id: "mention",
            label: "Mention a file…",
            icon: ICONS.mention,
            run: (rest: string) => compose("@", rest),
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
    if (suggestions.length > 0) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setActive((i) => (i + 1) % suggestions.length);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setActive((i) => (i - 1 + suggestions.length) % suggestions.length);
        return;
      }
      if (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey)) {
        e.preventDefault();
        pick(suggestions[active]);
        return;
      }
    }
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  };

  return (
    <div className="composer-wrap">
      {/* A click meant for "put that away" should not also hit whatever was
          under the popup, so it lands here instead. */}
      {(menuOpen || menu !== "none") && <div className="scrim" onMouseDown={closeMenu} />}
      {menu === "permission" && (
        <ApprovalPicker
          mode={permissionMode}
          onSelect={onPermissionMode}
          onClose={closeMenu}
          onReview={onReview}
        />
      )}
      {(menu === "settings" || menu === "model" || menu === "provider") && (
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
      )}
      {menuOpen && (
        <CommandMenu
          sections={sections}
          query={command ? command.query : null}
          onRun={runItem}
          onClose={closeMenu}
        />
      )}
      {menu === "mcp" && (
        <McpPicker
          servers={mcpServers}
          onToggle={onToggleMcp}
          onClose={closeMenu}
        />
      )}
      {menu === "none" && <Autocomplete items={suggestions} active={active} onPick={pick} />}

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
          <AddMenu onUpload={() => post({ type: "attachFiles" })} onMention={() => compose("@")} />

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

          <ContextMeter used={contextUsed} budget={contextBudget} onCompact={() => onCommand("compact")} />

          <span className="grow" />

          <button
            className="ghost"
            onMouseDown={toggle("permission")}
            title="Mode"
            aria-expanded={menu === "permission"}
          >
            {permissionIcon(permissionMode)}
            {permissionLabel(permissionMode)}
          </button>

          <button
            className="ghost model-btn"
            onMouseDown={toggle("settings")}
            // The chip is a hover target, not a hunt: pointing at it opens the
            // menu, the way the mode rows open their lists.
            onMouseEnter={() => menu === "none" && !menuOpen && setMenu("settings")}
            title={effort ? `${model ?? "Model"} · ${effort} effort` : (model ?? "Model")}
            aria-haspopup="menu"
            aria-expanded={menu === "settings"}
          >
            <span className="model-label">{modelShort(model)}</span>
            {effort && <span className="model-effort">{effortShort(effort)}</span>}
            <CaretUpIcon open={menu === "settings"} />
          </button>

          {/* One button whose glyph morphs between send and stop, so the swap
              reads as the same control changing job rather than a re-render.
              Queueing a follow-up mid-run stays on Enter. */}
          <button
            className={busy ? "send stop" : "send"}
            onClick={busy ? onCancel : send}
            disabled={!busy && !text.trim()}
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

/**
 * The file URIs a transfer carries, under whichever flavour the source used:
 * VS Code's own explorer and editor tabs write `resourceurls` (JSON, and
 * percent-encoded), everything else the standard `text/uri-list`.
 */
function fileUris(data: DataTransfer): string[] {
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

  const list = data.getData("text/uri-list") || data.getData("text/plain");
  return (
    list
      .split(/\r?\n/)
      .map((line) => line.trim())
      // `#` opens a comment line in uri-list, and a bare path is not something
      // we can resolve; only real file URIs go to the host.
      .filter((line) => line.startsWith("file://"))
  );
}

/** Past this the bytes are not worth pushing through the message channel; the
 *  host still gets the name and can match it against the workspace. */
const MAX_PASTE_BYTES = 10 * 1024 * 1024;

async function readPasted(file: File): Promise<PastedFile> {
  if (file.size > MAX_PASTE_BYTES) {
    return { name: file.name, size: file.size };
  }
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = "";
  // Chunked: spreading megabytes of bytes into one call overflows the stack.
  for (let at = 0; at < bytes.length; at += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(at, at + 0x8000));
  }
  return { name: file.name, data: btoa(binary), size: file.size };
}

function basename(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

/** The folder holding `path`, or nothing when it sits at the repo root. */
function dirname(path: string): string | undefined {
  const at = path.lastIndexOf("/");
  return at === -1 ? undefined : path.slice(0, at);
}

/**
 * Split the raw text so @mentions can be drawn as chips in the mirror layer.
 * The trailing zero-width space keeps a text node present on an empty line so
 * the mirror's height never collapses below the textarea's.
 */
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
      </span>
    );
    cursor = match.index + match[0].length;
  }
  nodes.push(`${text.slice(cursor)}​`);
  return nodes;
}

/**
 * Swap each `@basename` back to the path it was picked from, so the composer
 * stays readable while the agent still gets an unambiguous location.
 */
function expandMentions(text: string, mentions: Map<string, string>): string {
  let out = text;
  for (const [shown, full] of mentions) {
    out = out.split(shown).join(full);
  }
  return out;
}
