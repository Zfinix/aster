import { useEffect, useState } from "react";
import { configList, configSet, type ConfigValue } from "../lib/aster";
import { useToast } from "./chrome";

/** Render one resolved config value back into its text form. Lists are comma
 *  separated, everything else is its scalar; unset (null) is blank. */
function toText(value: ConfigValue): string {
  if (value == null) return "";
  if (Array.isArray(value)) return value.join(", ");
  return String(value);
}

const REVIEW_KEYS = [
  "review.model",
  "review.hypothesis_model",
  "review.verify_model",
  "review.min_confidence",
  "review.max_diff_bytes",
  "review.analyzers",
  "review.focus_areas",
  "review.include",
  "review.exclude",
  "review.effort",
  "review.web_search",
] as const;

const LABELS: Record<string, string> = {
  "review.model": "Model",
  "review.hypothesis_model": "First-pass model",
  "review.verify_model": "Verify model",
  "review.min_confidence": "Confidence floor (0-1)",
  "review.max_diff_bytes": "Largest diff (bytes)",
  "review.analyzers": "Static analyzers (comma separated)",
  "review.focus_areas": "Focus areas (comma separated)",
  "review.include": "Only review (globs, comma separated)",
  "review.exclude": "Never review (globs, comma separated)",
  "review.effort": "Reasoning effort (off/low/medium/high)",
  "review.web_search": "Web search (true/false)",
};

/** Review pipeline settings, read and written through `aster config` so the
 *  desktop, terminal, and editors share one `aster.yaml`. */
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
        for (const e of entries) {
          if ((REVIEW_KEYS as readonly string[]).includes(e.key)) {
            next[e.key] = toText(e.value);
          }
        }
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
      toast("Review setting saved");
    } catch (e) {
      toast(`Save failed: ${String(e)}`);
    }
  };

  if (!loaded) return null;

  return (
    <div className="settings-section">
      <div className="menu-label">Code review</div>
      {REVIEW_KEYS.map((key) => (
        <label className="menu-field" key={key}>
          <span className="menu-field-label">{LABELS[key]}</span>
          <input
            className="menu-input"
            spellCheck={false}
            value={values[key] ?? ""}
            placeholder={key}
            onChange={(e) =>
              setValues((v) => ({ ...v, [key]: e.target.value }))
            }
            onBlur={() => save(key)}
          />
        </label>
      ))}
      <div className="menu-hint">Empty clears a key back to its default.</div>
    </div>
  );
}
