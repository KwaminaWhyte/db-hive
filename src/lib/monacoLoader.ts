/**
 * Bundles Monaco locally instead of fetching it from the jsdelivr CDN at
 * runtime (@monaco-editor/react's default loader behavior). As a desktop app
 * the editor must work offline and should not block first mount on a 3-5 MB
 * network download — see PERF-06 in docs/audit/performance-audit.md.
 *
 * Imported once for its side effects in src/main.tsx.
 */
import * as monaco from "monaco-editor";
import { loader } from "@monaco-editor/react";
// Vite `?worker` imports compile to same-origin worker chunks, compatible
// with the app CSP (`worker-src 'self' blob:`, `script-src 'self'`).
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";

// All editors in the app use the "sql" language, which has no dedicated
// language service worker — the base editor worker is sufficient.
self.MonacoEnvironment = {
  getWorker() {
    return new EditorWorker();
  },
};

loader.config({ monaco });
