//! Maps the backup manager's plain Rust rows (`dbhive_ui_model::backup_manager`)
//! to the Slint structs the Backup panel draws.

use dbhive_core::models::DbDriver;
use dbhive_ui_model::backup::drop_existing_applies;
use dbhive_ui_model::backup_manager::{
    restore_blocker, restore_confirm_label, restore_warnings, row_view, BackupManager, RowView,
    View,
};
use slint::{ModelRc, VecModel};

use crate::import_export_view::strings;
use crate::{BackupRow, BackupState, BackupView};

/// What the panel needs to know about the connection it acts on.
#[derive(Default, Clone)]
pub struct Context {
    /// "Widgets · Sqlite".
    pub location: String,
    pub driver: Option<DbDriver>,
    pub target_name: String,
    /// What a restore overwrites: the database, or the SQLite file's name.
    pub database_label: String,
    /// Why a backup cannot be created, or blank.
    pub blocker: String,
}

impl Context {
    pub fn driver_label(&self) -> String {
        self.driver.as_ref().map(|d| format!("{d:?}")).unwrap_or_default()
    }
}

/// Everything the panel shows. The text fields (filter, note, tables, output
/// folder) are left alone: they are two-way bound to the window.
pub fn project(g: &BackupState, m: &BackupManager, c: &Context) {
    g.set_view(match m.view {
        View::List => BackupView::List,
        View::Create => BackupView::Create,
        View::Restore => BackupView::Restore,
        View::Progress => BackupView::Progress,
    });
    g.set_location(c.location.clone().into());

    // The list.
    g.set_directory(m.directory.clone().into());
    g.set_loading(m.loading);
    g.set_list_error(m.list_error.clone().into());
    g.set_rows(rows_model(m));
    g.set_summary(m.summary().into());
    g.set_backup_blocker(c.blocker.clone().into());

    // The create form.
    g.set_include_schema(m.form.include_schema);
    g.set_include_data(m.form.include_data);
    let whole_file = matches!(c.driver, Some(DbDriver::Sqlite | DbDriver::MongoDb));
    g.set_tables_enabled(!whole_file);
    g.set_tables_hint(tables_hint(whole_file, &c.driver_label()).into());
    g.set_form_error(m.form_error.clone().into());

    // The restore confirmation.
    if let (Some(entry), Some(driver)) = (&m.restore_target, &c.driver) {
        g.set_restore_file(entry.file_name.clone().into());
        g.set_restore_facts(row_view(entry).subtitle.into());
        g.set_restore_warnings(strings(&restore_warnings(
            entry,
            &c.target_name,
            &c.driver_label(),
            &c.database_label,
            m.drop_existing,
        )));
        g.set_restore_blocker(restore_blocker(entry, Some(driver)).unwrap_or_default().into());
        g.set_drop_applies(drop_existing_applies(Some(driver)));
        g.set_drop_existing(m.drop_existing);
        g.set_restore_label(restore_confirm_label(m.drop_existing, &c.database_label).into());
    }

    // Progress.
    if let Some(op) = &m.op {
        g.set_op_title(op.kind.title().into());
        g.set_op_line(op.line().into());
        g.set_op_fraction(op.fraction());
        g.set_op_done(op.done);
        g.set_op_failed(op.failed());
        g.set_op_result(
            match (&op.error, op.done) {
                (Some(e), _) => e.clone(),
                (None, true) => op.message.clone(),
                _ => String::new(),
            }
            .into(),
        );
    }
}

fn tables_hint(whole_file: bool, driver: &str) -> String {
    if whole_file {
        format!("{driver} backups copy the whole database; choosing tables is not available.")
    } else {
        "Leave blank for every table. Separate names with commas or new lines.".to_string()
    }
}

pub fn to_row(r: RowView) -> BackupRow {
    BackupRow {
        id: r.id.into(),
        title: r.title.into(),
        subtitle: r.subtitle.into(),
        note: r.note.into(),
        ok: r.ok,
    }
}

/// The list as shown: filtered, in the order the service returned it
/// (newest first).
pub fn rows_model(m: &dbhive_ui_model::backup_manager::BackupManager) -> ModelRc<BackupRow> {
    ModelRc::new(VecModel::from(
        m.entries_matching().into_iter().map(|e| to_row(row_view(e))).collect::<Vec<_>>(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbhive_core::models::backup::{BackupEntry, BackupStatus};
    use dbhive_ui_model::backup_manager::BackupManager;
    use slint::Model;

    fn entry(name: &str, ok: bool) -> BackupEntry {
        BackupEntry {
            id: "1".into(),
            connection_id: String::new(),
            connection_name: "Shop".into(),
            driver: "Sqlite".into(),
            file_path: format!("/b/{name}"),
            file_name: name.into(),
            size_bytes: 10,
            created_at: 1_700_000_000,
            status: if ok { BackupStatus::Completed } else { BackupStatus::Failed("disk full".into()) },
            note: None,
        }
    }

    #[test]
    fn whole_file_drivers_say_why_tables_cannot_be_chosen() {
        assert!(tables_hint(true, "Sqlite").starts_with("Sqlite backups copy the whole database"));
        assert!(tables_hint(false, "Postgres").contains("Leave blank"));
    }

    #[test]
    fn the_driver_label_is_empty_without_a_driver() {
        assert_eq!(Context::default().driver_label(), "");
        let c = Context { driver: Some(DbDriver::MySql), ..Context::default() };
        assert_eq!(c.driver_label(), "MySql");
    }

    #[test]
    fn the_list_is_filtered_and_failures_keep_their_reason() {
        let mut m = BackupManager::default();
        m.entries = vec![entry("a.sqlite", true), entry("b.sqlite", false)];
        let rows = rows_model(&m);
        assert_eq!(rows.row_count(), 2);
        let bad = rows.row_data(1).unwrap();
        assert!(!bad.ok);
        assert_eq!(bad.note, "disk full");
        assert_eq!(bad.id, "/b/b.sqlite");
        m.filter = "a.sq".into();
        assert_eq!(rows_model(&m).row_count(), 1);
    }
}
