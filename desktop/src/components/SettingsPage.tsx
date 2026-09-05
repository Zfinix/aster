import { useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import type { Update } from "@tauri-apps/plugin-updater";
import type { AuthStatus } from "../lib/aster";
import { checkForUpdate, installUpdate, type UpdateStage } from "../lib/updater";
import { useToast, type Theme } from "./chrome";
import { CloudIcon, InfoIcon, MoonIcon, PaletteIcon, ReviewIcon, SunIcon, XIcon } from "./icons";
import { Mark } from "./Mark";
import { Modal } from "./Modal";
import { Dropdown } from "./Dropdown";
import { modelShort } from "../lib/model";
import { ReviewSettings } from "./ReviewSettings";
import { SettingsRow } from "./SettingsRow";
import { Toggle } from "./Toggle";
import { UpdateControl } from "./UpdateControl";

type Section = "provider" | "appearance" | "review" | "about";

const SECTIONS: { id: Section; label: string; icon: React.ReactNode }[] = [
  { id: "provider", label: "Provider", icon: <CloudIcon /> },
  { id: "appearance", label: "Appearance", icon: <PaletteIcon /> },
  { id: "review", label: "Code review", icon: <ReviewIcon /> },
  { id: "about", label: "About", icon: <InfoIcon /> },
];

export interface SettingsProps {
  onClose: () => void;
  theme: Theme;
  setTheme: (t: Theme) => void;
  minConfidence: number;
  onSetConfidence: (v: number) => void;
  model: string;
  models: string[];
  onModel: (value: string) => void;
  analyzers: string[];
  onToggleAnalyzer: (name: string, on: boolean) => void;
  repoPath: string;
  auth: AuthStatus | null;
  onSaveProvider: (fields: { apiKey?: string; baseUrl?: string | null; model?: string | null }) => void | Promise<void>;
}

export function SettingsPage(props: SettingsProps) {
  const { onClose, theme, setTheme, auth } = props;
  const toast = useToast();
  const [section, setSection] = useState<Section>("provider");
  const keyRef = useRef<HTMLInputElement>(null);
  const modelRef = useRef<HTMLInputElement>(null);
  const baseUrlRef = useRef<HTMLInputElement>(null);
  const [version, setVersion] = useState("");
  const [update, setUpdate] = useState<UpdateStage>({ kind: "idle" });
  const pending = useRef<Update | null>(null);

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);

  const onCheckUpdate = async () => {
    setUpdate({ kind: "checking" });
    try {
      const found = await checkForUpdate();
      if (!found) {
        setUpdate({ kind: "none" });
        return;
      }
      pending.current = found;
      setUpdate({ kind: "available", version: found.version, notes: found.body });
    } catch (e) {
      setUpdate({ kind: "error", message: String(e) });
    }
  };

  const onInstallUpdate = async () => {
    if (!pending.current) return;
    try {
      await installUpdate(pending.current, setUpdate);
    } catch (e) {
      setUpdate({ kind: "error", message: String(e) });
    }
  };

  const saveProviderFields = async () => {
    const apiKey = keyRef.current?.value.trim();
    await props.onSaveProvider({
      apiKey: apiKey ? apiKey : undefined,
      model: modelRef.current?.value.trim() || null,
      baseUrl: baseUrlRef.current?.value.trim() || null,
    });
    if (keyRef.current) keyRef.current.value = "";
    toast("Provider saved");
  };

  return (
    <Modal label="Settings" className="settings" align="center" onClose={onClose}>
      <header className="settings-head">
        <span className="settings-title">Settings</span>
        <button type="button" className="ghost icon-action" aria-label="Close settings" onClick={onClose}>
          <XIcon />
        </button>
      </header>

      <div className="settings-body">
        <nav className="settings-nav" aria-label="Settings sections">
          {SECTIONS.map((s) => (
            <button
              key={s.id}
              type="button"
              className="settings-nav-item"
              data-active={section === s.id}
              onClick={() => setSection(s.id)}
            >
              {s.icon}
              {s.label}
            </button>
          ))}
        </nav>

        <div className="settings-main">
          {section === "provider" && (
            <>
              <div className="settings-section">
                <h2>Provider</h2>
                <p className="settings-note">Saved to your Aster config, so the terminal and editors use the same endpoint.</p>
                <div className="settings-card">
                  <SettingsRow label="API key" help={auth?.hasKey ? "A key is saved. Paste a new one to replace it." : "No key saved yet."}>
                    <input
                      ref={keyRef}
                      className="field-input"
                      data-mono="true"
                      type="password"
                      spellCheck={false}
                      aria-label="API key"
                      placeholder={auth?.hasKey ? "••••••••" : "sk-…"}
                    />
                  </SettingsRow>
                  <SettingsRow label="Base URL" help="Leave empty for the default endpoint.">
                    <input
                      ref={baseUrlRef}
                      className="field-input"
                      data-mono="true"
                      spellCheck={false}
                      aria-label="Base URL"
                      defaultValue={auth?.baseUrl ?? ""}
                      placeholder="https://openrouter.ai/api/v1"
                    />
                  </SettingsRow>
                  <SettingsRow label="Default model" help="What every surface starts with.">
                    <input
                      ref={modelRef}
                      className="field-input"
                      data-mono="true"
                      spellCheck={false}
                      aria-label="Default model"
                      defaultValue={auth?.model ?? ""}
                      placeholder="anthropic/claude-sonnet-5"
                    />
                  </SettingsRow>
                  <div className="settings-actions">
                    <button type="button" className="btn-primary" onClick={saveProviderFields}>
                      Save
                    </button>
                  </div>
                </div>
              </div>

              <div className="settings-section">
                <h2>This app</h2>
                <div className="settings-card">
                  <SettingsRow label="Model" help="Used for chat and reviews here.">
                    <Dropdown
                      triggerClass="btn"
                      options={props.models.map((m) => ({ value: m, label: modelShort(m), detail: m }))}
                      value={props.model}
                      onSelect={props.onModel}
                      dir="down"
                      align="right"
                      width="wide"
                      trigger={() => <span>{modelShort(props.model)}</span>}
                    />
                  </SettingsRow>
                </div>
              </div>
            </>
          )}

          {section === "appearance" && (
            <div className="settings-section">
              <h2>Appearance</h2>
              <div className="settings-card">
                <SettingsRow label="Theme">
                  <div className="seg" role="group" aria-label="Theme">
                    <button type="button" aria-pressed={theme === "light"} onClick={() => setTheme("light")}>
                      <SunIcon />
                      Light
                    </button>
                    <button type="button" aria-pressed={theme === "dark"} onClick={() => setTheme("dark")}>
                      <MoonIcon />
                      Dark
                    </button>
                  </div>
                </SettingsRow>
              </div>
            </div>
          )}

          {section === "review" && (
            <>
              <div className="settings-section">
                <h2>Findings</h2>
                <div className="settings-card">
                  <SettingsRow label="Confidence gate" help="Findings below this are dropped before they reach you.">
                    <div className="slider-row">
                      <input
                        type="range"
                        className="range"
                        min={0}
                        max={0.95}
                        step={0.05}
                        value={props.minConfidence}
                        onChange={(e) => props.onSetConfidence(Number(e.target.value))}
                        style={{ ["--pct" as string]: `${(props.minConfidence / 0.95) * 100}%` }}
                        aria-label="Confidence gate"
                      />
                      <span className="slider-value">{props.minConfidence.toFixed(2)}</span>
                    </div>
                  </SettingsRow>
                  <SettingsRow label="ast-grep" help="Structural pattern matching.">
                    <Toggle
                      on={props.analyzers.includes("ast-grep")}
                      onChange={(on) => props.onToggleAnalyzer("ast-grep", on)}
                      label="ast-grep"
                    />
                  </SettingsRow>
                  <SettingsRow label="semgrep" help="Rule-based static analysis.">
                    <Toggle
                      on={props.analyzers.includes("semgrep")}
                      onChange={(on) => props.onToggleAnalyzer("semgrep", on)}
                      label="semgrep"
                    />
                  </SettingsRow>
                </div>
              </div>
              <ReviewSettings repoPath={props.repoPath || null} />
            </>
          )}

          {section === "about" && (
            <div className="settings-section">
              <h2>About</h2>
              <div className="settings-card">
                <div className="about">
                  <Mark px={2} />
                  <div className="about-text">
                    <span className="about-name">Aster</span>
                    <span className="settings-note">{version ? `Version ${version}` : ""}</span>
                  </div>
                  <UpdateControl stage={update} onCheck={onCheckUpdate} onInstall={onInstallUpdate} />
                </div>
              </div>
            </div>
          )}
        </div>
      </div>
    </Modal>
  );
}
