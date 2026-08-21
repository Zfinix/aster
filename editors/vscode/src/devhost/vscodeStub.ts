/**
 * The slice of the `vscode` module the CLI-facing sources touch, so `asterCli`,
 * `repo`, and `commands` load unchanged outside an editor. Anything the browser
 * host does not reach is deliberately absent: a missing property throws where it
 * is used, which is louder than a stub that quietly does nothing.
 */

const settings: Record<string, unknown> = {
  binaryPath: process.env.ASTER_BINARY || "aster",
};

let root: string | undefined;

export function setRoot(next: string): void {
  root = next;
}

export const stub = {
  workspace: {
    getConfiguration(): { get<T>(key: string): T | undefined } {
      return { get: <T,>(key: string) => settings[key] as T | undefined };
    },
    get workspaceFolders(): { uri: { fsPath: string } }[] | undefined {
      return root ? [{ uri: { fsPath: root } }] : undefined;
    },
  },
};
