import { defineConfig } from "vitest/config";
import solid from "vite-plugin-solid";
import wasm from "vite-plugin-wasm";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
  // loro-crdt ships as an ES-module WebAssembly import (`import ... from "x.wasm"`).
  // Its initialisation awaits at the top level, which every webview Tauri 2 targets
  // supports (Safari 15+, WebView2), so no downlevel plugin is needed.
  plugins: [solid(), wasm()],
  build: { target: "esnext" },

  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["src/test/setup.ts"],
    restoreMocks: true,
    // Tests run in Node, which cannot load the bundler build of loro-crdt (it
    // imports the .wasm file as an ES module). Use the Node build, the same one
    // loro-prosemirror resolves on its own, so both share one instance.
    alias: { "loro-crdt": "loro-crdt/nodejs" },
  },

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
