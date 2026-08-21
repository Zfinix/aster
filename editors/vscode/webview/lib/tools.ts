import type { ToolCall } from "./thread";

/** Verb plus the salient argument, split so the argument can be styled apart. */
export interface ToolDescription {
  /** Absent when the detail is a whole sentence that reads worse behind a verb. */
  verb?: string;
  detail?: string;
  /** Rendered monospace: a path or glob rather than prose. */
  code?: boolean;
}

/** Arguments stream in, so a partial parse is normal rather than an error. */
function args(call: ToolCall): Record<string, unknown> {
  try {
    const parsed = JSON.parse(call.arguments) as unknown;
    return typeof parsed === "object" && parsed !== null ? (parsed as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

function arg(call: ToolCall, key: string): string | undefined {
  const value = args(call)[key];
  return typeof value === "string" && value.trim() ? value : undefined;
}

/** `explore` and `update_plan` both carry their work in a `steps` array. */
function steps(call: ToolCall): unknown[] {
  const value = args(call).steps;
  return Array.isArray(value) ? value : [];
}

/** Tools whose id does not say what they do. Both search backends read the
 *  same on purpose: which provider served it is not the reader's concern. */
const TOOL_NAMES: Record<string, string> = {
  "websearch/search": "Web Search",
  "websearch/fetch_content": "Web Fetch",
  "web/search": "Web Search",
  "web/extract": "Web Fetch",
  "web/crawl": "Web Crawl",
  "web/sitemap": "Sitemap",
  "web/screenshot": "Screenshot",
};

/** Past this the server has crowded out the verb, so the verb wins. */
const LABEL_MAX = 24;

function titleCase(segment: string): string {
  return segment
    .split(/[_-]+/)
    .filter(Boolean)
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
}

/**
 * A tool id as a person would say it: `linear/save_issue` reads "Linear Save
 * Issue". The server stays, since two servers often carry the same verb, but it
 * is dropped once the pair grows too long to scan: `chrome-devtools/
 * take_screenshot` is "Take Screenshot", not its full address.
 */
export function humanize(name: string): string {
  const parts = name.split("/").map((part) => part.trim()).filter(Boolean);
  const tool = parts.pop() ?? name;
  // A plugin server is keyed `<plugin>/<server>`, which repeats a name as often
  // as not, so only the innermost distinct one is the server here.
  const server = parts.filter((part, i) => part !== parts[i - 1]).pop();

  const named = TOOL_NAMES[name] ?? (server && TOOL_NAMES[`${server}/${tool}`]);
  if (named) return named;

  const label = titleCase(tool) || name;
  if (!server) return label;
  const full = `${titleCase(server)} ${label}`;
  return full.length <= LABEL_MAX ? full : label;
}

/** Keys worth putting in a header, most identifying first. A label the tool
 *  wrote for itself leads: it is the only argument meant to be read. */
const SALIENT = [
  "title",
  "label",
  "description",
  "summary",
  "query",
  "url",
  "path",
  "command",
  "name",
  "id",
  "prompt",
  "message",
  "text",
];

/** Past this an argument has stopped naming the call and started reciting it. */
const DETAIL_MAX = 72;

/** An unnamed argument this long is the call's payload, not a name for it. */
const PAYLOAD_MIN = 200;

/** One line, capped. A header that wraps is no longer a header. */
function asLabel(value: string): string {
  const line = value.split("\n", 1)[0].replace(/\s+/g, " ").trim();
  return line.length <= DETAIL_MAX ? line : `${line.slice(0, DETAIL_MAX - 1).trimEnd()}…`;
}

/** The one argument that says what an MCP call was about. */
function salientArg(values: Record<string, unknown>): string | undefined {
  const strings = (key: string) => {
    const value = values[key];
    return typeof value === "string" && value.trim() ? value.trim() : undefined;
  };
  for (const key of SALIENT) {
    const found = strings(key);
    if (found) return asLabel(found);
  }
  // Nothing named itself, so what is left has to earn the header. A script or a
  // paragraph is not a name for what ran, and smeared across the row it buries
  // the tool that did run; the humanized tool name reads better than either. An
  // unbroken value is still likely an identifier, so only a payload is refused.
  const spare = Object.keys(values).map(strings).find(Boolean);
  if (!spare || spare.includes("\n") || spare.length > PAYLOAD_MIN) return undefined;
  return asLabel(spare);
}

/** One hit from an `aster_mcp` search, which the model reads as JSON and the
 *  reader is better served seeing as a list of tools it found. */
export type McpMatch = { id: string; server: string; name: string; description: string };

/** The matches a `search` call returned, or undefined when this is not a search
 *  or the payload is not the shape we expect. */
export function mcpMatches(call: ToolCall): McpMatch[] | undefined {
  if (call.name !== "aster_mcp" || args(call).action !== "search" || call.error) return undefined;
  const raw = call.result?.trim();
  if (!raw || !raw.startsWith("{")) return undefined;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return undefined;
  }
  const list = (parsed as { matches?: unknown }).matches;
  if (!Array.isArray(list)) return undefined;
  const matches = list.filter(
    (m): m is McpMatch =>
      typeof m === "object" && m !== null && typeof (m as McpMatch).id === "string"
  );
  return matches.length ? matches : undefined;
}

/** The MCP tool an `execute` call names, e.g. `websearch/search`. */
export function mcpTarget(call: ToolCall): string | undefined {
  return call.name === "aster_mcp" && args(call).action === "execute"
    ? arg(call, "name")
    : undefined;
}

const SHELLS = new Set(["bash", "sh", "zsh", "dash", "fish"]);

/** The command as the user would type it: shell wrappers (`bash -lc`) elided. */
function commandLine(call: ToolCall): string | undefined {
  const binary = arg(call, "command");
  if (!binary) return undefined;
  const list = args(call).args;
  const rest = Array.isArray(list) ? list.filter((a): a is string => typeof a === "string") : [];
  if (SHELLS.has(binary)) {
    const script = rest.filter((a) => !/^-[a-z]+$/i.test(a));
    if (script.length) return script.join(" ");
  }
  return [binary, ...rest].join(" ");
}

/**
 * The result without the stdout/stderr/exit markers the model needs but the
 * reader does not. A nonzero exit code is kept: that line is the story.
 */
export function displayOutput(call: ToolCall): string | undefined {
  // Agent reports render in the `agents` block; the raw JSON is for the model.
  if (call.name === "agent") return undefined;
  const raw = call.result?.trim();
  if (!raw) return undefined;
  if (call.name !== "run_command" && call.name !== "run_tests") return raw;
  const cleaned = raw
    .split("\n")
    .filter((line) => line !== "stdout:" && line !== "stderr:" && line !== "exit code: 0")
    .join("\n")
    .trim();
  return cleaned || undefined;
}


/**
 * The command a step ran, for the card's `in` cell. Long command lines belong in
 * a block the reader can scan, not smeared across the header.
 */
export function toolInput(call: ToolCall): string | undefined {
  return call.name === "run_command" || call.name === "run_tests"
    ? commandLine(call)
    : undefined;
}

/** A short name for the scratch tab a step's output opens in. It is the whole
 *  tab label, so it stays a word or two rather than a description. */
export function outputTitle(call: ToolCall): string {
  const path = toolPath(call);
  if (path) return path.split(/[/\\]/).pop() ?? "output";
  const name = mcpTarget(call)?.split("/").pop() ?? call.name;
  return name.replace(/^(run|read)_/, "").replace(/_/g, "-") || "output";
}

/** The workspace path a step touched, when it has one that an editor can open. */
export function toolPath(call: ToolCall): string | undefined {
  return call.name === "read_file" || call.name === "edit_file" ? arg(call, "path") : undefined;
}

export function describeTool(call: ToolCall): ToolDescription {
  switch (call.name) {
    case "run_command": {
      // The model's own summary stands alone: the terminal icon already says a
      // command ran, and "Ran Rebuild the bundle" doubles the verb.
      const summary = arg(call, "description");
      if (summary) {
        // Strip a leading "Ran "/"Run "/"Running " the model often includes,
        // since the terminal icon already conveys the verb.
        const stripped = summary.replace(/^(?:Ran|Run|Running)\s+/i, "");
        return { detail: stripped };
      }
      return {
        verb: call.result === undefined ? "Run" : "Ran",
        detail: commandLine(call),
        code: true,
      };
    }
    case "run_tests": {
      const filter = arg(call, "filter");
      return {
        verb: call.result === undefined ? "Run" : "Ran",
        detail: filter ? `tests matching ${filter}` : "the test suite",
        code: Boolean(filter),
      };
    }
    case "explore": {
      // One call, several lookups; naming them all would out-shout the row, so
      // the header counts them and the output lists them.
      const count = steps(call).length;
      return { verb: "Explore", detail: count ? `${count} lookups` : undefined };
    }
    case "find_files":
      return { verb: "Find", detail: arg(call, "pattern"), code: true };
    case "read_file":
      return { verb: "Read", detail: arg(call, "path"), code: true };
    case "list_files":
      return { verb: "List", detail: arg(call, "dir") ?? "repo root", code: true };
    case "search_files":
      return { verb: "Search", detail: arg(call, "query") };
    case "edit_file":
      return { verb: "Edit", detail: arg(call, "path"), code: true };
    case "open_preview":
      return { verb: "Opened", detail: arg(call, "target"), code: true };
    case "remember":
      return { verb: "Remember", detail: arg(call, "fact") };
    case "recall":
      return { verb: "Recall", detail: arg(call, "query") };
    case "read_skill":
      return { verb: "Skill", detail: arg(call, "name"), code: true };
    case "update_plan":
      return { verb: "Plan", detail: `${steps(call).length} steps` };
    case "ask_user":
      return { verb: "Ask", detail: arg(call, "header") ?? arg(call, "question") };
    case "exit_plan_mode":
      return { detail: "Asked to leave plan mode" };
    case "agent": {
      const tasks = args(call).tasks;
      const names = Array.isArray(tasks)
        ? tasks
            .map((t) =>
              t && typeof t === "object" && typeof (t as { agent?: unknown }).agent === "string"
                ? ((t as { agent: string }).agent as string)
                : ""
            )
            .filter(Boolean)
        : [];
      const unique = [...new Set(names)];
      if (names.length === 0) return { verb: "Agent" };
      const prefix = names.length > 1 ? `×${names.length} ` : "";
      return { verb: "Agent", detail: unique.length === 1 ? `${prefix}${unique[0]}` : unique.join(", ") };
    }
    case "aster_mcp": {
      // The bridge is plumbing. A reader wants the tool it reached, not the
      // three-action wrapper it went through.
      switch (args(call).action) {
        case "search":
          return { verb: "Find tools", detail: arg(call, "query") };
        case "describe":
          return { verb: "Inspect", detail: arg(call, "name"), code: true };
        case "execute": {
          const target = arg(call, "name");
          if (!target) return { verb: "Run tool" };
          const values = args(call).arguments;
          const detail =
            typeof values === "object" && values !== null
              ? salientArg(values as Record<string, unknown>)
              : undefined;
          return { verb: humanize(target), detail };
        }
        default:
          return { verb: "MCP" };
      }
    }
    default:
      return { verb: humanize(call.name) };
  }
}

/**
 * A one-line result gloss, so a collapsed row still says what happened. Counting
 * lines beats echoing the head of the output, which is usually a path or a brace.
 */
export function resultHint(call: ToolCall): string | undefined {
  const result = call.result;
  if (result === undefined) return undefined;
  if (call.error) return "failed";

  const trimmed = displayOutput(call) ?? "";
  if (!trimmed) return "empty";
  if (trimmed === "no matches") return "no matches";

  // A search's payload is JSON, so its line count says nothing. Count what it
  // actually found.
  const found = mcpMatches(call);
  if (found) return `${found.length} ${found.length === 1 ? "tool" : "tools"}`;

  const lines = trimmed.split("\n").length;
  switch (call.name) {
    case "search_files":
      return `${lines} ${lines === 1 ? "match" : "matches"}`;
    case "find_files":
      return `${lines} ${lines === 1 ? "file" : "files"}`;
    case "list_files":
      return `${lines} ${lines === 1 ? "entry" : "entries"}`;
    case "read_file":
      return `${lines} ${lines === 1 ? "line" : "lines"}`;
    default:
      return lines > 1 ? `${lines} lines` : undefined;
  }
}

/** A run of consecutive calls to the same tool, folded behind one header. */
export interface ToolRun {
  id: string;
  name: string;
  calls: ToolCall[];
}

/** Below this a run is shorter than the header that would hide it. */
const RUN_MIN = 3;

export function isRun(item: ToolCall | ToolRun): item is ToolRun {
  return "calls" in item;
}

/**
 * Folds consecutive calls to the same tool into runs, so eighteen reads read as
 * one line. Mixed sequences are left alone: the interleaving is the story.
 */
export function groupRuns(calls: ToolCall[]): (ToolCall | ToolRun)[] {
  const out: (ToolCall | ToolRun)[] = [];
  let i = 0;

  while (i < calls.length) {
    let end = i + 1;
    while (end < calls.length && calls[end].name === calls[i].name) end++;

    if (end - i >= RUN_MIN) {
      out.push({ id: `run-${calls[i].id}`, name: calls[i].name, calls: calls.slice(i, end) });
    } else {
      out.push(...calls.slice(i, end));
    }
    i = end;
  }

  return out;
}

const RUN_NOUNS: Record<string, [string, string]> = {
  read_file: ["file", "files"],
  edit_file: ["file", "files"],
  list_files: ["directory", "directories"],
  // Not "searches": the verb is already Find/Search, and "Search 3 searches"
  // says one word twice. The noun names what was passed, not the act.
  find_files: ["pattern", "patterns"],
  search_files: ["query", "queries"],
  run_command: ["command", "commands"],
  run_tests: ["test run", "test runs"],
  read_skill: ["skill", "skills"],
  explore: ["batch", "batches"],
  aster_mcp: ["tool call", "tool calls"],
};

/** The header a folded run wears, e.g. "Read 6 files". */
export function runLabel(name: string, count: number): string {
  const verb = describeTool({ id: "", name, arguments: "{}" }).verb;
  const noun = RUN_NOUNS[name] ?? ["step", "steps"];
  return `${verb} ${count} ${count === 1 ? noun[0] : noun[1]}`;
}
