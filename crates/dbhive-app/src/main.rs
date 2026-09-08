//! DB Hive native shell.
//!
//! A standalone cross-platform desktop binary. No WebView, no Tauri: eframe
//! opens a native window and egui draws into it, on macOS, Windows and
//! Linux from one codebase.
//!
//! All database logic lives in `dbhive-core` and is shared with the Tauri
//! shell until that shell is removed at parity.

mod app;
mod screens;
mod task;
mod widgets;
mod theme;

use app::DbHiveApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DB Hive")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "DB Hive",
        options,
        Box::new(|cc| Ok(Box::new(DbHiveApp::new(cc)))),
    )
}
