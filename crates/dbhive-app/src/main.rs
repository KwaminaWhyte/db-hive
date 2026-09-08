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
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("DB Hive")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([900.0, 600.0]);

    // Development aid: DBHIVE_DEV_WINDOW="x,y,w,h" pins the window somewhere
    // known, so a screenshot can be cropped to it exactly instead of grabbing
    // the whole display. Unset in normal use.
    if let Ok(spec) = std::env::var("DBHIVE_DEV_WINDOW") {
        let n: Vec<f32> = spec.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if let [x, y, w, h] = n[..] {
            viewport = viewport
                .with_position([x, y])
                .with_inner_size([w, h]);
        }
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "DB Hive",
        options,
        Box::new(|cc| Ok(Box::new(DbHiveApp::new(cc)))),
    )
}
