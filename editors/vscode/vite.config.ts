import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig(({ mode }) => {
  const settings = mode === "settings";
  const name = settings ? "settings" : "index";
  return {
    plugins: [react()],
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
          assetFileNames: `${name}.[ext]`,
        },
      },
    },
  };
});
