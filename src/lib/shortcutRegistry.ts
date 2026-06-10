/**
 * Shortcut Registry
 *
 * Single source of truth for every keyboard shortcut that actually works in
 * DB Hive. The KeyboardShortcutsModal renders from this list, substituting
 * user-rebound keys from settings where `rebindable` is set.
 *
 * Bindings audited from:
 * - `src/routes/__root.tsx` — command palette (⌘K), settings (⌘,),
 *   shortcuts modal (?), new window (⌘⇧N)
 * - `src/routes/index.tsx` — new connection (⌘N), Escape back
 * - `src/routes/_connected/query.tsx` — new tab / close tab (settings-bound)
 * - `src/routes/_connected/route.tsx` — toggle sidebar (⌘B)
 * - `src/components/SQLEditor.tsx` — execute query (Monaco CtrlCmd+Enter)
 *
 * Keys use the same cross-platform string format as `parseShortcut` in
 * `useKeyboardShortcuts` ("Ctrl+X" means Cmd+X on macOS).
 */

import type { ShortcutsSettings } from "@/types";

/** Platform detection shared by shortcut display code. */
export const isMacOS =
  typeof navigator !== "undefined" && navigator.userAgent.includes("Mac");

export interface ShortcutEntry {
  /** Stable identifier */
  id: string;
  /** Human-readable action name */
  label: string;
  /**
   * Default binding in the cross-platform "Ctrl+X" format used by settings
   * and `parseShortcut` (bare "Ctrl" renders/behaves as Cmd on macOS).
   */
  keys: string;
  /** Display group in the shortcuts modal */
  category: string;
  /** Optional longer description */
  description?: string;
  /**
   * When set, the binding is user-rebindable in Settings → Shortcuts and the
   * live value from `settings.shortcuts` should be displayed instead of
   * `keys`.
   */
  rebindable?: keyof ShortcutsSettings;
}

export const shortcutRegistry: ShortcutEntry[] = [
  // ── General (global, bound in __root.tsx) ────────────────────────────────
  {
    id: "command-palette",
    label: "Command Palette",
    keys: "Ctrl+K",
    category: "General",
    description: "Search and run commands from anywhere",
  },
  {
    id: "show-shortcuts",
    label: "Keyboard Shortcuts",
    keys: "?",
    category: "General",
    description: "Show this keyboard shortcuts guide",
    rebindable: "showShortcuts",
  },
  {
    id: "open-settings",
    label: "Open Settings",
    keys: "Ctrl+,",
    category: "General",
    description: "Open application settings",
    rebindable: "openSettings",
  },
  {
    id: "new-window",
    label: "New Window",
    keys: "Ctrl+Shift+N",
    category: "General",
    description: "Open a new DB Hive window",
  },

  // ── Connections page (bound in routes/index.tsx) ─────────────────────────
  {
    id: "new-connection",
    label: "New Connection",
    keys: "Ctrl+N",
    category: "Connections",
    description: "Create a new database connection",
  },
  {
    id: "connections-back",
    label: "Back",
    keys: "Escape",
    category: "Connections",
    description: "Return to the connection list",
  },

  // ── Query workspace (query.tsx, route.tsx, SQLEditor.tsx) ────────────────
  {
    id: "execute-query",
    label: "Execute Query",
    keys: "Ctrl+Enter",
    category: "Query Workspace",
    description: "Run the current SQL query",
  },
  {
    id: "new-tab",
    label: "New Tab",
    keys: "Ctrl+T",
    category: "Query Workspace",
    description: "Open a new query tab",
    rebindable: "newTab",
  },
  {
    id: "close-tab",
    label: "Close Tab",
    keys: "Ctrl+W",
    category: "Query Workspace",
    description: "Close the current tab",
    rebindable: "closeTab",
  },
  {
    id: "toggle-sidebar",
    label: "Toggle Sidebar",
    keys: "Ctrl+B",
    category: "Query Workspace",
    description: "Show or hide the schema explorer",
  },
];

/**
 * Resolve the effective binding for an entry, preferring the user's rebound
 * value from settings when the entry is rebindable.
 */
export function resolveShortcutKeys(
  entry: ShortcutEntry,
  shortcuts?: ShortcutsSettings | null
): string {
  if (entry.rebindable && shortcuts) {
    const bound = shortcuts[entry.rebindable];
    if (bound && bound.trim()) return bound;
  }
  return entry.keys;
}

const MAC_MODIFIER_SYMBOLS: Record<string, string> = {
  ctrl: "⌘", // bare Ctrl behaves as Cmd on macOS (see parseShortcut)
  control: "⌘",
  cmd: "⌘",
  meta: "⌘",
  "⌘": "⌘",
  shift: "⇧",
  "⇧": "⇧",
  alt: "⌥",
  option: "⌥",
  "⌥": "⌥",
};

const KEY_DISPLAY_NAMES: Record<string, string> = {
  enter: "Enter",
  escape: "Esc",
  esc: "Esc",
  space: "Space",
  backspace: "Backspace",
  delete: "Delete",
  tab: "Tab",
};

function formatKeyToken(token: string): string {
  const lower = token.toLowerCase();
  if (KEY_DISPLAY_NAMES[lower]) return KEY_DISPLAY_NAMES[lower];
  return token.length === 1 ? token.toUpperCase() : token;
}

/**
 * Format a cross-platform binding string for display on the current platform:
 * "Ctrl+Shift+N" → "⌘⇧N" on macOS, "Ctrl+Shift+N" elsewhere.
 */
export function formatShortcutKeys(
  keys: string,
  mac: boolean = isMacOS
): string {
  const parts = keys.split("+").map((p) => p.trim()).filter(Boolean);
  if (parts.length === 0) return keys;

  if (!mac) {
    // Normalize casing on Windows/Linux: "ctrl+shift+f" → "Ctrl+Shift+F"
    return parts
      .map((part, index) =>
        index === parts.length - 1
          ? formatKeyToken(part)
          : part.charAt(0).toUpperCase() + part.slice(1).toLowerCase()
      )
      .join("+");
  }

  const modifiers = parts
    .slice(0, -1)
    .map((part) => MAC_MODIFIER_SYMBOLS[part.toLowerCase()] ?? part);
  const key = formatKeyToken(parts[parts.length - 1]);
  return [...modifiers, key].join("");
}
