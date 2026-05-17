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
    // Monaco is inherently large; raise the warning threshold modestly so
    // the build output stays readable while still flagging real regressions.
    chunkSizeWarningLimit: 900,
    rollupOptions: {
      output: {
        // Split heavy vendor deps into dedicated chunks so the initial
        // bundle stays small and big editors/graph libs load on demand.
        manualChunks(id) {
          if (!id.includes("node_modules")) return undefined;

          const inPkg = (...pkgs: string[]) =>
            pkgs.some(
              (p) =>
                id.includes(`node_modules/${p}/`) ||
                id.includes(`node_modules/${p}\\`)
            );

          // Monaco editor + its React wrapper (very large).
          if (inPkg("monaco-editor", "@monaco-editor/react")) {
            return "monaco";
          }

          // Graph layout engine.
          if (inPkg("dagre", "@dagrejs/dagre")) {
            return "dagre";
          }

          // React core + router.
          if (
            inPkg(
              "react",
              "react-dom",
              "react-router",
              "@tanstack/react-router",
              "@tanstack/router-core"
            )
          ) {
            return "react-vendor";
          }

          // All Radix UI primitives.
          if (id.includes("node_modules/@radix-ui/")) {
            return "radix";
          }

          // Icon libraries.
          if (inPkg("react-icons", "lucide-react")) {
            return "icons";
          }

          // Graph rendering (heavy, used only by the schema designer).
          if (inPkg("reactflow", "@reactflow")) {
            return "reactflow";
          }

          // Remaining TanStack libs (table/virtual/query) used across grids.
          if (id.includes("node_modules/@tanstack/")) {
            return "tanstack";
          }

          return undefined;
        },
      },
    },
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
