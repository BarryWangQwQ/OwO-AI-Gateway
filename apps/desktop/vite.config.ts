import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const host = process.env.TAURI_DEV_HOST;

// https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { "@": path.resolve(import.meta.dirname, "./src") },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  optimizeDeps: {
    // The CodeMirror editor is loaded lazily (Settings and Skills pages), so the dev
    // server would otherwise discover these packages mid-session and
    // re-optimize; pre-bundle them at start-up instead.
    include: [
      "@uiw/react-codemirror",
      "@codemirror/state",
      "@codemirror/view",
      "@codemirror/language",
      "@codemirror/commands",
      "@codemirror/legacy-modes/mode/toml",
      "@codemirror/lang-markdown",
      "@codemirror/lang-yaml",
      "@lezer/highlight",
    ],
  },
  build: {
    target: "chrome105",
    minify: !process.env.TAURI_ENV_DEBUG,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    // A desktop app loads its bundle from disk; one chunk is fine.
    chunkSizeWarningLimit: 2000,
  },
});
