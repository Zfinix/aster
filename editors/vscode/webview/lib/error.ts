/**
 * A turn-level failure, split into a short human label and the raw detail, so
 * the panel can render a clean boxed error instead of dumping the provider
 * string verbatim.
 */
export interface ParsedError {
  label: string;
  detail: string;
}

const STATUS_LABELS: Record<string, string> = {
  "400": "Bad request",
  "401": "Authentication failed",
  "403": "Authentication failed",
  "404": "Not found",
  "429": "Rate limited",
};

/**
 * The CLI formats provider failures as `bad request (400): <detail>`. Pull the
 * code out for a label and keep the rest as the detail. Anything that does not
 * match that shape is shown whole, so we never mangle an unexpected message.
 */
export function parseError(message: string): ParsedError {
  const match = /^([a-z ]+)\((\d{3})\):\s*(.*)$/s.exec(message);
  if (!match) return { label: "Something went wrong", detail: message };

  const [, , code, detail] = match;
  return {
    label: STATUS_LABELS[code] ?? `Error ${code}`,
    detail: detail || message,
  };
}
