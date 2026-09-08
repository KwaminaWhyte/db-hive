//! TODO(migration): er_diagram
//!
//! Stub. See docs/rust-ui-migration/03-conventions.md before implementing,
//! and docs/rust-ui-migration/01-frontend-inventory.md for the parity items
//! this screen owns.

use crate::app::DbHiveApp;

#[derive(Default)]
pub struct ErDiagram {
    pub open: bool,
}

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    ui.label(crate::theme::text::muted(palette, "Not implemented yet"));
}
