import { useEffect, useState } from "react";
import { configList, configSet, type ConfigValue } from "../lib/aster";
import { useToast } from "./chrome";
import { SettingsRow } from "./SettingsRow";

function toText(value: ConfigValue): string {
  if (value == null) return "";
  if (Array.isArray(value)) return value.join(", ");
  return String(value);
}

const KEYS: { key: string; label: string; help: string }[] = [
  { key: "review.model", label: "Model", help: "Used for the whole review unless a stage sets its own." },
  { key: "review.hypothesis_model", label: "First-pass model", help: "Finds candidate issues." },
  { key: "review.verify_model", label: "Verify model", help: "Checks each candidate before it reaches you." },
  { key: "review.effort", label: "Reasoning effort", help: "off, low, medium or high." },
  { key: "review.max_diff_bytes", label: "Largest diff", help: "In bytes. Bigger diffs are skipped." },
  { key: "review.analyzers", label: "Static analyzers", help: "Comma separated." },
  { key: "review.focus_areas", label: "Focus areas", help: "Comma separated." },
  { key: "review.include", label: "Only review", help: "Globs, comma separated." },
  { key: "review.exclude", label: "Never review", help: "Globs, comma separated." },
  { key: "review.web_search", label: "Web search", help: "true or false." },
];

/** Review pipeline settings, read and written through `aster config` so the
 *  desktop, terminal, and editors share one aster.yaml. */
export function ReviewSettings({ repoPath }: { repoPath?: string | null }) {
  const toast = useToast();
  const [values, setValues] = useState<Record<string, string>>({});
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    let live = true;
    configList(repoPath ?? null)
      .then((entries) => {
        if (!live) return;
        const next: Record<string, string> = {};
        for (const e of entries) next[e.key] = toText(e.value);
        setValues(next);
        setLoaded(true);
      })
      .catch(() => setLoaded(true));
    return () => {
      live = false;
    };
  }, [repoPath]);

  const save = async (key: string) => {
    try {
      await configSet(key, values[key] ?? "", { repoPath: repoPath ?? null });
      toast("Saved");
    } catch (e) {
      toast(`Save failed: ${String(e)}`);
    }
  };

  if (!loaded) return null;

  return (
    <div className="settings-section">
      <h2>Pipeline</h2>
      <p className="settings-note">Stored in aster.yaml, so the terminal and editors read the same values. Empty resets a key.</p>
      <div className="settings-card">
        {KEYS.map(({ key, label, help }) => (
          <SettingsRow key={key} label={label} help={help}>
            <input
              className="field-input"
              data-mono="true"
              spellCheck={false}
              value={values[key] ?? ""}
              placeholder={key}
              aria-label={label}
              onChange={(e) => setValues((v) => ({ ...v, [key]: e.target.value }))}
              onBlur={() => save(key)}
            />
          </SettingsRow>
        ))}
      </div>
    </div>
  );
}
