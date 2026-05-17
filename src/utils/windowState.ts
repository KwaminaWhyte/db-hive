/**
 * Window State Persistence
 *
 * Utilities to save and restore window position and size
 */

import { getCurrentWindow, PhysicalPosition, PhysicalSize } from "@tauri-apps/api/window";

interface WindowState {
  x: number;
  y: number;
  width: number;
  height: number;
  maximized: boolean;
}

// Window position/size is scoped per OS window so multiple open windows
// (multi-window mode) don't clobber each other's geometry. The `main` window
// keeps the legacy key for backward compatibility with existing installs.
function storageKey(label: string): string {
  return label === "main"
    ? "db-hive-window-state"
    : `db-hive-window-state-${label}`;
}

/**
 * Save current window state to localStorage
 */
export async function saveWindowState(): Promise<void> {
  try {
    const appWindow = getCurrentWindow();
    const key = storageKey(appWindow.label);

    const [position, size, maximized] = await Promise.all([
      appWindow.outerPosition(),
      appWindow.outerSize(),
      appWindow.isMaximized(),
    ]);

    const state: WindowState = {
      x: position.x,
      y: position.y,
      width: size.width,
      height: size.height,
      maximized,
    };

    localStorage.setItem(key, JSON.stringify(state));
  } catch (error) {
    console.error("Failed to save window state:", error);
  }
}

/**
 * Restore window state from localStorage
 */
export async function restoreWindowState(): Promise<void> {
  try {
    const appWindow = getCurrentWindow();
    const saved = localStorage.getItem(storageKey(appWindow.label));
    if (!saved) return;

    const state: WindowState = JSON.parse(saved);

    // Restore size and position
    await Promise.all([
      appWindow.setPosition(new PhysicalPosition(state.x, state.y)),
      appWindow.setSize(new PhysicalSize(state.width, state.height)),
    ]);

    // Restore maximized state
    if (state.maximized) {
      await appWindow.maximize();
    }
  } catch (error) {
    console.error("Failed to restore window state:", error);
  }
}

/**
 * Setup window state persistence
 * Call this on app initialization
 */
export function setupWindowStatePersistence(): () => void {
  // Restore state on mount
  restoreWindowState();

  // Save state periodically (every 2 seconds of inactivity)
  let saveTimeout: ReturnType<typeof setTimeout>;

  const scheduleSave = () => {
    clearTimeout(saveTimeout);
    saveTimeout = setTimeout(() => {
      saveWindowState();
    }, 2000);
  };

  // Listen for window events that should trigger save
  const appWindow = getCurrentWindow();

  const unlistenResize = appWindow.onResized(() => {
    scheduleSave();
  });

  const unlistenMove = appWindow.onMoved(() => {
    scheduleSave();
  });

  // Save state before window closes
  window.addEventListener("beforeunload", () => {
    saveWindowState();
  });

  // Return cleanup function
  return () => {
    clearTimeout(saveTimeout);
    unlistenResize.then((fn) => fn());
    unlistenMove.then((fn) => fn());
  };
}
