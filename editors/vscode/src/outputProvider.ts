import * as vscode from "vscode";

/**
 * Virtual document provider for scratch output tabs. Using a content provider
 * with a custom URI scheme means VS Code treats these as read-only virtual
 * documents and never prompts the user to save them on close, unlike untitled
 * documents which are always unsaved.
 */
export const ASTER_OUTPUT_SCHEME = "aster-output";

/** Tab labels come from the URI, so the name has to stay short enough to read
 *  in a tab strip. */
function tabName(title: string | undefined, id: string): string {
  const base = (title ?? "")
    .split(/[/\\]/)
    .pop()
    ?.replace(/[?#]/g, "")
    .trim();
  return `${base ? base.slice(0, 32) : "output"} (${id})`;
}

export class OutputContentProvider implements vscode.TextDocumentContentProvider {
  // Keyed by the URI path, which is also what the tab is labelled with.
  private static readonly contents = new Map<string, string>();

  static provideUri(content: string, title?: string): vscode.Uri {
    const name = tabName(title, Math.random().toString(36).slice(2, 8));
    this.contents.set(name, content);
    return vscode.Uri.from({ scheme: ASTER_OUTPUT_SCHEME, path: name });
  }

  provideTextDocumentContent(uri: vscode.Uri): string {
    return OutputContentProvider.contents.get(uri.path) ?? "";
  }
}

export function registerOutputProvider(
  context: vscode.ExtensionContext
): void {
  context.subscriptions.push(
    vscode.workspace.registerTextDocumentContentProvider(ASTER_OUTPUT_SCHEME, new OutputContentProvider())
  );
}
