/** The slice of the `vscode` module the CLI-facing sources touch, so they load
 *  unchanged outside an editor. Anything else is absent on purpose: a missing
 *  property throws where it is used, louder than a stub that does nothing. */

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
