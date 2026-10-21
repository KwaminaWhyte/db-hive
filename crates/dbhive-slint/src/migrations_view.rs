//! Maps the migrations panel's plain view types
//! (`dbhive_ui_model::migrations_panel`) to the Slint structs it draws.

use dbhive_ui_model::migrations_panel::{DiffKind, DiffTreeRow, ProfileOption};
use slint::{ModelRc, SharedString, VecModel};

use crate::{MigrationDiffRow, Tone};

fn kind_tone(k: DiffKind) -> Tone {
    match k {
        DiffKind::Added => Tone::Success,
        DiffKind::Removed => Tone::Danger,
        DiffKind::Modified => Tone::Warning,
    }
}

pub fn diff_rows(rows: &[DiffTreeRow]) -> ModelRc<MigrationDiffRow> {
    ModelRc::new(VecModel::from(
        rows.iter()
            .map(|r| MigrationDiffRow {
                key: r.key.as_str().into(),
                depth: r.depth as i32,
                marker: r.marker.into(),
                tone: kind_tone(r.kind),
                label: r.label.as_str().into(),
                detail: r.detail.as_str().into(),
                expandable: r.expandable,
                expanded: r.expanded,
                heading: r.heading,
            })
            .collect::<Vec<_>>(),
    ))
}

/// Picker labels: a filled dot for an open connection, a hollow one for a
/// saved connection that is not open.
pub fn connection_labels(options: &[ProfileOption]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        options
            .iter()
            .map(|o| SharedString::from(format!("{} {}", if o.connected { "●" } else { "○" }, o.label)))
            .collect::<Vec<_>>(),
    ))
}
