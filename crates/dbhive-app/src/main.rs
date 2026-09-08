//! DB Hive native shell.
//!
//! Replaces the Tauri + React shell with a native egui UI. All database
//! logic lives in `dbhive-core` and is shared with the Tauri shell until
//! that shell is removed at parity.

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
        Box::new(|_cc| Ok(Box::new(DbHiveApp::default()))),
    )
}

#[derive(Default)]
struct DbHiveApp {}

impl eframe::App for DbHiveApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("DB Hive");
            ui.label("Native shell bootstrapping.");
        });
    }
}
