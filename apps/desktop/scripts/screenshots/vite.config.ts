// The app with its Tauri backend replaced by ./mock-tauri.ts, for the README screenshots.
// Served by ./capture.mjs; never part of `npm run build` (the app's own vite.config.ts builds that).
import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const here = import.meta.dirname;
const app = path.resolve(here, "../..");
const mock = path.join(here, "mock-tauri.ts");

export default defineConfig({
  root: app,
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: [
      { find: /^@tauri-apps\/api\/(core|window)$/, replacement: mock },
      { find: "@", replacement: path.join(app, "src") },
    ],
  },
  clearScreen: false,
  server: { port: 1430, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  optimizeDeps: {
    entries: ["scripts/screenshots/index.html"],
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
});
