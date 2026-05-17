/**
 * Multi-window helpers
 *
 * Each OS window is an independent WebView with its own connection context.
 * Window creation happens in the Rust core; an optional profile ID is stashed
 * there and consumed once by the new window on boot to auto-connect.
 */

import { invoke } from "@tauri-apps/api/core";

/**
 * Open a new application window. When `profileId` is provided the new window
 * auto-connects to that saved connection on launch; otherwise it opens to the
 * connection picker. Resolves with the new window's label.
 */
export async function openDatabaseWindow(
  profileId?: string,
): Promise<string> {
  return invoke<string>("open_database_window", {
    profileId: profileId ?? null,
  });
}

/**
 * Called once by a freshly-launched window to learn whether it was opened for
 * a specific saved profile. Returns the profile ID (one-shot) or null.
 */
export async function takePendingWindowProfile(): Promise<string | null> {
  return invoke<string | null>("take_pending_window_profile");
}
