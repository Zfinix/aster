/** Reading the ACP wire. A `ToolCallUpdate` carries its fields flattened next
 *  to the tool call id rather than nested, and content blocks arrive either
 *  bare or wrapped in a tool-call content envelope. */

export interface ToolCallWire {
  toolCallId?: string;
  name?: string;
  kind?: string;
  title?: string;
  status?: string;
  content?: unknown;
  rawInput?: unknown;
  rawOutput?: unknown;
}

export function contentText(content: unknown): string {
  if (!Array.isArray(content)) {
    return blockText(content);
  }
  return content.map((block) => blockText(block)).join("");
}

function blockText(block: unknown): string {
  if (typeof block === "string") {
    return block;
  }
  if (!block || typeof block !== "object") {
    return "";
  }
  const value = block as Record<string, unknown>;
  if (typeof value["text"] === "string") {
    return value["text"];
  }
  if (value["content"] !== undefined) {
    return contentText(value["content"]);
  }
  if (typeof value["newText"] === "string") {
    return value["newText"];
  }
  return "";
}

export interface ToolResultWire {
  id: string;
  result: string;
  error: boolean;
}

/** Only a finished call closes its row, so a status-only update yields nothing. */
export function toolResult(update: ToolCallWire): ToolResultWire | null {
  if (update.status !== "completed" && update.status !== "failed") {
    return null;
  }
  const output = update.rawOutput;
  return {
    id: String(update.toolCallId ?? ""),
    result:
      typeof output === "string"
        ? output
        : output === undefined
          ? contentText(update.content)
          : JSON.stringify(output),
    error: update.status === "failed",
  };
}

/** The path a file edit touched, for the panel's edit log. */
export function editedPath(update: ToolCallWire): string | null {
  if (update.name !== "edit_file" || !update.rawInput || typeof update.rawInput !== "object") {
    return null;
  }
  const path = (update.rawInput as { path?: unknown }).path;
  return typeof path === "string" && path ? path : null;
}
