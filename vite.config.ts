import path from "path";
import { readFileSync } from "fs";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { TanStackRouterVite } from "@tanstack/router-plugin/vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// Single source of truth for the app version: package.json. Injected at
// build time as the `__APP_VERSION__` global so every UI surface stays in
// sync — bump the version in package.json only.
const appVersion = JSON.parse(
  readFileSync(path.resolve(__dirname, "package.json"), "utf-8")
).version as string;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [TanStackRouterVite(), react(), tailwindcss()],

  define: {
    __APP_VERSION__: JSON.stringify(appVersion),
  },

  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },

  build: {
    // No manual chunk splitting. This is a desktop app: assets are served
    // from the local Tauri bundle, so initial-load chunk size is irrelevant
    // and there is no network round-trip to optimize for. Hand-splitting the
    // vendor graph here created circular chunk dependencies (React, TanStack,
    // reactflow, dagre/graphlib all cross-reference) where a consumer chunk
    // evaluated before the chunk holding its dependency — leaving the binding
    // undefined at module-init time and producing a blank production window
    // (e.g. "Cannot set properties of undefined (setting 'Activity')",
    // "reading 'useLayoutEffect'", "reading 'Graph'"). A single bundle keeps
    // init order correct. Raise the size warning so the build stays quiet.
    chunkSizeWarningLimit: 4000,
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
