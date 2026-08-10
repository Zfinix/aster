import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type {
  Effort,
  McpServer,
  PermissionMode,
  Provider,
  SkillCommand,
} from "../../src/protocol";
import { applyTrigger, triggerAt, type Trigger } from "../lib/trigger";
import { post } from "../lib/host";
import { modelShort } from "../lib/model";
import { ApprovalPicker, permissionLabel } from "./ApprovalPicker";
import { Autocomplete, type Suggestion } from "./Autocomplete";
import { CommandMenu, type MenuSection } from "./CommandMenu";
import { McpPicker } from "./McpPicker";
import { ModelPicker } from "./ModelPicker";
import { ProviderPicker } from "./ProviderPicker";
import { IconMorphGlyph, sendStop } from "../interior/icon-morph";
import { CaretUpIcon, CommandIcon, SendIcon, ShieldIcon } from "./icons";

const MAX_ROWS = 10;

/** Only one popup can occupy the slot above the composer. */
type Menu = "none" | "commands" | "permission" | "model" | "provider" | "mcp";

/** Unfiltered, the skills list would bury every other action; the filter is one
 *  keystroke away, and the note says so. */
const SKILLS_SHOWN = 5;

const EFFORTS = [
  { value: "", label: "Default" },
  { value: "off", label: "Off" },
  { value: "low", label: "Low" },
  { value: "medium", label: "Medium" },
  { value: "high", label: "High" },
];

export function Composer({
  busy,
  model,
  models,
  recommended,
  modelsLoading,
  modelsError,
  onRefreshModels,
  permissionMode,
  effort,
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
  modelsLoading: boolean;
  modelsError?: string;
  onRefreshModels: () => void;
  permissionMode: PermissionMode;
  effort: Effort | null;
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

  const trigger: Trigger | null = menu === "none" ? triggerAt(text, caret) : null;

  useEffect(() => {
    if (trigger) onSearchFiles(trigger.query);
  }, [trigger?.query]);

  useEffect(() => setActive(0), [trigger?.query]);

  useEffect(() => {
    if (openMenu) {
      setMenu("commands");
      onMenuOpened();
    }
  }, [openMenu, onMenuOpened]);

  // The folder disambiguates two files of the same name; a file at the root has
  // none, and repeating its name under itself said nothing twice.
  const suggestions: Suggestion[] = useMemo(
    () =>
      trigger
        ? fileResults.slice(0, 8).map((path) => ({
            value: `@${basename(path)}`,
            label: basename(path),
            detail: dirname(path),
            full: path,
          }))
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

  /**
   * A drop carries its files as URIs, under whichever flavour the source used:
   * VS Code's own explorer and editor tabs write `resourceurls` (JSON, and
   * percent-encoded), everything else the standard `text/uri-list`. The host
   * turns them into repo-relative mentions.
   */
  const onDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setDropping(false);
    const data = e.dataTransfer;

    const resources = data.getData("resourceurls");
    if (resources) {
      try {
        const uris = (JSON.parse(resources) as string[]).map(decodeURIComponent);
        if (uris.length) return post({ type: "dropFiles", uris });
      } catch {
        // Fall through to the standard flavours below.
      }
    }

    const list = data.getData("text/uri-list") || data.getData("text/plain");
    const uris = list
      .split(/\r?\n/)
      .map((line) => line.trim())
      // `#` opens a comment line in uri-list, and a bare path is not a drop we
      // can resolve; only real file URIs go to the host.
      .filter((line) => line.startsWith("file://"));
    if (uris.length) {
      post({ type: "dropFiles", uris });
    }
  };

  const pick = (item: Suggestion) => {
    if (!trigger) return;
    if (item.full) {
      mentions.current.set(item.value, `@${item.full}`);
    }
    const next = applyTrigger(text, trigger, caret, item.value);
    setText(next);
    const at = trigger.start + item.value.length + 1;
    requestAnimationFrame(() => {
      const area = areaRef.current;
      if (area) {
        area.focus();
        area.setSelectionRange(at, at);
        setCaret(at);
      }
    });
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
  const closeMenu = () => {
    setMenu("none");
    focusInput();
  };

  /** Put text in the box, for the menu rows that start a message rather than
   *  doing something. The caret follows it so an inserted `@` opens the file
   *  list straight away. */
  const compose = (value: string) => {
    const next = text ? `${text.replace(/\s*$/, "")} ${value}` : value;
    setText(next);
    setCaret(next.length);
    focusInput(next.length);
  };

  // Only while the menu is up: mapping every skill on each streamed token
  // would be a real cost for a list nobody is looking at.
  const sections: MenuSection[] = useMemo(() => {
    if (menu !== "commands") return [];
    const enabled = mcpServers.filter((s) => !s.disabled).length;
    const provider = providers.find((p) => p.current);
    const action = (id: string, label: string, hint?: string) => ({
      kind: "action" as const,
      id,
      label,
      hint,
      run: () => onCommand(id),
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
            run: () => compose("@"),
          },
        ],
      },
      {
        title: "Model",
        items: [
          {
            kind: "action" as const,
            id: "model",
            label: "Switch model…",
            hint: modelShort(model),
            run: () => setMenu("model"),
          },
          {
            kind: "action" as const,
            id: "provider",
            label: "Switch provider…",
            hint: provider?.name,
            run: () => setMenu("provider"),
          },
          {
            kind: "choice" as const,
            id: "effort",
            label: "Effort",
            value: effort ?? "",
            options: EFFORTS,
            onSelect: (value: string) => onEffort((value || null) as Effort | null),
          },
          {
            kind: "action" as const,
            id: "mode",
            label: "Mode…",
            hint: permissionLabel(permissionMode),
            run: () => setMenu("permission"),
          },
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
          {
            kind: "action" as const,
            id: "mcp",
            label: "MCP servers…",
            hint: mcpServers.length ? `${enabled} of ${mcpServers.length} on` : undefined,
            run: () => setMenu("mcp"),
          },
        ],
      },
      {
        title: "Skills",
        items: skills.map((skill) => ({
          kind: "action" as const,
          id: `skill:${skill.name}`,
          label: skill.name,
          detail: skill.plugin ? `${skill.plugin} · ${skill.detail}` : skill.detail,
          // The agent applies a skill by reading it, so this opens the sentence
          // that asks for it and leaves the task to the user.
          run: () => compose(`Use the "${skill.name}" skill:`),
        })),
      },
    ].map((section) =>
      section.title === "Skills" && section.items.length > SKILLS_SHOWN
        ? {
            ...section,
            items: section.items.slice(0, SKILLS_SHOWN),
            note: `${section.items.length - SKILLS_SHOWN} more; type to filter`,
          }
        : section
    );
  }, [menu, model, providers, effort, permissionMode, mcpServers, skills, onCommand, onEffort]);

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // `/` on an empty box is the menu's shortcut, not a character: a GUI should
    // not make anyone type a command name at a prompt.
    if (e.key === "/" && !text) {
      e.preventDefault();
      setMenu("commands");
      return;
    }
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
      {menu === "permission" && (
        <ApprovalPicker
          mode={permissionMode}
          onSelect={onPermissionMode}
          onClose={closeMenu}
          onReview={onReview}
        />
      )}
      {menu === "model" && (
        <ModelPicker
          model={model}
          models={models}
          recommended={recommended}
          loading={modelsLoading}
          error={modelsError}
          onSelect={onModel}
          onRefresh={onRefreshModels}
          onClose={closeMenu}
        />
      )}
      {menu === "commands" && (
        <CommandMenu sections={sections} onClose={closeMenu} />
      )}
      {menu === "provider" && (
        <ProviderPicker
          providers={providers}
          onSelect={onProvider}
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
            onScroll={syncScroll}
          />
        </div>
        <div className="composer-foot">
          <button
            className="ghost cmd-btn"
            onClick={() => setMenu(menu === "commands" ? "none" : "commands")}
            title="Show command menu (/)"
            aria-label="Show command menu"
            aria-haspopup="dialog"
            aria-expanded={menu === "commands"}
          >
            <CommandIcon />
          </button>

          <button
            className="ghost"
            onClick={() => setMenu(menu === "permission" ? "none" : "permission")}
            title="Mode"
            aria-expanded={menu === "permission"}
          >
            <ShieldIcon />
            {permissionLabel(permissionMode)}
          </button>

          <span className="grow" />

          <button
            className="ghost model-btn"
            onClick={() => setMenu(menu === "model" ? "none" : "model")}
            title={effort ? `${model ?? "Model"} · ${effort} effort` : (model ?? "Model")}
            aria-haspopup="menu"
            aria-expanded={menu === "model"}
          >
            <span className="model-label">{modelShort(model)}</span>
            {effort && <span className="model-effort">{effort}</span>}
            <CaretUpIcon open={menu === "model"} />
          </button>

          {/* While busy, Stop stays reachable and Send doubles as "queue". */}
          {busy && text.trim() && (
            <button className="send" onClick={send} title="Queue message" aria-label="Queue message">
              <SendIcon />
            </button>
          )}
          {/* One button whose glyph morphs between send and stop, so the swap
              reads as the same control changing job rather than a re-render. */}
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
