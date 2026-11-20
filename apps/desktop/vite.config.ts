import { defineConfig } from "vite";
import wasm from "vite-plugin-wasm";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
  // loro-crdt ships as an ES-module WebAssembly import (`import ... from "x.wasm"`).
  // Its initialisation awaits at the top level, which every webview Tauri 2 targets
  // supports (Safari 15+, WebView2), so no downlevel plugin is needed.
  plugins: [wasm()],
  build: { target: "esnext" },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
