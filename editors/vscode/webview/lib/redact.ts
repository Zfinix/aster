/** MCP server commands routinely carry a live key inline. These panels get
 *  screenshotted and screen-shared, so what is shown is masked; the config
 *  file is the place to read or change the real value. */

const MASK = "••••";

const ASSIGNED = /\b([A-Za-z_][A-Za-z0-9_]*(?:KEY|TOKEN|SECRET|PASSWORD|PASSWD|AUTH|CREDENTIAL)S?)=(\S+)/gi;

const FLAGGED = /(--[A-Za-z0-9-]*(?:key|token|secret|password|auth)[A-Za-z0-9-]*)([= ])(\S+)/gi;

const PREFIXED = /\b(sk-[A-Za-z0-9_-]|sbp_|ghp_|gho_|github_pat_|xoxb-|xoxp-|AKIA|AIza)[A-Za-z0-9_-]{6,}/g;

const BARE = /\b[A-Za-z0-9_-]{32,}\b/g;

export function redactSecrets(text: string): string {
  return text
    .replace(ASSIGNED, (_m, name: string) => `${name}=${MASK}`)
    .replace(FLAGGED, (_m, flag: string, sep: string) => `${flag}${sep}${MASK}`)
    .replace(PREFIXED, MASK)
    .replace(BARE, MASK);
}
