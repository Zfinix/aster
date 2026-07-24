import type { Finding } from "./types";

/** Whether a finding's file path refers to the given diff file key. */
export function matchFile(finding: Finding, key: string): boolean {
  const a = finding.file_path;
  return a === key || key.endsWith(a) || a.endsWith(key);
}
