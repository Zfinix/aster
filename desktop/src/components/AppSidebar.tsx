import { useEffect, useRef, useState } from "react";
import { Switch } from "@heroui/react";
import { latestReview, modelShort, type Conversation } from "../lib/session";
import type { Severity } from "../lib/types";
import type { AuthStatus } from "../lib/aster";
import { severityOf, SEV_RANK } from "../lib/severity";
import { useToast, type Theme } from "./chrome";
import { Mark } from "./Mark";
import { EditIcon, FolderIcon, GearIcon, LayersIcon } from "./icons";

const SEV_COLOR: Record<Severity, string> = {
  critical: "var(--red)",
  high: "var(--coral)",
  medium: "var(--amber)",
  low: "#7aa2f7",
  info: "var(--faint)",
};

function dotColor(c: Conversation): string {
  const review = latestReview(c);
  if (!review) return "var(--faint)";
  if (review.findings.length) {
    const top = review.findings
      .map((f) => severityOf(f.severity))
      .sort((a, b) => SEV_RANK[a] - SEV_RANK[b])[0];
    return SEV_COLOR[top];
  }
  return review.status === "done" ? "var(--green)" : "var(--faint)";
}

interface Props {
  conversations: Conversation[];
  activeId: string | null;
  onNewReview: () => void;
  onOpen: (id: string) => void;
  theme: Theme;
  setTheme: (t: Theme) => void;
  minConfidence: number;
  model: string;
  analyzers: string[];
  onToggleAnalyzer: (name: string, on: boolean) => void;
  auth: AuthStatus | null;
  onSaveProvider: (fields: {
    apiKey?: string;
    baseUrl?: string | null;
    model?: string | null;
  }) => void | Promise<void>;
}

export function AppSidebar(props: Props) {
  const { conversations, activeId, onOpen, theme, setTheme, minConfidence, model } =
    props;
  const [settingsOpen, setSettingsOpen] = useState(false);

  const groups = new Map<string, Conversation[]>();
  for (const c of conversations) {
    groups.set(c.repoName, [...(groups.get(c.repoName) ?? []), c]);
  }

  return (
    <aside className="side">
      <div className="side-top" data-tauri-drag-region>
        <Mark px={1.6} interactive />
        <span className="name">Aster</span>
      </div>

      <button className="side-item" onClick={props.onNewReview}>
        <EditIcon />
        New review
      </button>
      <button
        className="side-item"
        onClick={() => setSettingsOpen(true)}
      >
        <LayersIcon />
        Analyzers
      </button>

      <div className="side-label">Reviews</div>

      <div className="side-scroll">
        {conversations.length === 0 && (
          <div style={{ padding: "6px 10px", color: "var(--faint)", fontSize: 13 }}>
            No reviews yet.
          </div>
        )}
        {[...groups.entries()].map(([repo, list]) => (
          <div key={repo}>
            <div className="proj">
              <FolderIcon />
              {repo}
            </div>
            {list.map((c) => (
              <button
                key={c.id}
                className="thread"
                data-active={c.id === activeId}
                onClick={() => onOpen(c.id)}
              >
                <span className="t-dot" style={{ background: dotColor(c) }} />
                <span className="t-name">{c.title || "Working tree review"}</span>
                <span className="t-meta">{c.whenLabel}</span>
              </button>
            ))}
          </div>
        ))}
      </div>

      <SettingsFoot
        open={settingsOpen}
        setOpen={setSettingsOpen}
        theme={theme}
        setTheme={setTheme}
        minConfidence={minConfidence}
        model={model}
        analyzers={props.analyzers}
        onToggleAnalyzer={props.onToggleAnalyzer}
        auth={props.auth}
        onSaveProvider={props.onSaveProvider}
      />
    </aside>
  );
}

function SettingsFoot({
  open,
  setOpen,
  theme,
  setTheme,
  minConfidence,
  model,
  analyzers,
  onToggleAnalyzer,
  auth,
  onSaveProvider,
}: {
  open: boolean;
  setOpen: (open: boolean) => void;
  theme: Theme;
  setTheme: (t: Theme) => void;
  minConfidence: number;
  model: string;
  analyzers: string[];
  onToggleAnalyzer: (name: string, on: boolean) => void;
  auth: AuthStatus | null;
  onSaveProvider: (fields: {
    apiKey?: string;
    baseUrl?: string | null;
    model?: string | null;
  }) => void | Promise<void>;
}) {
  const toast = useToast();
  const ref = useRef<HTMLDivElement>(null);
  const keyRef = useRef<HTMLInputElement>(null);
  const modelRef = useRef<HTMLInputElement>(null);
  const baseUrlRef = useRef<HTMLInputElement>(null);

  const saveProviderFields = async () => {
    const apiKey = keyRef.current?.value.trim();
    await onSaveProvider({
      apiKey: apiKey ? apiKey : undefined,
      model: modelRef.current?.value.trim() || null,
      baseUrl: baseUrlRef.current?.value.trim() || null,
    });
    if (keyRef.current) keyRef.current.value = "";
    toast("Provider saved");
  };

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open, setOpen]);

  return (
    <div className="side-foot" ref={ref}>
      <button
        className="ghost-icon"
        aria-haspopup="menu"
        aria-expanded={open}
        title="Settings"
        onClick={() => setOpen(!open)}
      >
        <GearIcon />
      </button>
      {open && (
        <div className="settings-menu" role="menu" aria-label="Settings">
          <div className="settings-section">
            <div className="menu-label">Provider</div>
            <div className="menu-row">
              <span>API key</span>
              <span
                className="val"
                style={{ color: auth?.hasKey ? "var(--green)" : "var(--red)" }}
              >
                {auth?.hasKey ? "configured" : "not set"}
              </span>
            </div>
            <input
              ref={keyRef}
              className="menu-input"
              type="password"
              spellCheck={false}
              placeholder={auth?.hasKey ? "replace key…" : "ASTER_API_KEY"}
            />
            <input
              ref={modelRef}
              className="menu-input"
              spellCheck={false}
              defaultValue={auth?.model ?? ""}
              placeholder="model · openai/gpt-4o-mini"
            />
            <input
              ref={baseUrlRef}
              className="menu-input"
              spellCheck={false}
              defaultValue={auth?.baseUrl ?? ""}
              placeholder="base url · OpenRouter default"
            />
            <button className="btn btn-line menu-save" onClick={saveProviderFields}>
              Save
            </button>
          </div>
          <div className="settings-section">
            <div className="menu-label">Appearance</div>
            <div className="theme-seg" role="group" aria-label="Theme">
              <button
                aria-pressed={theme === "light"}
                onClick={() => setTheme("light")}
              >
                Light
              </button>
              <button
                aria-pressed={theme === "dark"}
                onClick={() => setTheme("dark")}
              >
                Dark
              </button>
            </div>
          </div>
          <div className="settings-section">
            <div className="menu-label">Pipeline</div>
            <div className="menu-row">
              <span>Model</span>
              <span className="val">{modelShort(model)}</span>
            </div>
            <div className="menu-row">
              <span>Confidence gate</span>
              <span className="val">{minConfidence.toFixed(2)}</span>
            </div>
          </div>
          <div className="settings-section">
            <div className="menu-label">Analyzers</div>
            <div className="menu-row">
              <span>ast-grep</span>
              <Switch
                size="sm"
                isSelected={analyzers.includes("ast-grep")}
                onChange={(on) => onToggleAnalyzer("ast-grep", on)}
                aria-label="Toggle ast-grep"
              >
                <Switch.Content>
                  <Switch.Control>
                    <Switch.Thumb />
                  </Switch.Control>
                </Switch.Content>
              </Switch>
            </div>
            <div className="menu-row">
              <span>semgrep</span>
              <Switch
                size="sm"
                isSelected={analyzers.includes("semgrep")}
                onChange={(on) => onToggleAnalyzer("semgrep", on)}
                aria-label="Toggle semgrep"
              >
                <Switch.Content>
                  <Switch.Control>
                    <Switch.Thumb />
                  </Switch.Control>
                </Switch.Content>
              </Switch>
            </div>
          </div>
        </div>
      )}
      <div className="side-avatar" />
    </div>
  );
}
