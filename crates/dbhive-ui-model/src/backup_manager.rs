//! The backup manager as state, without a toolkit: which view is showing,
//! the listing and its filter, the create form, what a restore is about to
//! do, and how the stages the core reports turn into a progress readout.
//!
//! The dump and restore themselves, and the formatting helpers, are in
//! `dbhive_core::service_backup` and `crate::backup`.

use std::time::Duration;

use dbhive_core::models::backup::{BackupEntry, BackupProgress, BackupStatus};
use dbhive_core::models::DbDriver;
use dbhive_core::service_backup as svc;

use crate::backup::{
    drop_existing_applies, format_bytes, format_duration, format_timestamp, restore_button_label,
    status_label, BackupForm,
};

/// What the panel is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    List,
    Create,
    Restore,
    /// A backup or restore is running or has just finished.
    Progress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    Backup,
    Restore,
}

impl OpKind {
    pub fn title(self) -> &'static str {
        match self {
            OpKind::Backup => "Creating backup",
            OpKind::Restore => "Restoring backup",
        }
    }
}

/// A backup or restore in flight, or the last one's outcome.
#[derive(Debug, Clone)]
pub struct Operation {
    pub kind: OpKind,
    pub stage: String,
    pub message: String,
    pub done: bool,
    pub error: Option<String>,
    /// Whether the restore drops the database first, which adds a stage.
    pub drops: bool,
    pub elapsed: Option<Duration>,
}

impl Operation {
    pub fn started(kind: OpKind, drops: bool) -> Self {
        Self {
            kind,
            stage: "preparing".to_string(),
            message: "Starting".to_string(),
            done: false,
            error: None,
            drops,
            elapsed: None,
        }
    }

    /// Fold in a stage the service reported.
    pub fn apply(&mut self, p: &BackupProgress) {
        self.stage = p.stage.clone();
        self.message = p.message.clone();
        self.done = p.done;
        self.error = p.error.clone();
    }

    /// How far along, 0 to 1, from the stage reached. The tools give no
    /// byte counts, so this is stage progress, not data progress; the bar is
    /// honest about being coarse by never reaching 1 before the end.
    pub fn fraction(&self) -> f32 {
        if self.done {
            return 1.0;
        }
        let order: &[&str] = match (self.kind, self.drops) {
            (OpKind::Backup, _) => &["preparing", "dumping", "writing"],
            (OpKind::Restore, false) => &["preparing", "restoring"],
            (OpKind::Restore, true) => &["preparing", "dropping", "restoring"],
        };
        let at = order.iter().position(|s| *s == self.stage).unwrap_or(0);
        // The last stage is "in progress", so it sits short of full.
        (at as f32 + 0.5) / (order.len() as f32 + 0.5)
    }

    pub fn failed(&self) -> bool {
        self.error.is_some()
    }

    /// "Running pg_dump… (2.3s)".
    pub fn line(&self) -> String {
        match self.elapsed {
            Some(d) if !self.message.is_empty() => format!("{} ({})", self.message, format_duration(d)),
            Some(d) => format_duration(d),
            None => self.message.clone(),
        }
    }
}

/// Everything the panel needs between renders.
#[derive(Debug, Clone)]
pub struct BackupManager {
    pub view: View,
    /// Directory the listing came from.
    pub directory: String,
    pub entries: Vec<BackupEntry>,
    pub loading: bool,
    pub list_error: String,
    pub filter: String,
    pub form: BackupForm,
    pub form_error: String,
    /// The backup a restore view is open for.
    pub restore_target: Option<BackupEntry>,
    pub drop_existing: bool,
    pub op: Option<Operation>,
}

impl Default for BackupManager {
    fn default() -> Self {
        Self {
            view: View::List,
            directory: String::new(),
            entries: Vec::new(),
            loading: false,
            list_error: String::new(),
            filter: String::new(),
            form: BackupForm::default(),
            form_error: String::new(),
            restore_target: None,
            drop_existing: false,
            op: None,
        }
    }
}

impl BackupManager {
    /// A backup or restore is running: the panel must not start another, and
    /// must not close under it without saying so.
    pub fn busy(&self) -> bool {
        self.op.as_ref().is_some_and(|o| !o.done)
    }

    pub fn entries_matching(&self) -> Vec<&BackupEntry> {
        let q = self.filter.trim().to_lowercase();
        self.entries
            .iter()
            .filter(|e| {
                q.is_empty()
                    || e.file_name.to_lowercase().contains(&q)
                    || e.connection_name.to_lowercase().contains(&q)
                    || e.driver.to_lowercase().contains(&q)
                    || e.note.as_deref().is_some_and(|n| n.to_lowercase().contains(&q))
            })
            .collect()
    }

    pub fn find(&self, path: &str) -> Option<&BackupEntry> {
        self.entries.iter().find(|e| e.file_path == path)
    }

    /// "12 backups, 1.4 GB" or "3 of 12 backups".
    pub fn summary(&self) -> String {
        let total = self.entries.len();
        let shown = self.entries_matching().len();
        let bytes: u64 = self.entries.iter().map(|e| e.size_bytes).sum();
        let noun = |n: usize| if n == 1 { "backup" } else { "backups" };
        if shown != total {
            format!("{shown} of {total} {}", noun(total))
        } else {
            format!("{total} {}, {}", noun(total), format_bytes(bytes))
        }
    }

    pub fn open_restore(&mut self, entry: BackupEntry) {
        self.drop_existing = false;
        self.restore_target = Some(entry);
        self.view = View::Restore;
    }

    pub fn set_drop_existing(&mut self, on: bool, driver: Option<&DbDriver>) {
        // SQLite restore replaces the file; the toggle means nothing there.
        self.drop_existing = on && drop_existing_applies(driver);
    }

    pub fn begin(&mut self, kind: OpKind) {
        let drops = kind == OpKind::Restore && self.drop_existing;
        self.op = Some(Operation::started(kind, drops));
        self.view = View::Progress;
    }

    /// Back to the list after a progress view is dismissed.
    pub fn dismiss_progress(&mut self) {
        if !self.busy() {
            self.op = None;
            self.restore_target = None;
            self.view = View::List;
        }
    }
}

// ---------------------------------------------------------------------------
// Row projection
// ---------------------------------------------------------------------------

/// What one list row says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowView {
    /// The file path, which identifies the backup.
    pub id: String,
    pub title: String,
    pub subtitle: String,
    /// The failure reason of a failed backup.
    pub note: String,
    pub ok: bool,
    pub status: String,
}

pub fn row_view(e: &BackupEntry) -> RowView {
    let (status, reason) = status_label(&e.status);
    let mut facts: Vec<String> = Vec::new();
    if !e.connection_name.is_empty() {
        facts.push(e.connection_name.clone());
    }
    if !e.driver.is_empty() {
        facts.push(e.driver.clone());
    }
    facts.push(format_bytes(e.size_bytes));
    facts.push(format_timestamp(e.created_at));
    if let Some(n) = e.note.as_deref().filter(|n| !n.is_empty()) {
        facts.push(format!("\"{n}\""));
    }
    RowView {
        id: e.file_path.clone(),
        title: e.file_name.clone(),
        subtitle: facts.join("  ·  "),
        note: reason.unwrap_or_default().to_string(),
        ok: matches!(e.status, BackupStatus::Completed),
        status: status.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Restore
// ---------------------------------------------------------------------------

/// Why a restore of `entry` into the open connection cannot start.
pub fn restore_blocker(entry: &BackupEntry, target_driver: Option<&DbDriver>) -> Option<String> {
    let Some(driver) = target_driver else {
        return Some("Open a connection to restore into.".to_string());
    };
    if !svc::supports_restore(driver) {
        return Some(format!("Restore is not supported for {driver:?} connections."));
    }
    if let BackupStatus::Failed(_) = entry.status {
        return Some("This backup failed and cannot be restored.".to_string());
    }
    if !std::path::Path::new(&entry.file_path).is_file() {
        return Some("The backup file no longer exists.".to_string());
    }
    None
}

/// Warnings to show before a restore: always the destructive one, plus a
/// mismatch between the backup's source and the target.
pub fn restore_warnings(
    entry: &BackupEntry,
    target_name: &str,
    target_driver_label: &str,
    db_label: &str,
    drop_existing: bool,
) -> Vec<String> {
    let mut out = Vec::new();
    out.push(if drop_existing {
        format!("{db_label} will be dropped and recreated, then filled from this backup. Everything in it now is lost.")
    } else {
        format!("This writes into {db_label} on {target_name}. Rows and tables that clash with the backup may be overwritten or cause errors.")
    });
    if !entry.driver.is_empty() && !entry.driver.eq_ignore_ascii_case(target_driver_label) {
        out.push(format!(
            "This backup came from a {} connection; you are restoring into {}.",
            entry.driver, target_driver_label
        ));
    }
    out
}

pub fn restore_confirm_label(drop_existing: bool, db_label: &str) -> String {
    restore_button_label(drop_existing, db_label)
}

/// Delete confirmation text naming the exact file.
pub fn delete_message(entry: &BackupEntry) -> String {
    format!(
        "{} ({}) will be removed from disk. This cannot be undone.",
        entry.file_name,
        format_bytes(entry.size_bytes)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> BackupEntry {
        BackupEntry {
            id: "1".into(),
            connection_id: "c1".into(),
            connection_name: "Shop".into(),
            driver: "Sqlite".into(),
            file_path: format!("/b/{name}"),
            file_name: name.into(),
            size_bytes: 2048,
            created_at: 1_700_000_000,
            status: BackupStatus::Completed,
            note: Some("before migration".into()),
        }
    }

    fn progress(stage: &str, done: bool, error: Option<&str>) -> BackupProgress {
        BackupProgress {
            stage: stage.into(),
            message: format!("msg {stage}"),
            done,
            error: error.map(str::to_string),
        }
    }

    #[test]
    fn a_backup_advances_through_its_stages_and_ends_at_one() {
        let mut op = Operation::started(OpKind::Backup, false);
        let a = op.fraction();
        op.apply(&progress("dumping", false, None));
        let b = op.fraction();
        op.apply(&progress("writing", false, None));
        let c = op.fraction();
        assert!(a < b && b < c && c < 1.0, "{a} {b} {c}");
        op.apply(&progress("done", true, None));
        assert_eq!(op.fraction(), 1.0);
        assert!(!op.failed());
    }

    #[test]
    fn a_restore_that_drops_has_one_more_stage() {
        let mut plain = Operation::started(OpKind::Restore, false);
        plain.apply(&progress("restoring", false, None));
        let mut dropping = Operation::started(OpKind::Restore, true);
        dropping.apply(&progress("dropping", false, None));
        assert!(plain.fraction() > dropping.fraction());
        dropping.apply(&progress("restoring", false, None));
        assert!(dropping.fraction() > 0.5);
    }

    #[test]
    fn a_failure_is_carried_and_finishes_the_bar() {
        let mut op = Operation::started(OpKind::Restore, false);
        op.apply(&progress("done", true, Some("psql: connection refused")));
        assert!(op.failed());
        assert!(op.done);
        assert_eq!(op.error.as_deref(), Some("psql: connection refused"));
    }

    #[test]
    fn the_progress_line_adds_the_elapsed_time() {
        let mut op = Operation::started(OpKind::Backup, false);
        op.apply(&progress("dumping", false, None));
        assert_eq!(op.line(), "msg dumping");
        op.elapsed = Some(Duration::from_millis(2300));
        assert_eq!(op.line(), "msg dumping (2.3s)");
    }

    #[test]
    fn busy_follows_the_operation_and_blocks_dismissal() {
        let mut m = BackupManager::default();
        assert!(!m.busy());
        m.begin(OpKind::Backup);
        assert!(m.busy());
        assert_eq!(m.view, View::Progress);
        m.dismiss_progress();
        assert_eq!(m.view, View::Progress, "cannot leave a running operation");
        m.op.as_mut().unwrap().apply(&progress("done", true, None));
        assert!(!m.busy());
        m.dismiss_progress();
        assert_eq!(m.view, View::List);
        assert!(m.op.is_none());
    }

    #[test]
    fn the_filter_matches_file_connection_driver_and_note() {
        let mut m = BackupManager::default();
        let mut other = entry("Other_1.sql");
        other.connection_name = "Warehouse".into();
        other.driver = "Postgres".into();
        other.note = None;
        m.entries = vec![entry("Shop_1.sqlite"), other];
        assert_eq!(m.entries_matching().len(), 2);
        m.filter = "ware".into();
        assert_eq!(m.entries_matching().len(), 1);
        m.filter = "MIGRATION".into();
        assert_eq!(m.entries_matching()[0].file_name, "Shop_1.sqlite");
        m.filter = "postgres".into();
        assert_eq!(m.entries_matching()[0].file_name, "Other_1.sql");
        m.filter = "zzz".into();
        assert!(m.entries_matching().is_empty());
        assert_eq!(m.summary(), "0 of 2 backups");
        m.filter.clear();
        assert_eq!(m.summary(), "2 backups, 4.0 KB");
        m.entries.truncate(1);
        assert_eq!(m.summary(), "1 backup, 2.0 KB");
    }

    #[test]
    fn a_row_names_the_file_and_its_facts_and_carries_a_failure_reason() {
        let r = row_view(&entry("Shop_1.sqlite"));
        assert_eq!(r.title, "Shop_1.sqlite");
        assert!(r.subtitle.starts_with("Shop  ·  Sqlite  ·  2.0 KB  ·  "), "{}", r.subtitle);
        assert!(r.subtitle.ends_with("\"before migration\""), "{}", r.subtitle);
        assert!(r.ok);
        assert_eq!(r.status, "Completed");
        assert_eq!(r.note, "");

        let mut bad = entry("x.sql");
        bad.status = BackupStatus::Failed("disk full".into());
        bad.connection_name.clear();
        bad.driver.clear();
        bad.note = None;
        let r = row_view(&bad);
        assert!(!r.ok);
        assert_eq!(r.note, "disk full");
        assert!(r.subtitle.starts_with("0 B") || r.subtitle.starts_with("2.0 KB"), "{}", r.subtitle);
    }

    #[test]
    fn a_restore_is_blocked_with_a_reason_when_it_cannot_work() {
        let dir = std::env::temp_dir().join(format!("dbhive-bm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.sqlite");
        std::fs::write(&file, b"x").unwrap();
        let mut e = entry("a.sqlite");
        e.file_path = file.to_string_lossy().to_string();

        assert_eq!(restore_blocker(&e, Some(&DbDriver::Sqlite)), None);
        assert!(restore_blocker(&e, None).unwrap().contains("Open a connection"));
        assert!(restore_blocker(&e, Some(&DbDriver::Redis)).unwrap().contains("not supported"));
        let mut failed = e.clone();
        failed.status = BackupStatus::Failed("x".into());
        assert!(restore_blocker(&failed, Some(&DbDriver::Sqlite)).unwrap().contains("failed"));
        let mut gone = e.clone();
        gone.file_path = dir.join("gone.sqlite").to_string_lossy().to_string();
        assert!(restore_blocker(&gone, Some(&DbDriver::Sqlite)).unwrap().contains("no longer exists"));
    }

    #[test]
    fn restore_warnings_name_what_is_lost_and_flag_a_driver_mismatch() {
        let e = entry("a.sqlite"); // driver "Sqlite"
        let w = restore_warnings(&e, "Shop", "Sqlite", "shop.db", false);
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("shop.db") && w[0].contains("overwritten"));

        let w = restore_warnings(&e, "Prod", "Postgres", "app", true);
        assert_eq!(w.len(), 2);
        assert!(w[0].contains("dropped and recreated"));
        assert!(w[1].contains("Sqlite") && w[1].contains("Postgres"));

        // An unknown source driver gives nothing to compare.
        let mut unknown = e.clone();
        unknown.driver.clear();
        assert_eq!(restore_warnings(&unknown, "Prod", "Postgres", "app", false).len(), 1);
    }

    #[test]
    fn drop_and_recreate_cannot_be_switched_on_for_sqlite() {
        let mut m = BackupManager::default();
        m.set_drop_existing(true, Some(&DbDriver::Sqlite));
        assert!(!m.drop_existing);
        m.set_drop_existing(true, Some(&DbDriver::Postgres));
        assert!(m.drop_existing);
        m.set_drop_existing(false, Some(&DbDriver::Postgres));
        assert!(!m.drop_existing);
    }

    #[test]
    fn opening_a_restore_resets_the_destructive_toggle() {
        let mut m = BackupManager::default();
        m.drop_existing = true;
        m.open_restore(entry("a.sql"));
        assert!(!m.drop_existing, "a destructive option never carries over");
        assert_eq!(m.view, View::Restore);
    }

    #[test]
    fn the_delete_message_names_the_file_and_size() {
        let m = delete_message(&entry("Shop_1.sqlite"));
        assert!(m.contains("Shop_1.sqlite") && m.contains("2.0 KB") && m.contains("cannot be undone"));
    }
}
