import { useEffect, useState } from "react";
import { configList, configSet } from "../lib/aster";
import { SettingsRow } from "./SettingsRow";

const KEY = "review.codex_fast_mode";

export function CodexFastModeSetting({ repoPath }: { repoPath: string | null }) {
  return <CodexFastModeControl key={repoPath} repoPath={repoPath} />;
}

function CodexFastModeControl({ repoPath }: { repoPath: string | null }) {
  const [setting, setSetting] = useState<{ on: boolean; source: string } | null>(null);
  const [request, setRequest] = useState({ value: null as boolean | null, attempt: 0 });
  const [pending, setPending] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    const apply = async () => {
      try {
        if (request.value !== null) {
          await configSet(KEY, String(request.value), { repoPath });
        }
        const entries = await configList(repoPath);
        const entry = entries.find((e) => e.key === KEY);
        if (!entry) throw new Error("Codex Fast mode is not available in this CLI. Update Aster and retry.");
        const value = String(entry.value ?? entry.default ?? false).trim().toLowerCase();
        if (!["true", "false", "1", "0", "yes", "no", "on", "off"].includes(value)) {
          throw new Error(`Invalid Codex Fast mode value: ${value}`);
        }
        if (live) setSetting({ on: ["true", "1", "yes", "on"].includes(value), source: entry.source });
      } catch (e) {
        if (live) setError(String(e));
      } finally {
        if (live) setPending(false);
      }
    };
    void apply();
    return () => {
      live = false;
    };
  }, [repoPath, request]);

  const overridden = /^env\b/i.test(setting?.source ?? "");
  const submit = (value: boolean | null) => {
    if (pending || overridden) return;
    setPending(true);
    setError(null);
    setRequest((r) => ({ value, attempt: r.attempt + 1 }));
  };

  return (
    <SettingsRow
      label="Codex Fast mode"
      help={
        <>
          Codex only (chatgpt.com). Faster responses with higher usage charges. Saved to {repoPath ? "this repository's config" : "Aster config"}; applies to the next request.
          {overridden && <> Controlled by ASTER_CODEX_FAST_MODE ({setting?.source}). Unset the environment override to change this setting.</>}
          {pending && <span role="status"> {request.value === null ? "Loading…" : "Saving…"}</span>}
          {error && <span role="alert"> {error}</span>}
        </>
      }
    >
      <button
        type="button"
        role="switch"
        aria-checked={setting?.on ?? false}
        aria-label="Codex Fast mode"
        className="switch"
        disabled={pending || !setting || overridden || error !== null}
        onClick={() => submit(!setting?.on)}
      >
        <span className="switch-knob" />
      </button>
      {error && (
        <button type="button" className="btn" disabled={pending || overridden} onClick={() => submit(request.value)}>
          Retry
        </button>
      )}
    </SettingsRow>
  );
}
