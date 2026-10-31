/** A turn-level failure, split into a plain-language label, a sentence saying
 *  what it means and what to do, and the raw detail, so the panel renders a
 *  readable boxed error instead of dumping the provider string verbatim. */
export interface ParsedError {
  label: string;
  hint?: string;
  detail: string;
}

interface Explanation {
  label: string;
  hint?: string;
}

const SIGN_IN: Explanation = {
  label: "Sign-in problem",
  hint: "Check your API key.",
};

const NOT_SIGNED_IN: Explanation = {
  label: "Not signed in",
  hint: "Sign in first.",
};

const NOT_CONFIGURED = /not signed in to chatgpt|no api key found/i;

const CONNECTION: Explanation = {
  label: "Connection dropped",
};

const PROVIDER_DOWN: Explanation = {
  label: "Provider trouble",
  hint: "Try again in a moment.",
};

const UNREACHABLE: Explanation = {
  label: "Can't reach the provider",
  hint: "Check your connection.",
};

const STATUS_LABELS: Record<string, Explanation> = {
  "400": {
    label: "Request rejected",
    hint: "The provider said no.",
  },
  "401": SIGN_IN,
  "403": SIGN_IN,
  "404": {
    label: "Model not found",
    hint: "Pick another model.",
  },
  "408": CONNECTION,
  "429": {
    label: "Too many requests",
    hint: "Wait a moment, then retry.",
  },
  "500": PROVIDER_DOWN,
  "502": PROVIDER_DOWN,
  "503": PROVIDER_DOWN,
  "529": PROVIDER_DOWN,
};

const DEGENERATE = /repeating itself|degenerated|degenerate output/i;

const NETWORKISH = /timed out|time out|connection|network|dropped mid-reply|decoding response body|stream chunk/i;

const UNREACHABLE_ISH = /dns error|failed to lookup address|nodename nor servname|client error \(connect\)|connection refused|unreachable/i;

const EXIT_PREFIX = /^aster(?: \w+)? exited with code \d+[.:]?\s*/i;

const STATUS_ANYWHERE = /\b(?:status|returned|error|code|http)\D{0,4}\b(400|401|403|404|408|429|500|502|503|529)\b/i;

export function parseError(message: string): ParsedError {
  const stripped = message.replace(EXIT_PREFIX, "");
  const wrapped = stripped !== message;

  const match = /^([a-z ]+)\((\d{3})\):\s*(.*)$/s.exec(stripped);
  if (match) {
    const [, , code, detail] = match;
    const known = STATUS_LABELS[code];
    return {
      label: known?.label ?? `Error ${code}`,
      hint: known?.hint,
      detail: detail || stripped,
    };
  }

  if (NOT_CONFIGURED.test(stripped)) {
    return { label: NOT_SIGNED_IN.label, hint: NOT_SIGNED_IN.hint, detail: stripped };
  }

  // The broken attempt was never shown, so the label is all the reader needs.
  if (DEGENERATE.test(stripped)) {
    return { label: "Reply broke", detail: stripped };
  }

  // Transport failures first: a dropped stream often quotes a status from an
  // earlier retry, and the connection advice is the one that helps.
  if (NETWORKISH.test(stripped)) {
    return { label: CONNECTION.label, hint: CONNECTION.hint, detail: stripped };
  }

  if (UNREACHABLE_ISH.test(stripped)) {
    return { label: UNREACHABLE.label, hint: UNREACHABLE.hint, detail: stripped };
  }

  const embedded = STATUS_ANYWHERE.exec(stripped);
  if (embedded && STATUS_LABELS[embedded[1]]) {
    const { label, hint } = STATUS_LABELS[embedded[1]];
    return { label, hint, detail: stripped };
  }

  if (wrapped) {
    const detail = /^See the (?:Aster output channel|terminal running aster serve)\.?\s*$/i.test(stripped)
      ? ""
      : stripped;
    return {
      label: "Aster stopped",
      hint: "Send your message again.",
      detail,
    };
  }

  if (/^nothing to compact yet$/i.test(stripped)) {
    return {
      label: "Nothing to compact",
      hint: "The conversation is still short.",
      detail: "",
    };
  }

  return { label: "Something went wrong", detail: message };
}
