import type { ConfigKey, ConfigValue, EnvVar } from "../../src/protocol";

/** A nav entry. `group` matches `aster config list`'s own group titles, so the
 *  rail follows the CLI rather than a second opinion about what goes together;
 *  the two synthetic ids own rows the config file does not hold. */
export interface Section {
  id: string;
  label: string;
  blurb: string;
  group?: string;
  parts?: SectionPart[];
}

/** A titled card within a section. Keys the parts do not name trail in one
 *  last card, so a key the CLI adds still shows up. */
export interface SectionPart {
  title?: string;
  keys: string[];
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
    parts: [
      { keys: ["permissions.mode"] },
      { title: "Rules", keys: ["permissions.allow", "permissions.ask", "permissions.deny"] },
      {
        title: "Access",
        keys: [
          "permissions.use_default_rules",
          "permissions.additional_directories",
          "permissions.allow_credentials",
        ],
      },
    ],
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
  { id: "display", label: "Display", blurb: "What chat prints on its own", group: "Display" },
  { id: "env", label: "Environment", blurb: "Every ASTER_ variable Aster reads, stored in .env files" },
  { id: "editor", label: "Editor", blurb: "Settings this extension keeps, not aster.yaml" },
];

export function keysFor(section: Section, keys: ConfigKey[]): ConfigKey[] {
  return section.group ? keys.filter((key) => key.group === section.group) : [];
}

export function partsFor(
  section: Section,
  keys: ConfigKey[]
): { title?: string; keys: ConfigKey[] }[] {
  const own = keysFor(section, keys);
  if (!section.parts) return [{ keys: own }];
  const byKey = new Map(own.map((key) => [key.key, key]));
  const out: { title?: string; keys: ConfigKey[] }[] = section.parts
    .map((part) => ({
      title: part.title,
      keys: part.keys.flatMap((name) => {
        const key = byKey.get(name);
        if (!key) return [];
        byKey.delete(name);
        return [key];
      }),
    }))
    .filter((part) => part.keys.length > 0);
  if (byKey.size > 0) out.push({ keys: [...byKey.values()] });
  return out;
}

/** The documented default, read back as the type its control expects. `aster
 *  config` writes defaults for prose rather than for parsing, so a list's
 *  default ("nothing", "the repo only") is a description, never an entry. */
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
      return [];
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

/** Env vars grouped by the CLI's own group titles, in the order it lists them. */
export function envGroups(vars: EnvVar[]): { group: string; vars: EnvVar[] }[] {
  const out: { group: string; vars: EnvVar[] }[] = [];
  for (const v of vars) {
    const last = out[out.length - 1];
    if (last && last.group === v.group) {
      last.vars.push(v);
    } else {
      out.push({ group: v.group, vars: [v] });
    }
  }
  return out;
}

/** A config path as the page shows it: inside the workspace it starts at the
 *  folder's name, under the home directory it starts at "~". */
export function shortPath(path: string, home: string | null, root: string | null): string {
  const under = (base: string) => path === base || path.startsWith(base + "/");
  if (root && under(root)) {
    const name = root.split("/").filter(Boolean).at(-1) ?? "";
    return name + path.slice(root.length);
  }
  if (home && under(home)) return "~" + path.slice(home.length);
  return path;
}

const GLOBAL_TAIL = "/.aster/aster.yaml";

/** The home directory, read off the global config path the CLI reported. */
export function homeFrom(globalPath: string | undefined): string | null {
  return globalPath?.endsWith(GLOBAL_TAIL) ? globalPath.slice(0, -GLOBAL_TAIL.length) : null;
}
