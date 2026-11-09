/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri-compatible Vite config. Keep the dev port in step with `build.devUrl` in
// crates/desktop/tauri.conf.json.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  // Readable module class names ("row_a1B2c") keep the DOM easy to inspect.
  css: { modules: { generateScopedName: "[local]_[hash:base64:5]" } },
  server: { port: 5173, strictPort: true },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // WebView2 (Chromium) on Windows, WebKitGTK on Linux.
    target: ["chrome111", "safari16"],
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    css: { modules: { classNameStrategy: "non-scoped" } },
  },
});
