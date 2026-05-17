//! Multi-window management
//!
//! DB-Hive supports opening multiple independent OS windows, each able to
//! connect to a different database. The Rust core stays a single process with
//! one shared [`crate::state::AppState`]: active connections are keyed by
//! connection ID, so two windows talking to different databases never collide.
//! Per-window UI/connection context lives entirely in each window's WebView.
//!
//! Windows are created on the Rust side (no JS capability needed for creation).
//! When a window is opened "for" a saved profile, the profile ID is stashed in
//! [`PendingWindowProfiles`] keyed by the new window's label; the fresh WebView
//! calls [`take_pending_window_profile`] on boot to learn it should auto-connect.

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{AppHandle, State, WebviewUrl, WebviewWindowBuilder};

/// Maps a freshly-spawned window label -> profile ID it should auto-connect to.
///
/// One-shot: the entry is removed the first time the window asks for it via
/// [`take_pending_window_profile`].
#[derive(Default)]
pub struct PendingWindowProfiles(pub Mutex<HashMap<String, String>>);

/// Build a new application window.
///
/// `profile_id` (when `Some`) is recorded so the new WebView auto-connects to
/// that saved connection on boot. Returns the new window's label.
///
/// Window chrome mirrors the statically-defined `main` window: on macOS the
/// overlay title bar style keeps native traffic lights + keyboard routing; on
/// other platforms native decorations are removed so the custom titlebar is the
/// only chrome.
pub fn spawn_window(
    app: &AppHandle,
    pending: &PendingWindowProfiles,
    profile_id: Option<String>,
) -> Result<String, String> {
    let label = format!("win-{}", uuid::Uuid::new_v4());

    if let Some(pid) = profile_id {
        pending
            .0
            .lock()
            .map_err(|e| format!("Lock poisoned: {e}"))?
            .insert(label.clone(), pid);
    }

    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title("DB Hive")
        .inner_size(1500.0, 900.0)
        .min_inner_size(1000.0, 700.0)
        .resizable(true)
        .center();

    #[cfg(target_os = "macos")]
    {
        builder = builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true);
    }

    #[cfg(not(target_os = "macos"))]
    {
        builder = builder.decorations(false);
    }

    builder.build().map_err(|e| {
        // Spawn failed: don't leak the pending mapping for a window that
        // will never exist.
        if let Ok(mut map) = pending.0.lock() {
            map.remove(&label);
        }
        format!("Failed to create window: {e}")
    })?;

    Ok(label)
}

/// Open a new application window, optionally bound to a saved connection
/// profile (the new window auto-connects to it on launch).
#[tauri::command]
pub fn open_database_window(
    app: AppHandle,
    pending: State<'_, PendingWindowProfiles>,
    profile_id: Option<String>,
) -> Result<String, String> {
    spawn_window(&app, &pending, profile_id)
}

/// Called once by a freshly-launched window to learn whether it was opened for
/// a specific saved profile. Consumes the mapping (returns `None` afterwards).
#[tauri::command]
pub fn take_pending_window_profile(
    window: tauri::Window,
    pending: State<'_, PendingWindowProfiles>,
) -> Option<String> {
    pending
        .0
        .lock()
        .ok()
        .and_then(|mut map| map.remove(window.label()))
}
