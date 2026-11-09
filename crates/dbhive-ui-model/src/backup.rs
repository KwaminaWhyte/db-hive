//! Backup manager: the UI-independent part.
//!
//! Formatting, the create form, matching listed files back to a profile, and
//! the projection of the list into the results grid.
//!
//! The work itself (dump, restore, list, delete, validation) lives in
//! `dbhive_core::service_backup`. What is here is formatting, the create
//! form and its mapping onto `BackupOptions`, matching listed files back to
//! their profile, and the projection of the list into the one results grid.

use std::time::Duration;

use dbhive_core::drivers::QueryResult;
use dbhive_core::models::backup::{BackupEntry, BackupOptions, BackupStatus};
use dbhive_core::models::DbDriver;
use dbhive_core::service_backup as svc;

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

/// Human-readable byte count, matching the `formatBytes` behaviour:
/// whole bytes below 1 KB, one decimal above.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

/// Elapsed time for an operation in flight, or one that just finished.
///
/// Sub-second work reads in milliseconds; anything a user would wait through
/// reads in seconds, then minutes, because "312.4s" is not a useful thing to
/// stare at during a large dump.
pub fn format_duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        return format!("{} ms", ms);
    }
    let secs = d.as_secs();
    if secs < 60 {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

/// A Unix timestamp as local date and time. Zero means "unknown", which is
/// what `list_backups` reports for a file with no readable mtime.
pub fn format_timestamp(unix_secs: i64) -> String {
    if unix_secs <= 0 {
        return "-".to_string();
    }
    match chrono::DateTime::from_timestamp(unix_secs, 0) {
        Some(dt) => dt
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        None => "-".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Create form
// ---------------------------------------------------------------------------

/// Split the typed table list. Commas and newlines both separate, blank
/// entries are dropped, so a trailing comma is not an error.
pub fn parse_tables(input: &str) -> Vec<String> {
    input
        .split(['\n', ','])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Empty text means "not set" rather than "set to the empty string".
pub fn optional(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// The create-backup form, as typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupForm {
    pub include_schema: bool,
    pub include_data: bool,
    pub note: String,
    /// Comma or newline separated; empty means every table.
    pub tables: String,
    /// Typed absolute path; empty means the default backup directory.
    pub output_dir: String,
}

impl Default for BackupForm {
    fn default() -> Self {
        Self {
            // Both on: the default backup is a complete one.
            include_schema: true,
            include_data: true,
            note: String::new(),
            tables: String::new(),
            output_dir: String::new(),
        }
    }
}

impl BackupForm {
    /// The options the form currently describes.
    pub fn options(&self) -> BackupOptions {
        BackupOptions {
            include_data: self.include_data,
            include_schema: self.include_schema,
            tables: parse_tables(&self.tables),
            note: optional(&self.note),
            output_dir: optional(&self.output_dir),
        }
    }
}

// ---------------------------------------------------------------------------
// Status and restore confirmation
// ---------------------------------------------------------------------------

/// Status as displayed, plus the failure reason if there is one.
pub fn status_label(status: &BackupStatus) -> (&'static str, Option<&str>) {
    match status {
        BackupStatus::Completed => ("Completed", None),
        BackupStatus::Failed(reason) => ("Failed", Some(reason.as_str())),
    }
}

/// Whether "drop and recreate first" means anything for the target. SQLite
/// restore replaces the whole file, so the toggle has no meaning there.
pub fn drop_existing_applies(driver: Option<&DbDriver>) -> bool {
    !matches!(driver, Some(DbDriver::Sqlite))
}

/// The restore confirmation button, which names the database it will drop.
pub fn restore_button_label(drop_existing: bool, db_label: &str) -> String {
    if drop_existing {
        format!("Drop {} and restore", db_label)
    } else {
        "Restore".to_string()
    }
}

// ---------------------------------------------------------------------------
// Listing
// ---------------------------------------------------------------------------

/// Fill in the connection and driver of listed backups from the saved
/// profiles, given as (id, name, driver label).
///
/// `list_backups` only sees filenames, so it cannot know which profile a file
/// came from. The filename carries the connection name with every
/// non-alphanumeric character replaced, so the match has to be made on that
/// same reduction: `prod/orders` and `prod orders` both land on
/// `prod_orders`, and an ambiguous name is left blank rather than guessed.
pub fn annotate_entries(entries: &mut [BackupEntry], profiles: &[(String, String, String)]) {
    for e in entries.iter_mut() {
        if !e.driver.is_empty() {
            continue;
        }
        let key = svc::safe_name(&e.connection_name);
        let mut matches = profiles
            .iter()
            .filter(|(_, name, _)| svc::safe_name(name) == key);
        if let (Some((id, name, driver)), None) = (matches.next(), matches.next()) {
            e.connection_id = id.clone();
            e.connection_name = name.clone();
            e.driver = driver.clone();
        }
    }
}

/// Project the backup list into the shape the results grid draws.
///
/// The grid is the only table in the app, so the list goes through it
/// rather than growing a second one; it also brings sorting and filtering
/// for free, with the shared cell formatting.
pub fn entries_to_result(entries: &[BackupEntry]) -> QueryResult {
    let columns = vec![
        "File".to_string(),
        "Connection".to_string(),
        "Driver".to_string(),
        "Size".to_string(),
        "Created".to_string(),
        "Status".to_string(),
        "Note".to_string(),
    ];
    let rows = entries
        .iter()
        .map(|e| {
            let (status, reason) = status_label(&e.status);
            let status = match reason {
                Some(r) => format!("{}: {}", status, r),
                None => status.to_string(),
            };
            vec![
                serde_json::Value::String(e.file_name.clone()),
                blank_as_null(&e.connection_name),
                blank_as_null(&e.driver),
                serde_json::Value::String(format_bytes(e.size_bytes)),
                serde_json::Value::String(format_timestamp(e.created_at)),
                serde_json::Value::String(status),
                e.note
                    .as_deref()
                    .map(|n| serde_json::Value::String(n.to_string()))
                    .unwrap_or(serde_json::Value::Null),
            ]
        })
        .collect();
    QueryResult::with_data(columns, rows)
}

/// Unknown values render as the grid's NULL, which is visually distinct from
/// a real empty string.
fn blank_as_null(s: &str) -> serde_json::Value {
    if s.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(s.to_string())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, size: u64) -> BackupEntry {
        BackupEntry {
            id: "1".into(),
            connection_id: String::new(),
            connection_name: name.into(),
            driver: String::new(),
            file_path: format!("/b/{}_1700000000.sql", name),
            file_name: format!("{}_1700000000.sql", name),
            size_bytes: size,
            created_at: 1_700_000_000,
            status: BackupStatus::Completed,
            note: None,
        }
    }

    #[test]
    fn bytes_use_whole_numbers_below_a_kilobyte() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1), "1 B");
        assert_eq!(format_bytes(1023), "1023 B");
    }

    #[test]
    fn bytes_step_up_a_unit_at_a_time() {
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(format_bytes(1024u64.pow(4)), "1.0 TB");
        // Nothing above TB, so a petabyte keeps counting in TB rather than
        // running off the end of the unit table.
        assert_eq!(format_bytes(1024u64.pow(5)), "1024.0 TB");
    }

    #[test]
    fn durations_change_unit_with_scale() {
        assert_eq!(format_duration(Duration::from_millis(0)), "0 ms");
        assert_eq!(format_duration(Duration::from_millis(999)), "999 ms");
        assert_eq!(format_duration(Duration::from_millis(1500)), "1.5s");
        assert_eq!(format_duration(Duration::from_secs(59)), "59.0s");
        assert_eq!(format_duration(Duration::from_secs(60)), "1m 00s");
        assert_eq!(format_duration(Duration::from_secs(125)), "2m 05s");
        assert_eq!(format_duration(Duration::from_secs(3600)), "60m 00s");
    }

    #[test]
    fn unknown_timestamps_render_as_a_dash() {
        assert_eq!(format_timestamp(0), "-");
        assert_eq!(format_timestamp(-1), "-");
        // A real timestamp formats to a local date; only the shape is stable
        // across machines.
        let s = format_timestamp(1_700_000_000);
        assert_eq!(s.len(), "2023-11-14 22:13".len());
        assert!(s.starts_with("2023-11-1"));
    }

    #[test]
    fn table_lists_accept_commas_newlines_and_stray_whitespace() {
        assert_eq!(parse_tables(""), Vec::<String>::new());
        assert_eq!(parse_tables("   "), Vec::<String>::new());
        assert_eq!(parse_tables("orders"), vec!["orders"]);
        assert_eq!(
            parse_tables(" orders , customers ,"),
            vec!["orders", "customers"]
        );
        assert_eq!(
            parse_tables("orders\ncustomers"),
            vec!["orders", "customers"]
        );
    }

    #[test]
    fn blank_text_fields_mean_unset_not_empty() {
        assert_eq!(optional("  "), None);
        assert_eq!(optional(""), None);
        assert_eq!(optional("  note "), Some("note".to_string()));
    }

    #[test]
    fn form_maps_onto_backup_options() {
        let mut b = BackupForm::default();
        b.note = "  before migration  ".into();
        b.tables = "orders, ,customers".into();
        b.output_dir = "  ".into();
        let o = b.options();
        assert!(o.include_schema && o.include_data);
        assert_eq!(o.note.as_deref(), Some("before migration"));
        assert_eq!(o.tables, vec!["orders", "customers"]);
        assert_eq!(o.output_dir, None, "blank means the default directory");

        b.output_dir = "/tmp/dumps".into();
        assert_eq!(b.options().output_dir.as_deref(), Some("/tmp/dumps"));
    }

    #[test]
    fn a_fresh_form_backs_up_everything() {
        let o = BackupForm::default().options();
        assert!(o.include_schema && o.include_data);
        assert!(o.tables.is_empty());
        assert_eq!(o.note, None);
        assert_eq!(o.output_dir, None);
    }

    #[test]
    fn failure_reason_travels_with_the_status() {
        assert_eq!(status_label(&BackupStatus::Completed), ("Completed", None));
        let failed = BackupStatus::Failed("pg_dump: connection refused".into());
        let (label, reason) = status_label(&failed);
        assert_eq!(label, "Failed");
        assert_eq!(reason, Some("pg_dump: connection refused"));
    }

    #[test]
    fn drop_and_recreate_does_not_apply_to_sqlite() {
        assert!(!drop_existing_applies(Some(&DbDriver::Sqlite)));
        assert!(drop_existing_applies(Some(&DbDriver::Postgres)));
        assert!(drop_existing_applies(Some(&DbDriver::MySql)));
        // No target known: the toggle stays available, as before.
        assert!(drop_existing_applies(None));
    }

    #[test]
    fn the_restore_button_names_the_database_it_drops() {
        assert_eq!(restore_button_label(false, "Shop"), "Restore");
        assert_eq!(restore_button_label(true, "Shop"), "Drop Shop and restore");
    }

    #[test]
    fn grid_projection_keeps_row_order_and_formats_cells() {
        let entries = vec![entry("Shop", 2048), entry("Other", 0)];
        let r = entries_to_result(&entries);
        assert_eq!(r.columns.len(), 7);
        assert_eq!(r.rows.len(), 2);
        assert_eq!(r.rows[0][0], serde_json::json!("Shop_1700000000.sql"));
        assert_eq!(r.rows[0][3], serde_json::json!("2.0 KB"));
        assert_eq!(r.rows[0][5], serde_json::json!("Completed"));
        // Unknown driver and absent note become NULL, which the grid renders
        // distinctly from an empty string.
        assert_eq!(r.rows[0][2], serde_json::Value::Null);
        assert_eq!(r.rows[0][6], serde_json::Value::Null);
    }

    #[test]
    fn failed_backups_show_their_reason_in_the_status_cell() {
        let mut e = entry("Shop", 10);
        e.status = BackupStatus::Failed("disk full".into());
        let r = entries_to_result(&[e]);
        assert_eq!(r.rows[0][5], serde_json::json!("Failed: disk full"));
    }

    #[test]
    fn listed_backups_are_matched_back_to_their_profile() {
        let profiles = vec![(
            "id-1".to_string(),
            "Local Postgres".to_string(),
            "Postgres".to_string(),
        )];
        let mut entries = vec![entry("Local Postgres", 1)];
        annotate_entries(&mut entries, &profiles);
        assert_eq!(entries[0].driver, "Postgres");
        assert_eq!(entries[0].connection_id, "id-1");
    }

    #[test]
    fn annotation_leaves_unknown_and_ambiguous_names_alone() {
        let profiles = vec![
            (
                "a".to_string(),
                "prod orders".to_string(),
                "MySql".to_string(),
            ),
            (
                "b".to_string(),
                "prod/orders".to_string(),
                "Postgres".to_string(),
            ),
        ];
        // Both profiles reduce to the same filename, so neither can be
        // claimed: guessing would attribute a backup to the wrong database.
        let mut entries = vec![entry("prod orders", 1)];
        annotate_entries(&mut entries, &profiles);
        assert_eq!(entries[0].driver, "");
        assert_eq!(entries[0].connection_id, "");

        let mut entries = vec![entry("Nothing Like This", 1)];
        annotate_entries(&mut entries, &profiles);
        assert_eq!(entries[0].driver, "");
    }

    #[test]
    fn annotation_never_overwrites_a_known_driver() {
        let profiles = vec![("id-1".to_string(), "Shop".to_string(), "MySql".to_string())];
        let mut entries = vec![entry("Shop", 1)];
        entries[0].driver = "Postgres".into();
        entries[0].connection_id = "original".into();
        annotate_entries(&mut entries, &profiles);
        assert_eq!(entries[0].driver, "Postgres");
        assert_eq!(entries[0].connection_id, "original");
    }
}
