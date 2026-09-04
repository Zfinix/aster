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

interface Explanation {
  label: string;
  hint: string;
}

const SIGN_IN: Explanation = {
  label: "Sign-in problem",
  hint: "Aster couldn't authenticate with your model provider. Check your API key, or switch with /provider.",
};

const NOT_SIGNED_IN: Explanation = {
  label: "Not signed in",
  hint: "Sign in to your model provider, or add its API key, then send again.",
};

/** The turn never started: the CLI found no key and no login for the endpoint. */
const NOT_CONFIGURED = /not signed in to chatgpt|no api key found/i;

const CONNECTION: Explanation = {
  label: "Connection dropped",
  hint: "The model provider stopped responding mid-reply. Alpha and preview models do this under load. Send again to retry, or pick a steadier model with /model.",
};

const PROVIDER_DOWN: Explanation = {
  label: "Provider trouble",
  hint: "The model provider had an internal problem. These are usually brief. Send again in a moment.",
};

const UNREACHABLE: Explanation = {
  label: "Can't reach the provider",
  hint: "Aster couldn't connect to the model provider. Check your internet connection, then send again.",
};

const STATUS_LABELS: Record<string, Explanation> = {
  "400": {
    label: "The request didn't go through",
    hint: "The model provider rejected it. The details below say why.",
  },
  "401": SIGN_IN,
  "403": SIGN_IN,
  "404": {
    label: "Model not found",
    hint: "This endpoint doesn't know the selected model. Pick another with /model.",
  },
  "408": CONNECTION,
  "429": {
    label: "Too many requests",
    hint: "The provider asked Aster to slow down. Give it a moment and send again. Nothing was lost.",
  },
  "500": PROVIDER_DOWN,
  "502": PROVIDER_DOWN,
  "503": PROVIDER_DOWN,
  "529": PROVIDER_DOWN,
};

/** Transport failures: stalls, drops, and timeouts that read as plumbing. */
const NETWORKISH = /timed out|time out|connection|network|dropped mid-reply|decoding response body|stream chunk/i;

/** Connect failures: the provider never answered, usually DNS or a refused
 *  socket. Distinct from a mid-stream drop, which is what NETWORKISH covers. */
const UNREACHABLE_ISH = /dns error|failed to lookup address|nodename nor servname|client error \(connect\)|connection refused|unreachable/i;

/** Wrappers like `aster chat exited with code 1: <detail>` add plumbing the
 *  reader doesn't need; the detail is what actually went wrong. */
const EXIT_PREFIX = /^aster(?: \w+)? exited with code \d+[.:]?\s*/i;

/** A status code stated as one, e.g. `model endpoint returned 429`. The
 *  context word keeps line numbers and byte counts from reading as statuses. */
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
      label: "Aster stopped unexpectedly",
      hint: "Aster stopped before it could finish. Send your message again; if it keeps happening, check the Aster output channel for the full error.",
      detail,
    };
  }

  return { label: "Something went wrong", detail: message };
}
