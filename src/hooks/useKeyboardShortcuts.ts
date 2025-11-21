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
 * Parse shortcut string like "Ctrl+Enter" into key components
 */
export function parseShortcut(shortcut: string): {
  key: string;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
} {
  const parts = shortcut.split("+").map((p) => p.trim().toLowerCase());
  const modifiers = {
    ctrl: parts.includes("ctrl") || parts.includes("control"),
    alt: parts.includes("alt") || parts.includes("option") || parts.includes("⌥"),
    shift: parts.includes("shift") || parts.includes("⇧"),
    meta: parts.includes("meta") || parts.includes("cmd") || parts.includes("⌘"),
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

  return (
    keyMatches &&
    event.ctrlKey === shortcut.ctrl &&
    event.altKey === shortcut.alt &&
    event.shiftKey === shortcut.shift &&
    event.metaKey === shortcut.meta
  );
}

/**
 * Hook to register keyboard shortcuts
 */
export function useKeyboardShortcuts(shortcuts: KeyboardShortcut[]) {
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      for (const shortcut of shortcuts) {
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
