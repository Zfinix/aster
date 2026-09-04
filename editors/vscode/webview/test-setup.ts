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
