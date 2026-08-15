import { describe, expect, it } from "vitest";
import type { ToolCall } from "./thread";
import {
  describeTool,
  displayOutput,
  groupRuns,
  isRun,
  resultHint,
  outputTitle,
  humanize,
  runLabel,
  toolInput,
  toolPath,
} from "./tools";

let ids = 0;
const call = (name: string, args: unknown, result?: string, error?: boolean): ToolCall => ({
  id: `c${ids++}`,
  name,
  arguments: typeof args === "string" ? args : JSON.stringify(args),
  result,
  error,
});

describe("describeTool", () => {
  it("spells out the whole command line in the header", () => {
    const run = call("run_command", { command: "cargo", args: ["test", "--all"] });
    expect(describeTool(run)).toEqual({ verb: "Run", detail: "cargo test --all", code: true });
  });

  it("lets the model's summary stand in for the verb", () => {
    const run = call("run_command", {
      command: "bun",
      args: ["run", "build"],
      description: "Rebuild the webview bundle",
    });
    expect(describeTool(run)).toEqual({ detail: "Rebuild the webview bundle" });
  });

  it("falls back to the command line when no summary came with the call", () => {
    const run = call("run_command", { command: "bun", args: ["run", "build"] });
    expect(describeTool(run).verb).toBe("Run");
  });

  it("elides the shell wrapper, showing the command the user would type", () => {
    const run = call("run_command", { command: "bash", args: ["-lc", "cargo build | tail -5"] });
    expect(describeTool(run).detail).toBe("cargo build | tail -5");
  });

  it("says Ran once the command has finished", () => {
    const run = call("run_command", { command: "cargo", args: ["fmt"] }, "done");
    expect(describeTool(run).verb).toBe("Ran");
  });

  it("labels a file search by its pattern", () => {
    expect(describeTool(call("find_files", { pattern: "*.rs" }))).toEqual({
      verb: "Find",
      detail: "*.rs",
      code: true,
    });
  });

  it("reads a tool it does not know as words, not as an identifier", () => {
    expect(describeTool(call("frobnicate", {}))).toEqual({ verb: "Frobnicate" });
    expect(describeTool(call("fetch_content", {}))).toEqual({ verb: "Fetch Content" });
  });

  it("counts an explore batch rather than naming every lookup", () => {
    const explore = call("explore", {
      steps: [
        { tool: "read_file", args: { path: "src/main.rs" } },
        { tool: "search_files", args: { query: "spawn" } },
      ],
    });
    expect(describeTool(explore)).toEqual({ verb: "Explore", detail: "2 lookups" });
  });

  it("names the whole suite when run_tests carries no filter", () => {
    expect(describeTool(call("run_tests", {}, "ok"))).toEqual({
      verb: "Ran",
      detail: "the test suite",
      code: false,
    });
  });

  it("prefers the header of a question over its body", () => {
    const ask = call("ask_user", { header: "Storage", question: "Where should this live?" });
    expect(describeTool(ask)).toEqual({ verb: "Ask", detail: "Storage" });
  });

  it("survives arguments that are still streaming in", () => {
    expect(describeTool(call("read_file", '{"path": "cra'))).toEqual({
      verb: "Read",
      detail: undefined,
      code: true,
    });
  });

  it("treats a blank argument as absent", () => {
    expect(describeTool(call("read_file", { path: "   " })).detail).toBeUndefined();
  });
});

describe("displayOutput", () => {
  it("strips the stream markers and a clean exit from a command result", () => {
    const run = call("run_command", { command: "cargo" }, "stdout:\nok\n\nexit code: 0");
    expect(displayOutput(run)).toBe("ok");
  });

  it("keeps a nonzero exit code, which is the story", () => {
    const run = call("run_command", { command: "cargo" }, "stderr:\nboom\n\nexit code: 101");
    expect(displayOutput(run)).toBe("boom\n\nexit code: 101");
  });

  it("treats a marker-only result as no output", () => {
    const run = call("run_command", { command: "true" }, "\nexit code: 0");
    expect(displayOutput(run)).toBeUndefined();
  });

  it("leaves other tools' results verbatim", () => {
    expect(displayOutput(call("read_file", { path: "a.rs" }, "stdout: is a word"))).toBe(
      "stdout: is a word"
    );
  });
});

describe("resultHint", () => {
  it("says nothing while the tool is still running", () => {
    expect(resultHint(call("read_file", { path: "a.rs" }))).toBeUndefined();
  });

  it("reports a failure over a line count", () => {
    expect(resultHint(call("run_command", { command: "cargo" }, "boom", true))).toBe("failed");
  });

  it("counts found files", () => {
    expect(resultHint(call("find_files", { pattern: "*.rs" }, "a.rs\nb.rs"))).toBe("2 files");
  });

  it("singularises a lone match", () => {
    expect(resultHint(call("search_files", { query: "fn" }, "one hit"))).toBe("1 match");
  });

  it("marks an empty result", () => {
    expect(resultHint(call("run_command", { command: "true" }, "  \n "))).toBe("empty");
  });

  it("leaves a one-line result from an unknown tool unglossed", () => {
    expect(resultHint(call("frobnicate", {}, "fine"))).toBeUndefined();
  });
});

describe("toolPath", () => {
  it("offers the path of a step an editor can open", () => {
    expect(toolPath(call("read_file", { path: "src/main.rs" }))).toBe("src/main.rs");
  });

  it("offers nothing for a step with no file behind it", () => {
    expect(toolPath(call("run_command", { command: "cargo" }))).toBeUndefined();
  });
});

describe("toolInput", () => {
  it("gives the command a block of its own", () => {
    const run = call("run_command", { command: "bash", args: ["-lc", "gh pr review 732 --comment"] });
    expect(toolInput(run)).toBe("gh pr review 732 --comment");
  });

  it("gives nothing for a step that ran no command", () => {
    expect(toolInput(call("read_file", { path: "src/main.rs" }))).toBeUndefined();
  });
});

describe("outputTitle", () => {
  it("names a scratch tab after the tool, not the whole command", () => {
    expect(outputTitle(call("run_command", { command: "gh", args: ["pr", "list"] }))).toBe(
      "command"
    );
    expect(outputTitle(call("search_files", { query: "toolPath" }))).toBe("search-files");
  });

  it("prefers the file's own name when the step touched one", () => {
    expect(outputTitle(call("read_file", { path: "crates/aster-cli/src/chat.rs" }))).toBe(
      "chat.rs"
    );
  });
});

describe("groupRuns", () => {
  it("folds a repeated tool into one run", () => {
    const calls = [
      call("read_file", { path: "a.rs" }),
      call("read_file", { path: "b.rs" }),
      call("read_file", { path: "c.rs" }),
    ];
    const [run] = groupRuns(calls);
    expect(isRun(run) && run.calls).toHaveLength(3);
  });

  it("leaves a pair alone, since the header would cost more than it hides", () => {
    const calls = [call("read_file", { path: "a.rs" }), call("read_file", { path: "b.rs" })];
    expect(groupRuns(calls).every((item) => !isRun(item))).toBe(true);
  });

  it("keeps interleaved steps in order rather than gathering by tool", () => {
    const calls = [
      call("read_file", { path: "a.rs" }),
      call("run_command", { command: "cargo" }),
      call("read_file", { path: "b.rs" }),
    ];
    expect(groupRuns(calls).map((item) => isRun(item))).toEqual([false, false, false]);
  });

  it("folds each run separately when two runs sit back to back", () => {
    const calls = [
      ...Array.from({ length: 3 }, () => call("read_file", { path: "a.rs" })),
      ...Array.from({ length: 3 }, () => call("search_files", { query: "fn" })),
    ];
    const grouped = groupRuns(calls);
    expect(grouped).toHaveLength(2);
    expect(grouped.map((item) => item.name)).toEqual(["read_file", "search_files"]);
  });
});

describe("runLabel", () => {
  it("names the tool and counts what it touched", () => {
    expect(runLabel("read_file", 6)).toBe("Read 6 files");
  });

  it("falls back to steps for a tool it does not know", () => {
    expect(runLabel("frobnicate", 4)).toBe("Frobnicate 4 steps");
  });

  it("counts bridged MCP work as tool calls", () => {
    expect(runLabel("aster_mcp", 3)).toBe("MCP 3 tool calls");
  });
});

describe("humanize", () => {
  it("reads a tool id as a person would say it, server and all", () => {
    expect(humanize("linear/save_issue")).toBe("Linear Save Issue");
    expect(humanize("railway/list-branches")).toBe("Railway List Branches");
    expect(humanize("fetch_content")).toBe("Fetch Content");
  });

  it("drops the server once the pair is too long to scan", () => {
    expect(humanize("chrome-devtools/take_screenshot")).toBe("Take Screenshot");
    expect(humanize("heroui-native/get_component_docs")).toBe("Get Component Docs");
  });

  it("does not say a plugin server's name twice", () => {
    expect(humanize("websearch/websearch/search")).toBe("Web Search");
    expect(humanize("demo/demo/save_issue")).toBe("Demo Save Issue");
  });

  it("gives the web tools names that say what they do", () => {
    expect(humanize("websearch/search")).toBe("Web Search");
    expect(humanize("websearch/fetch_content")).toBe("Web Fetch");
    expect(humanize("web/extract")).toBe("Web Fetch");
    expect(humanize("web/search")).toBe("Web Search");
  });

  it("leaves a name it cannot improve alone", () => {
    expect(humanize("search")).toBe("Search");
    expect(humanize("")).toBe("");
  });
});

describe("describeTool on the MCP bridge", () => {
  it("names the tool an execute reached, not the bridge", () => {
    const run = call("aster_mcp", {
      action: "execute",
      name: "websearch/search",
      arguments: { query: "agent client protocol" },
    });
    expect(describeTool(run)).toEqual({
      verb: "Web Search",
      detail: "agent client protocol",
    });
  });

  it("falls back to any string argument when it knows no salient key", () => {
    const run = call("aster_mcp", {
      action: "execute",
      name: "railway/deploy",
      arguments: { serviceName: "api" },
    });
    expect(describeTool(run)).toEqual({ verb: "Railway Deploy", detail: "api" });
  });

  it("says what a discovery call was looking for", () => {
    const run = call("aster_mcp", { action: "search", query: "web search" });
    expect(describeTool(run)).toEqual({ verb: "Find tools", detail: "web search" });
  });

  it("shows the tool id for a describe, monospaced", () => {
    const run = call("aster_mcp", { action: "describe", name: "linear/save_issue" });
    expect(describeTool(run)).toEqual({
      verb: "Inspect",
      detail: "linear/save_issue",
      code: true,
    });
  });

  it("stays readable while the arguments are still streaming", () => {
    expect(describeTool(call("aster_mcp", '{"action":"exec'))).toEqual({ verb: "MCP" });
    expect(describeTool(call("aster_mcp", { action: "execute" }))).toEqual({ verb: "Run tool" });
  });
});
