import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

// OpenUI loads these on demand, which would split chunks out of a bundle that
// has to stay one file; the stubs keep it whole.
const stubs = {
  "lucide-react/dynamicIconImports.mjs": fileURLToPath(new URL("./webview/lib/noIcons.ts", import.meta.url)),
  "rehype-katex": fileURLToPath(new URL("./webview/lib/noKatex.ts", import.meta.url)),
};

export { stubs };

export default defineConfig(({ mode }) => {
  const settings = mode === "settings";
  const name = settings ? "settings" : "index";
  return {
    plugins: [react()],
    resolve: { alias: stubs },
    test: {
      setupFiles: ["webview/test-setup.ts"],
    },
    build: {
      outDir: "media/webview",
      // The settings pass runs second and must not wipe the chat bundle.
      emptyOutDir: !settings,
      // The bundled TextMate grammars are most of the weight and the single-file
      // requirement rules out splitting them out, so the warning is just noise.
      chunkSizeWarningLimit: 2000,
      rollupOptions: {
        input: settings ? "webview/settings.tsx" : "webview/main.tsx",
        output: {
          entryFileNames: `${name}.js`,
          inlineDynamicImports: true,
          assetFileNames: `${name}.[ext]`,
        },
      },
    },
  };
});
