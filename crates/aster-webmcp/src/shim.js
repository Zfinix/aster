// The Aster WebMCP bridge, injected into every document of the attached tab.
// Two jobs: polyfill `document.modelContext` where the browser has no native
// WebMCP, and expose `window.__asterWebmcp` so the Rust side can list the
// page's tools and invoke their `execute` callbacks over CDP.
(() => {
  if (window.__asterWebmcp) return;
  const registry = new Map();

  const fire = (listeners) => listeners.forEach((fn) => fn());

  if (!document.modelContext) {
    const listeners = new Set();
    const drop = (name, tool) => {
      if (tool ? registry.get(name) === tool : registry.has(name)) {
        registry.delete(name);
        fire(listeners);
      }
    };
    document.modelContext = {
      registerTool(tool, options) {
        if (!tool || typeof tool.name !== "string" || !tool.name) {
          throw new TypeError("registerTool: a tool needs a name");
        }
        if (typeof tool.execute !== "function") {
          throw new TypeError(`registerTool: ${tool.name} needs an execute callback`);
        }
        registry.set(tool.name, tool);
        fire(listeners);
        const signal = options && options.signal;
        if (signal) signal.addEventListener("abort", () => drop(tool.name, tool));
        return { unregister: () => drop(tool.name, tool) };
      },
      unregisterTool(name) {
        drop(name);
      },
      getTools() {
        return [...registry.values()];
      },
      addEventListener(type, fn) {
        if (type === "toolchange") listeners.add(fn);
      },
      removeEventListener(type, fn) {
        if (type === "toolchange") listeners.delete(fn);
      },
    };
  } else {
    // Native WebMCP: mirror registrations into the registry so the bridge can
    // reach each tool's execute callback, which getTools() may not hand back.
    try {
      const native = document.modelContext.registerTool.bind(document.modelContext);
      document.modelContext.registerTool = (tool, options) => {
        if (tool && typeof tool.name === "string") registry.set(tool.name, tool);
        return native(tool, options);
      };
    } catch {
      // A frozen native object still lists tools through getTools(); calls to
      // anything registered before injection are simply not bridged.
    }
  }

  const tools = () => {
    if (registry.size) return [...registry.values()];
    const native = document.modelContext.getTools;
    return typeof native === "function" ? document.modelContext.getTools() : [];
  };

  window.__asterWebmcp = {
    list() {
      return JSON.stringify(
        tools().map((tool) => ({
          name: tool.name,
          description: tool.description,
          inputSchema: tool.inputSchema,
        })),
      );
    },
    async call(name, argumentsJson) {
      const tool = tools().find((t) => t.name === name);
      if (!tool || typeof tool.execute !== "function") {
        throw new Error(`no WebMCP tool named ${name} on this page`);
      }
      const result = await tool.execute(JSON.parse(argumentsJson));
      return JSON.stringify(result === undefined ? { content: [] } : result);
    },
  };
})();
