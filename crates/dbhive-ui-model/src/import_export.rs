//! Import and export view logic: delimiter labels, file naming, plan and
//! preview summaries, outcome classification, the target-column projection,
//! and the wizard's tab, source and step enums.
//!
//! `screens::import_export`, which keeps the drawing,
//! the jobs and the per-screen state. The import/export engine itself stays
//! in `dbhive_core::service_import_export`.

use dbhive_core::drivers::QueryResult;
use dbhive_core::service_import_export as svc;
use svc::{ColumnMapping, ImportPreview, ImportResult, SqlImportResult, TableColumnInfo};

/// Rows loaded when previewing a table before exporting it. Enough to see the
/// shape of the data without pulling the whole table twice.
pub const TABLE_PREVIEW_ROWS: usize = 100;

// ---------------------------------------------------------------------------
// Pure logic: paths, labels, projections
// ---------------------------------------------------------------------------

/// Delimiters the UI wizard offered, as (label, char).
pub const DELIMITERS: [(&str, char); 4] = [
    ("Comma  ,", ','),
    ("Semicolon  ;", ';'),
    ("Tab", '\t'),
    ("Pipe  |", '|'),
];

pub fn delimiter_label(d: char) -> String {
    DELIMITERS
        .iter()
        .find(|(_, c)| *c == d)
        .map(|(l, _)| l.to_string())
        .unwrap_or_else(|| format!("'{}'", d))
}

/// A default file name, date-stamped the way the UI save dialog was.
pub fn default_file_name(stem: &str, extension: &str, today: &str) -> String {
    let safe: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe = safe.trim_matches('_');
    let stem = if safe.is_empty() { "export" } else { safe };
    format!("{}_{}.{}", stem, today, extension)
}

/// "3 of 40 columns mapped" and friends, so the mapping step says at a glance
/// whether it is complete.
pub fn mapping_summary(mappings: &[ColumnMapping]) -> String {
    let total = mappings.len();
    let mapped = svc::active_mappings(mappings).len();
    if total == 0 {
        "No columns".to_string()
    } else {
        format!("{} of {} columns will be imported", mapped, total)
    }
}

/// One line describing what a preview found.
pub fn preview_summary(preview: &ImportPreview) -> String {
    let rows = match preview.total_rows {
        Some(n) => format!("{} rows", n),
        // A CSV row count needs a full pass; the exact figure appears on the
        // confirmation step, once the file has actually been read.
        None => format!("first {} rows shown", preview.rows.len()),
    };
    format!(
        "{} · {} columns · {}",
        preview.file_type.to_uppercase(),
        preview.columns.len(),
        rows
    )
}

/// Outcome banner for a finished dump import, colour-coded like the UI
/// dialog: cancelled, partial, or clean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Success,
    Partial,
    Cancelled,
}

pub fn sql_import_outcome(result: &SqlImportResult) -> Outcome {
    if result.cancelled {
        Outcome::Cancelled
    } else if result.errors_count > 0 {
        Outcome::Partial
    } else {
        Outcome::Success
    }
}
pub fn sql_import_summary(result: &SqlImportResult) -> String {
    format!(
        "{} statements executed, {} skipped, {} errors{}",
        result.executed,
        result.skipped,
        result.errors_count,
        if result.cancelled { " (stopped)" } else { "" }
    )
}

pub fn import_summary(result: &ImportResult) -> String {
    format!(
        "{} rows imported, {} failed",
        result.rows_imported, result.rows_failed
    )
}

/// The target columns as a grid, so the mapping step can show what the table
/// expects without a second table widget.
pub fn target_columns_result(columns: &[TableColumnInfo]) -> QueryResult {
    let rows = columns
        .iter()
        .map(|c| {
            vec![
                serde_json::Value::String(c.name.clone()),
                serde_json::Value::String(c.data_type.clone()),
                serde_json::Value::String(if c.nullable { "YES" } else { "NO" }.to_string()),
                serde_json::Value::String(if c.is_primary_key { "PK" } else { "" }.to_string()),
            ]
        })
        .collect();
    QueryResult::with_data(
        vec![
            "Column".into(),
            "Type".into(),
            "Nullable".into(),
            "Key".into(),
        ],
        rows,
    )
}
/// Keep a long generated statement readable without hiding what it does to.
pub fn truncate_middle(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max * 2 / 3).collect();
    let tail: String = s
        .chars()
        .skip(s.chars().count().saturating_sub(max / 4))
        .collect();
    format!("{} … [{} chars] … {}", head, s.chars().count(), tail)
}
// ---------------------------------------------------------------------------
// Wizard enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// Result sets and tables out, as CSV/JSON/SQL/Excel.
    Export,
    /// A whole schema out, as a SQL dump.
    Dump,
    /// A CSV or spreadsheet into a table.
    Import,
    /// A `.sql` file replayed against the connection.
    SqlImport,
}

/// What the export tab reads from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportSource {
    /// The result in the workspace's active query tab.
    CurrentResult,
    /// A table, read fresh.
    Table,
}

/// The import wizard's linear steps, same shape as the UI wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    File,
    Preview,
    Mapping,
    Options,
    /// Show the plan; nothing is written before this is accepted.
    Confirm,
    Running,
    Complete,
}

impl Step {
    pub const ORDER: [Step; 7] = [
        Step::File,
        Step::Preview,
        Step::Mapping,
        Step::Options,
        Step::Confirm,
        Step::Running,
        Step::Complete,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Step::File => "File",
            Step::Preview => "Preview",
            Step::Mapping => "Mapping",
            Step::Options => "Options",
            Step::Confirm => "Confirm",
            Step::Running => "Import",
            Step::Complete => "Done",
        }
    }

    pub fn index(self) -> usize {
        Step::ORDER.iter().position(|s| *s == self).unwrap_or(0)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn mapping(source: &str, target: &str, skip: bool) -> ColumnMapping {
        ColumnMapping {
            source_column: source.to_string(),
            target_column: target.to_string(),
            target_type: None,
            default_value: None,
            skip,
        }
    }
    #[test]
    fn a_suggested_file_name_is_date_stamped_and_safe() {
        assert_eq!(
            default_file_name("query_results", "csv", "2026-09-08"),
            "query_results_2026-09-08.csv"
        );
        // A table called `order details` must not produce a path with a
        // space or a slash in it.
        assert_eq!(
            default_file_name("order details/v2", "json", "2026-09-08"),
            "order_details_v2_2026-09-08.json"
        );
        assert_eq!(
            default_file_name("///", "sql", "2026-09-08"),
            "export_2026-09-08.sql"
        );
    }

    #[test]
    fn delimiter_labels_cover_the_offered_set_and_fall_back() {
        assert_eq!(delimiter_label(','), "Comma  ,");
        assert_eq!(delimiter_label('\t'), "Tab");
        assert_eq!(delimiter_label('#'), "'#'");
    }

    #[test]
    fn the_mapping_summary_counts_only_columns_that_will_be_written() {
        let mappings = vec![
            mapping("a", "a", false),
            mapping("b", "b", true),
            mapping("c", "", false),
        ];
        assert_eq!(
            mapping_summary(&mappings),
            "1 of 3 columns will be imported"
        );
        assert_eq!(mapping_summary(&[]), "No columns");
    }

    #[test]
    fn a_preview_summary_says_when_the_row_count_is_unknown() {
        let mut preview = ImportPreview {
            columns: vec!["a".into(), "b".into()],
            rows: vec![vec!["1".into(), "2".into()]],
            total_rows: None,
            detected_types: vec!["INTEGER".into(), "INTEGER".into()],
            file_type: "csv".into(),
            sheet_names: None,
        };
        assert_eq!(
            preview_summary(&preview),
            "CSV · 2 columns · first 1 rows shown"
        );
        preview.total_rows = Some(940);
        preview.file_type = "xlsx".into();
        assert_eq!(preview_summary(&preview), "XLSX · 2 columns · 940 rows");
    }

    #[test]
    fn a_dump_import_outcome_separates_cancelled_from_partial() {
        let base = SqlImportResult {
            executed: 10,
            errors_count: 0,
            skipped: 0,
            first_error: None,
            cancelled: false,
            log_file: None,
        };
        assert_eq!(sql_import_outcome(&base), Outcome::Success);
        assert_eq!(
            sql_import_outcome(&SqlImportResult {
                errors_count: 2,
                ..base.clone()
            }),
            Outcome::Partial
        );
        assert_eq!(
            sql_import_outcome(&SqlImportResult {
                errors_count: 2,
                cancelled: true,
                ..base
            }),
            Outcome::Cancelled,
            "a stopped import is reported as stopped, not as a failure"
        );
    }

    #[test]
    fn summaries_report_both_counts() {
        let result = ImportResult {
            rows_imported: 998,
            rows_failed: 2,
            errors: vec!["Row 3: boom".into()],
            success: false,
        };
        assert_eq!(import_summary(&result), "998 rows imported, 2 failed");

        let sql = SqlImportResult {
            executed: 5,
            errors_count: 1,
            skipped: 2,
            first_error: None,
            cancelled: true,
            log_file: None,
        };
        assert_eq!(
            sql_import_summary(&sql),
            "5 statements executed, 2 skipped, 1 errors (stopped)"
        );
    }

    #[test]
    fn steps_are_ordered_and_labelled() {
        assert_eq!(Step::File.index(), 0);
        assert_eq!(Step::Confirm.index(), 4);
        assert_eq!(Step::Complete.index(), Step::ORDER.len() - 1);
        assert_eq!(Step::Confirm.label(), "Confirm");
    }

    #[test]
    fn target_columns_project_into_a_grid_result() {
        let result = target_columns_result(&[TableColumnInfo {
            name: "id".into(),
            data_type: "integer".into(),
            nullable: false,
            default_value: None,
            is_primary_key: true,
        }]);
        assert_eq!(result.columns.len(), 4);
        assert_eq!(result.rows[0][0], serde_json::json!("id"));
        assert_eq!(result.rows[0][2], serde_json::json!("NO"));
        assert_eq!(result.rows[0][3], serde_json::json!("PK"));
    }

    #[test]
    fn a_long_statement_is_shortened_but_keeps_both_ends() {
        let long = format!("INSERT INTO t VALUES {}end", "x".repeat(1000));
        let short = truncate_middle(&long, 100);
        assert!(short.starts_with("INSERT INTO t VALUES"), "{}", short);
        assert!(short.ends_with("end"), "{}", short);
        assert!(short.len() < long.len());
        // Anything short enough is returned untouched.
        assert_eq!(truncate_middle("short", 100), "short");
    }
}
