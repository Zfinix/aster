import { copyFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const out = fileURLToPath(new URL("../../crates/aster-serve/ui", import.meta.url));

// The same webview, built as a page for `aster serve` to hand a browser. It is
// staged straight into the crate that embeds it, so `cargo build` picks it up.
export default defineConfig({
  root: fileURLToPath(new URL("./webview", import.meta.url)),
  plugins: [
    react(),
    {
      name: "aster-serve-assets",
      closeBundle() {
        copyFileSync(fileURLToPath(new URL("./media/aster.svg", import.meta.url)), `${out}/aster.svg`);
        // The crate embeds this directory at compile time, so it has to exist
        // in a checkout that has never built the UI.
        writeFileSync(`${out}/.gitkeep`, "");
      },
    },
  ],
  build: {
    outDir: out,
    emptyOutDir: true,
    // The bundled TextMate grammars are most of the weight and inlining them is
    // the point, so the warning is just noise.
    chunkSizeWarningLimit: 2000,
    // Stable names: the bundle is committed, and a hash in the filename turns
    // every UI change into a new file plus a deleted one.
    rollupOptions: {
      output: {
        entryFileNames: "index.js",
        chunkFileNames: "index.js",
        assetFileNames: "index.[ext]",
      },
    },
  },
});
