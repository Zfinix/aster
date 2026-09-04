/** Which built bundle the page loads, and which outbox it talks to. */
export type Page = "index" | "settings";

/** What VS Code contributes to a webview and a browser does not: the theme
 *  variables the panel is styled against, and the `acquireVsCodeApi` bridge.
 *  Here the bridge is an SSE stream in and a POST out. */
export function shell(repo: string, page: Page = "index"): string {
  return `<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <link rel="stylesheet" href="/webview/${page}.css" />
    <style>${THEME}</style>
    <title>${page === "settings" ? "Aster Settings" : `Aster — ${repo}`}</title>
  </head>
  <body class="vscode-dark">
    <div id="root"></div>
    <script>
      const outbox = "${page === "settings" ? "/settings-message" : "/message"}";
      let state;
      window.acquireVsCodeApi = () => ({
        postMessage: (message) =>
          fetch(outbox, { method: "POST", body: JSON.stringify(message) }).catch(() => {}),
        getState: () => state,
        setState: (next) => (state = next),
      });
      const events = new EventSource("/events");
      events.onmessage = (e) => {
        const message = JSON.parse(e.data);
        // A newer panel took the host over. Say so and stop, or EventSource
        // reconnects and the two tabs take turns evicting each other.
        if (message.type === "displaced") {
          events.close();
          document.getElementById("root").innerHTML =
            '<p class="devhost-note">This panel was taken over by a newer tab. Reload to use it here.</p>';
          return;
        }
        window.postMessage(message, "*");
      };
    </script>
    <script type="module" src="/webview/${page}.js"></script>
  </body>
</html>`;
}

const THEME = `
:root {
  --vscode-font-family: -apple-system, "SF Pro Text", system-ui, sans-serif;
  --vscode-font-size: 13px;
  --vscode-foreground: #cccccc;
  --vscode-descriptionForeground: #9d9d9d;
  --vscode-widget-border: #3c3c3c;
  --vscode-menu-border: #454545;
  --vscode-menu-background: #1f1f1f;
  --vscode-menu-foreground: #cccccc;
  --vscode-menu-selectionBackground: #04395e;
  --vscode-menu-selectionForeground: #ffffff;
  --vscode-toolbar-hoverBackground: rgba(90, 93, 94, 0.31);
  --vscode-list-hoverBackground: #2a2d2e;
  --vscode-list-dropBackground: #383b3d;
  --vscode-editorWidget-background: #202020;
  --vscode-sideBar-background: #181818;
  --vscode-editor-background: #1f1f1f;
  --vscode-editor-font-family: "SF Mono", Menlo, monospace;
  --vscode-editor-font-size: 12px;
  --vscode-focusBorder: #0078d4;
  --vscode-input-background: #313131;
  --vscode-input-foreground: #cccccc;
  --vscode-button-background: #0078d4;
  --vscode-button-foreground: #ffffff;
  --vscode-editorError-foreground: #f14c4c;
  --vscode-editorWarning-foreground: #cca700;
  --vscode-editorInfo-foreground: #3794ff;
  --vscode-progressBar-background: #0078d4;
  --vscode-gitDecoration-addedResourceForeground: #81b88b;
  --vscode-gitDecoration-deletedResourceForeground: #c74e39;
  --vscode-textLink-foreground: #4daafc;
}
html, body { margin: 0; height: 100%; }
.devhost-note {
  color: var(--vscode-descriptionForeground);
  font-family: var(--vscode-font-family);
  font-size: var(--vscode-font-size);
  padding: 24px;
}
`;
