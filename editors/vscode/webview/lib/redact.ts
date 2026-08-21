/**
 * MCP servers are configured with their credentials inline, so the command that
 * describes one routinely carries a live key. These panels get screenshotted and
 * screen-shared, so what is shown is masked; the config file still holds the
 * real value, and it is the place to read or change one.
 */

const MASK = "••••";

/** `API_KEY=…`, and anything else whose name says it is a credential. */
const ASSIGNED = /\b([A-Za-z_][A-Za-z0-9_]*(?:KEY|TOKEN|SECRET|PASSWORD|PASSWD|AUTH|CREDENTIAL)S?)=(\S+)/gi;

/** `--access-token abc`, `--api-key=abc`. */
const FLAGGED = /(--[A-Za-z0-9-]*(?:key|token|secret|password|auth)[A-Za-z0-9-]*)([= ])(\S+)/gi;

/** Vendor-prefixed keys, which are secret wherever they appear. */
const PREFIXED = /\b(sk-[A-Za-z0-9_-]|sbp_|ghp_|gho_|github_pat_|xoxb-|xoxp-|AKIA|AIza)[A-Za-z0-9_-]{6,}/g;

/** A bare hex or base64ish run long enough that it is not a path or a flag. */
const BARE = /\b[A-Za-z0-9_-]{32,}\b/g;

export function redactSecrets(text: string): string {
  return text
    .replace(ASSIGNED, (_m, name: string) => `${name}=${MASK}`)
    .replace(FLAGGED, (_m, flag: string, sep: string) => `${flag}${sep}${MASK}`)
    .replace(PREFIXED, MASK)
    .replace(BARE, MASK);
}
