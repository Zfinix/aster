import { useEffect, useMemo, useState } from "react";
import type {
  ConfigScope,
  ConfigValue,
  EditorSettings,
  SettingsSnapshot,
} from "../../src/protocol";
import { EditorSection } from "./EditorSection";
import { KeysSection } from "./KeysSection";
import { McpSection } from "./McpSection";
import { SettingRow } from "./SettingRow";
import { SettingsNav } from "./SettingsNav";
import { ScopeSwitcher } from "./ScopeSwitcher";
import { SettingsSearch } from "./SettingsSearch";
import { onHostMessage, post } from "./host";
import { SECTIONS, keysFor, search, type Section } from "./sections";

const INSTALL_CMD = "curl -fsSL https://withaster.dev/install | sh";

export function SettingsApp() {
  const [snapshot, setSnapshot] = useState<SettingsSnapshot | null>(null);
  const [section, setSection] = useState<Section>(SECTIONS[0]);
  const [scope, setScope] = useState<ConfigScope>("global");
  const [query, setQuery] = useState("");
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [revealed, setRevealed] = useState<Record<string, string>>({});

  useEffect(() => {
    const off = onHostMessage((message) => {
      if (message.type === "settings") {
        setSnapshot(message.snapshot);
        // A snapshot only arrives after a write succeeded or the page reloaded,
        // so whatever failed last time no longer describes what is on screen.
        setErrors({});
        setRevealed({});
      } else if (message.type === "apiKeyValue") {
        if (message.value !== null) {
          setRevealed((prev) => ({ ...prev, [message.var]: message.value as string }));
        }
      } else if (message.type === "settingsError") {
        setErrors((prev) => ({ ...prev, [message.key ?? "*"]: message.message }));
      }
    });
    post({ type: "ready" });
    return off;
  }, []);

  const hasWorkspace = Boolean(snapshot?.workspaceRoot);

  // Workspace settings need a folder; without one the switcher is disabled and
  // the selection must not be left somewhere it cannot write.
  useEffect(() => {
    if (!hasWorkspace && scope === "local") setScope("global");
  }, [hasWorkspace, scope]);

  const counts = useMemo(() => {
    const out: Record<string, number> = {};
    for (const entry of SECTIONS) {
      out[entry.id] = keysFor(entry, snapshot?.keys ?? []).filter(
        (key) => key.scopes[scope] !== null
      ).length;
    }
    return out;
  }, [snapshot, scope]);

  if (!snapshot) {
    return <div className="set-shell loading">Reading configuration…</div>;
  }

  if (!snapshot.binaryOk) {
    return <MissingCli path={snapshot.editor.binaryPath} />;
  }

  const rows = keysFor(section, snapshot.keys);
  const results = query ? search(snapshot.keys, query) : [];
  const found = results.reduce((total, group) => total + group.keys.length, 0);
  const setEditor = (key: keyof EditorSettings, value: ConfigValue) =>
    post({ type: "setEditor", key, value });
  const rowFor = (keyRow: (typeof snapshot.keys)[number]) => (
    <SettingRow
      key={keyRow.key}
      keyRow={keyRow}
      scope={scope}
      models={snapshot.models ?? []}
      providers={snapshot.providers ?? []}
      error={errors[keyRow.key]}
      onSet={(value) => post({ type: "setKey", key: keyRow.key, value, scope })}
      onUnset={() => post({ type: "unsetKey", key: keyRow.key, scope })}
    />
  );

  return (
    <div className="set-shell">
      <SettingsNav
        active={query ? null : section.id}
        counts={counts}
        query={query}
        onQuery={setQuery}
        onSelect={(next) => {
          setQuery("");
          setSection(next);
        }}
      />
      <main className="set-main">
        <header className="set-head">
          <div>
            <h1 className="set-title">{query ? "Search" : section.label}</h1>
            <p className="set-blurb">
              {query
                ? `${found} ${found === 1 ? "setting" : "settings"} matching “${query}”`
                : section.blurb}
            </p>
          </div>
          {section.id !== "editor" && (
            <ScopeSwitcher
              scope={scope}
              paths={snapshot.paths}
              hasWorkspace={hasWorkspace}
              onChange={setScope}
              onOpenFile={() => post({ type: "openConfigFile", scope })}
            />
          )}
        </header>

        {snapshot.error && <p className="set-banner error">{snapshot.error}</p>}
        {errors["*"] && <p className="set-banner error">{errors["*"]}</p>}

        {query ? (
          found === 0 ? (
            <p className="set-note">
              Nothing matches. The Editor section holds the settings VS Code keeps, which are not
              searched here.
            </p>
          ) : (
            results.map((group) => (
              <section key={group.section.id}>
                <h2 className="set-group-title">{group.section.label}</h2>
                <div className="set-card">{group.keys.map(rowFor)}</div>
              </section>
            ))
          )
        ) : section.id === "editor" ? (
          <>
            <p className="set-note">
              These belong to the extension and are stored in VS Code settings, not in aster.yaml.
            </p>
            <EditorSection editor={snapshot.editor} onSet={setEditor} />
          </>
        ) : section.id === "keys" ? (
          <KeysSection
            apiKeys={snapshot.apiKeys ?? []}
            errors={errors}
            revealed={revealed}
            onSet={(name, value) => post({ type: "setApiKey", var: name, value, scope })}
            onUnset={(name) => post({ type: "unsetApiKey", var: name, scope })}
            onReveal={(name) => post({ type: "revealApiKey", var: name })}
            onHide={(name) =>
              setRevealed((prev) => {
                const next = { ...prev };
                delete next[name];
                return next;
              })
            }
          />
        ) : (
          <div className="set-card">{rows.map(rowFor)}</div>
        )}

        {!query && section.id === "mcp" && (
          <McpSection
            servers={snapshot.servers}
            onToggle={(name, disabled) => post({ type: "toggleMcp", name, disabled })}
          />
        )}
      </main>
    </div>
  );
}

function MissingCli({ path }: { path: string }) {
  return (
    <div className="set-shell empty">
      <div className="set-empty">
        <h1 className="set-title">Aster CLI not found</h1>
        <p className="set-blurb">
          Settings are read and written through the <code>aster</code> binary
          {path === "aster" ? "" : ` at ${path}`}. Install it, then reload.
        </p>
        <pre className="set-code">
          <code>{INSTALL_CMD}</code>
        </pre>
        <p className="set-blurb">
          Installed elsewhere? Point <code>aster.binaryPath</code> at it.
        </p>
        <button type="button" className="set-retry" onClick={() => post({ type: "reload" })}>
          Try again
        </button>
      </div>
    </div>
  );
}
