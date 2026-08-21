import { describe, expect, it } from "vitest";
import type { ConfigKey } from "../../src/protocol";
import { defaultValue, matches, search, shownValue } from "./sections";

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
  it("reads the words the CLI uses for an unset list as empty", () => {
    expect(defaultValue(key({ key: "permissions.allow", kind: "list", default: "nothing" }))).toEqual([]);
  });

  it("parses a documented bool and number", () => {
    expect(defaultValue(key({ key: "review.web_search", kind: "bool", default: "false" }))).toBe(false);
    expect(defaultValue(key({ key: "agents.max_concurrent", kind: "number", default: "8" }))).toBe(8);
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
