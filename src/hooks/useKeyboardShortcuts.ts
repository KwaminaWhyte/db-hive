/**
 * Keyboard Shortcuts Hook
 *
 * Provides global keyboard shortcut handling based on user settings.
 * Shortcuts are normalized across platforms (Ctrl on Windows/Linux, Cmd on macOS).
 */

import { useEffect } from "react";

export interface KeyboardShortcut {
  key: string;
  ctrl?: boolean;
  alt?: boolean;
  shift?: boolean;
  meta?: boolean;
  handler: () => void;
  description?: string;
}

/**
 * Detect macOS at module load. Used to translate cross-platform
 * "Ctrl+X" bindings into "Cmd+X" so a single stored shortcut works
 * on both Windows/Linux and macOS.
 */
const IS_MAC =
  typeof navigator !== "undefined" &&
  navigator.platform.toUpperCase().indexOf("MAC") >= 0;

/**
 * Parse shortcut string like "Ctrl+Enter" into key components.
 *
 * On macOS, a bare "Ctrl" modifier (without an explicit "Cmd") is
 * remapped to "meta" so cross-platform shortcuts behave naturally.
 */
export function parseShortcut(shortcut: string): {
  key: string;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
} {
  const parts = shortcut.split("+").map((p) => p.trim().toLowerCase());
  const hasCtrl = parts.includes("ctrl") || parts.includes("control");
  const hasMeta =
    parts.includes("meta") || parts.includes("cmd") || parts.includes("⌘");

  // On macOS, treat a bare "Ctrl" (without "Cmd") as Cmd so users with
  // a default "Ctrl+T" binding still trigger via Cmd+T.
  const ctrl = IS_MAC && hasCtrl && !hasMeta ? false : hasCtrl;
  const meta = IS_MAC && hasCtrl && !hasMeta ? true : hasMeta;

  const modifiers = {
    ctrl,
    alt: parts.includes("alt") || parts.includes("option") || parts.includes("⌥"),
    shift: parts.includes("shift") || parts.includes("⇧"),
    meta,
  };

  // Last part is the key
  const key = parts[parts.length - 1];

  return { ...modifiers, key };
}

/**
 * Check if a keyboard event matches a shortcut definition
 */
export function matchesShortcut(
  event: KeyboardEvent,
  shortcut: {
    key: string;
    ctrl: boolean;
    alt: boolean;
    shift: boolean;
    meta: boolean;
  }
): boolean {
  const eventKey = event.key.toLowerCase();
  const shortcutKey = shortcut.key.toLowerCase();

  // Handle special key mappings
  const keyMatches =
    eventKey === shortcutKey ||
    (shortcutKey === "enter" && eventKey === "enter") ||
    (shortcutKey === "⏎" && eventKey === "enter") ||
    (shortcutKey === "?" && eventKey === "?");

  // Printable punctuation keys (e.g. "?") often require Shift to type, and
  // event.key already reflects the shifted character. Requiring
  // event.shiftKey === false would make such shortcuts impossible to trigger,
  // so ignore the Shift modifier for single non-alphanumeric character keys.
  const isShiftedCharKey =
    shortcutKey.length === 1 && !/[a-z0-9]/.test(shortcutKey);
  const shiftMatches =
    isShiftedCharKey || event.shiftKey === shortcut.shift;

  return (
    keyMatches &&
    event.ctrlKey === shortcut.ctrl &&
    event.altKey === shortcut.alt &&
    shiftMatches &&
    event.metaKey === shortcut.meta
  );
}

/**
 * Hook to register keyboard shortcuts
 */
export function useKeyboardShortcuts(shortcuts: KeyboardShortcut[]) {
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      // Don't trigger if user is typing in an input/textarea
      const target = event.target as HTMLElement;
      if (
        target.tagName === "INPUT" ||
        target.tagName === "TEXTAREA" ||
        target.isContentEditable
      ) {
        return;
      }

      for (const shortcut of shortcuts) {
        // Skip unbound shortcuts (empty string means user cleared the binding)
        if (!shortcut.key || !shortcut.key.trim()) continue;
        const parsed = parseShortcut(shortcut.key);
        if (matchesShortcut(event, parsed)) {
          event.preventDefault();
          event.stopPropagation();
          shortcut.handler();
          break;
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);

    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [shortcuts]);
}

/**
 * Hook for route-specific shortcuts
 * Only active when the component is mounted
 */
export function useRouteShortcuts(shortcuts: KeyboardShortcut[]) {
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      // Don't trigger if user is typing in an input/textarea
      const target = event.target as HTMLElement;
      if (
        target.tagName === "INPUT" ||
        target.tagName === "TEXTAREA" ||
        target.isContentEditable
      ) {
        return;
      }

      for (const shortcut of shortcuts) {
        // Skip unbound shortcuts (empty string means user cleared the binding)
        if (!shortcut.key || !shortcut.key.trim()) continue;
        const parsed = parseShortcut(shortcut.key);
        if (matchesShortcut(event, parsed)) {
          event.preventDefault();
          event.stopPropagation();
          shortcut.handler();
          break;
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);

    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [shortcuts]);
}
