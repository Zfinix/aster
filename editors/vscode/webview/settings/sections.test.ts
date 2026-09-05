import { describe, expect, it } from "vitest";
import type { ConfigKey, EnvVar } from "../../src/protocol";
import {
  SECTIONS,
  defaultValue,
  envGroups,
  homeFrom,
  matches,
  partsFor,
  search,
  shortPath,
  shownValue,
} from "./sections";

function key(over: Partial<ConfigKey> & { key: string }): ConfigKey {
  return {
    label: over.key,
    group: "Model and provider",
    kind: "text",
    choices: [],
    unit: "none",
    value: null,
    display: "",
    default: "",
    source: "default",
    shadowed: null,
    scopes: { global: null, local: null },
    env: [],
    help: "",
    ...over,
  };
}

describe("matches", () => {
  const deny = key({
    key: "permissions.deny",
    label: "Never allow",
    group: "Permissions",
    help: "Rules refused outright, e.g. Edit(infra/**)",
  });

  it("finds a key by its dotted name", () => {
    expect(matches(deny, "permissions.deny")).toBe(true);
  });

  it("finds a key by words that are not adjacent", () => {
    expect(matches(deny, "deny rule")).toBe(true);
  });

  it("finds a key by its label when the name shares no words", () => {
    expect(matches(deny, "never allow")).toBe(true);
  });

  it("rejects a query only partly present", () => {
    expect(matches(deny, "deny sandbox")).toBe(false);
  });

  it("treats an empty query as matching everything", () => {
    expect(matches(deny, "   ")).toBe(true);
  });
});

describe("search", () => {
  it("groups results by the section each key came from", () => {
    const keys = [
      key({ key: "review.model", label: "Model", group: "Model and provider" }),
      key({ key: "agents.collector_model", label: "Collector model", group: "Sub-agents" }),
    ];
    const groups = search(keys, "model");
    expect(groups.map((g) => g.section.id)).toEqual(["model", "subagents"]);
    expect(groups[0].keys).toHaveLength(1);
  });

  it("drops sections with no match rather than listing them empty", () => {
    const keys = [key({ key: "review.model", label: "Model" })];
    expect(search(keys, "mcp")).toEqual([]);
  });
});

describe("defaultValue", () => {
  it("reads a list's prose default as empty rather than as an entry", () => {
    expect(defaultValue(key({ key: "permissions.allow", kind: "list", default: "nothing" }))).toEqual([]);
    expect(
      defaultValue(key({ key: "permissions.additional_directories", kind: "list", default: "the repo only" }))
    ).toEqual([]);
  });

  it("parses a documented bool and number", () => {
    expect(defaultValue(key({ key: "review.web_search", kind: "bool", default: "false" }))).toBe(false);
    expect(defaultValue(key({ key: "agents.max_concurrent", kind: "number", default: "8" }))).toBe(8);
  });
});

describe("partsFor", () => {
  const permissions = SECTIONS.find((s) => s.id === "permissions")!;
  const rows = (...names: string[]) => names.map((name) => key({ key: name, group: "Permissions" }));

  it("splits a section into its cards in the declared order", () => {
    const parts = partsFor(permissions, rows("permissions.deny", "permissions.mode", "permissions.allow"));
    expect(parts.map((p) => p.title)).toEqual([undefined, "Rules"]);
    expect(parts[1].keys.map((k) => k.key)).toEqual(["permissions.allow", "permissions.deny"]);
  });

  it("trails keys no part names in one last card", () => {
    const parts = partsFor(permissions, rows("permissions.mode", "permissions.new_thing"));
    expect(parts.at(-1)?.keys.map((k) => k.key)).toEqual(["permissions.new_thing"]);
  });

  it("keeps a section without parts as a single card", () => {
    const agent = SECTIONS.find((s) => s.id === "agent")!;
    const parts = partsFor(agent, [key({ key: "agent.max_tool_rounds", group: "Agent limits" })]);
    expect(parts).toHaveLength(1);
    expect(parts[0].title).toBeUndefined();
  });
});

describe("envGroups", () => {
  function env(over: Partial<EnvVar> & { var: string }): EnvVar {
    return {
      group: "Toggles",
      kind: "text",
      secret: false,
      set: false,
      source: "unset",
      masked: null,
      value: null,
      help: "",
      ...over,
    };
  }

  it("keeps the CLI's group order and merges adjacent rows", () => {
    const groups = envGroups([
      env({ var: "ASTER_MAX_TOOL_ROUNDS", group: "Turns and limits" }),
      env({ var: "ASTER_COMMAND_TIMEOUT", group: "Turns and limits" }),
      env({ var: "ASTER_NO_BROWSER", group: "Toggles" }),
    ]);
    expect(groups.map((g) => g.group)).toEqual(["Turns and limits", "Toggles"]);
    expect(groups[0].vars).toHaveLength(2);
  });

  it("keeps a repeated group name apart when the CLI lists it twice", () => {
    const groups = envGroups([
      env({ var: "ASTER_A", group: "Toggles" }),
      env({ var: "ASTER_B", group: "Models" }),
      env({ var: "ASTER_C", group: "Toggles" }),
    ]);
    expect(groups.map((g) => g.group)).toEqual(["Toggles", "Models", "Toggles"]);
  });
});

describe("shownValue", () => {
  it("prefers what this scope sets over what won", () => {
    const row = key({ key: "review.model", value: "won/model", scopes: { global: "mine", local: null } });
    expect(shownValue(row, row.scopes.global)).toBe("mine");
  });

  it("falls back to the resolved value when this scope sets nothing", () => {
    const row = key({ key: "review.model", value: "won/model" });
    expect(shownValue(row, null)).toBe("won/model");
  });

  it("falls back to the default when nothing sets it at all", () => {
    const row = key({ key: "review.model", default: "openai/gpt-4o-mini" });
    expect(shownValue(row, null)).toBe("openai/gpt-4o-mini");
  });
});

describe("shortPath", () => {
  const home = "/Users/me";
  const root = "/Users/me/code/app";

  it("starts a workspace file at the folder's name", () => {
    expect(shortPath("/Users/me/code/app/aster.yaml", home, root)).toBe("app/aster.yaml");
  });

  it("starts a home file at ~", () => {
    expect(shortPath("/Users/me/.aster/aster.yaml", home, root)).toBe("~/.aster/aster.yaml");
  });

  it("leaves a path outside both alone, and a sibling of the root alone", () => {
    expect(shortPath("/etc/aster.yaml", home, root)).toBe("/etc/aster.yaml");
    expect(shortPath("/Users/me/code/apple/aster.yaml", null, root)).toBe("/Users/me/code/apple/aster.yaml");
  });

  it("reads the home directory off the global path", () => {
    expect(homeFrom("/Users/me/.aster/aster.yaml")).toBe("/Users/me");
    expect(homeFrom("~/.aster/aster.yaml")).toBe("~");
    expect(homeFrom(undefined)).toBeNull();
  });
});
