import type { ConfigKey, ConfigValue } from "../../src/protocol";

/** A nav entry. `group` matches `aster config list`'s own group titles, so the
 *  rail follows the CLI rather than a second opinion about what goes together;
 *  the two synthetic ids own rows the config file does not hold. */
export interface Section {
  id: string;
  label: string;
  blurb: string;
  group?: string;
}

export const SECTIONS: Section[] = [
  {
    id: "model",
    label: "Model",
    blurb: "What Aster talks to, and how hard it thinks",
    group: "Model and provider",
  },
  {
    id: "keys",
    label: "API keys",
    blurb: "Every key Aster reads, stored in .env files",
  },
  {
    id: "permissions",
    label: "Permissions",
    blurb: "What the agent may edit, read, and run",
    group: "Permissions",
  },
  { id: "agent", label: "Agent", blurb: "How far one turn may go", group: "Agent limits" },
  {
    id: "subagents",
    label: "Sub-agents",
    blurb: "The fan-out the agent tool is allowed",
    group: "Sub-agents",
  },
  { id: "review", label: "Code review", blurb: "The review pipeline, not chat", group: "Code review" },
  { id: "mcp", label: "MCP", blurb: "Tool servers, and how many the model sees", group: "MCP tools" },
  { id: "editor", label: "Editor", blurb: "Settings this extension keeps, not aster.yaml" },
];

export function keysFor(section: Section, keys: ConfigKey[]): ConfigKey[] {
  return section.group ? keys.filter((key) => key.group === section.group) : [];
}

/** The documented default, read back as the type its control expects. `aster
 *  config` writes defaults for prose rather than for parsing, so the words it
 *  uses for "unset" resolve to empty rather than to themselves. */
export function defaultValue(key: ConfigKey): ConfigValue {
  const text = key.default;
  switch (key.kind) {
    case "bool":
      return text === "true";
    case "number": {
      const parsed = Number.parseFloat(text);
      return Number.isFinite(parsed) ? parsed : null;
    }
    case "list":
      return text === "nothing" || text === "" ? [] : text.split(",").map((s) => s.trim());
    default:
      return text;
  }
}

/** What the control shows: this scope's own value when it sets one, else what
 *  the next turn would resolve to anyway. */
export function shownValue(key: ConfigKey, scopeValue: ConfigValue): ConfigValue {
  if (scopeValue !== null) return scopeValue;
  return key.value !== null ? key.value : defaultValue(key);
}

/** A key matches when every word of the query appears somewhere in it, so
 *  "deny rule" finds `permissions.deny` without the words being adjacent. */
export function matches(key: ConfigKey, query: string): boolean {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  const hay = `${key.label} ${key.key} ${key.help} ${key.group}`.toLowerCase();
  return words.every((word) => hay.includes(word));
}

/** Searching crosses sections: what you typed is rarely in the one you had
 *  open, so results come back grouped by the section they came from. */
export function search(keys: ConfigKey[], query: string): { section: Section; keys: ConfigKey[] }[] {
  return SECTIONS.map((section) => ({
    section,
    keys: keysFor(section, keys).filter((key) => matches(key, query)),
  })).filter((group) => group.keys.length > 0);
}

export function asList(value: ConfigValue): string[] {
  if (Array.isArray(value)) return value;
  if (typeof value === "string" && value.length > 0) return value.split(",").map((s) => s.trim());
  return [];
}
