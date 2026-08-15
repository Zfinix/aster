/**
 * `host.ts` grabs the webview bridge at module scope, so importing any
 * component that can talk to the host explodes outside VS Code. Tests get an
 * inert one; asserting on what was posted is the individual test's business.
 */
const state: { current: unknown } = { current: undefined };

Object.assign(globalThis, {
  acquireVsCodeApi: () => ({
    postMessage: () => {},
    getState: () => state.current,
    setState: (next: unknown) => {
      state.current = next;
    },
  }),
});
