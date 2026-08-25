/**
 * A turn-level failure, split into a plain-language label, a sentence saying
 * what it means and what to do, and the raw detail, so the panel renders a
 * readable boxed error instead of dumping the provider string verbatim.
 */
export interface ParsedError {
  label: string;
  hint?: string;
  detail: string;
}

const STATUS_LABELS: Record<string, { label: string; hint: string }> = {
  "400": {
    label: "The request didn't go through",
    hint: "The model provider rejected it. The details below say why.",
  },
  "401": {
    label: "Sign-in problem",
    hint: "Aster couldn't authenticate with your model provider. Check your API key, or switch with /provider.",
  },
  "403": {
    label: "Sign-in problem",
    hint: "Aster couldn't authenticate with your model provider. Check your API key, or switch with /provider.",
  },
  "404": {
    label: "Model not found",
    hint: "This endpoint doesn't know the selected model. Pick another with /model.",
  },
  "429": {
    label: "Too many requests",
    hint: "The provider asked Aster to slow down. Give it a moment and send again. Nothing was lost.",
  },
};

/**
 * The CLI formats provider failures as `bad request (400): <detail>`. Pull the
 * code out for a label and keep the rest as the detail. Anything that does not
 * match that shape is shown whole, so we never mangle an unexpected message.
 */
/** Transport failures: stalls, drops, and timeouts that read as plumbing. */
const NETWORKISH = /timed out|time out|connection|network|dropped mid-reply|decoding response body|stream chunk/i;

export function parseError(message: string): ParsedError {
  const match = /^([a-z ]+)\((\d{3})\):\s*(.*)$/s.exec(message);
  if (!match) {
    if (NETWORKISH.test(message)) {
      return {
        label: "Connection dropped",
        hint: "The model provider stopped responding mid-reply. Alpha and preview models do this under load. Send again to retry, or pick a steadier model with /model.",
        detail: message,
      };
    }
    return { label: "Something went wrong", detail: message };
  }

  const [, , code, detail] = match;
  const known = STATUS_LABELS[code];
  return {
    label: known?.label ?? `Error ${code}`,
    hint: known?.hint,
    detail: detail || message,
  };
}
