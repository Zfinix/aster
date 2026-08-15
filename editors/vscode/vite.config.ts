import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The webview bundle is loaded from disk by the extension under a strict CSP,
// so everything must be inlined into one JS file and one CSS file with stable
// names that panel.ts can reference.
export default defineConfig({
  plugins: [react()],
  test: {
    setupFiles: ["webview/test-setup.ts"],
  },
  build: {
    outDir: "media/webview",
    emptyOutDir: true,
    // The bundled TextMate grammars are most of the weight and the single-file
    // requirement rules out splitting them out, so the warning is just noise.
    chunkSizeWarningLimit: 2000,
    rollupOptions: {
      input: "webview/main.tsx",
      output: {
        entryFileNames: "index.js",
        assetFileNames: "index.[ext]",
      },
    },
  },
});
