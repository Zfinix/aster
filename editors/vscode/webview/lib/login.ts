/** Progress of one `aster login` run started from the panel. */
export interface LoginState {
  lines: string[];
  done?: boolean;
  ok?: boolean;
  message?: string;
}

const MAX_LINES = 12;

export function loginLine(prev: LoginState | null, line: string): LoginState {
  const lines = [...(prev?.lines ?? []), line].slice(-MAX_LINES);
  return { lines };
}
