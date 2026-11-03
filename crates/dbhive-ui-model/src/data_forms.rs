//! The plain forms of the import/export panel: export a table, dump a schema
//! to SQL, replay a SQL file. Each form holds what was typed, validates it,
//! and turns it into the options the core service takes.
//!
//! The CSV/Excel import wizard is bigger and lives in `import_wizard`.

use std::path::{Path, PathBuf};

use dbhive_core::models::DbDriver;
use dbhive_core::service_import_export::{
    self as svc, CsvOptions, ExportFormat, ExportOptions, SqlExportOptions, SqlImportOptions,
};

use crate::import_export::{default_file_name, Tab, DELIMITERS};

/// The panel's tabs, left to right.
pub const TABS: [Tab; 4] = [Tab::Import, Tab::SqlImport, Tab::Export, Tab::Dump];

pub fn tab_label(tab: Tab) -> &'static str {
    match tab {
        Tab::Import => "Import data",
        Tab::SqlImport => "Run SQL file",
        Tab::Export => "Export table",
        Tab::Dump => "SQL dump",
    }
}

pub fn tab_labels() -> Vec<String> {
    TABS.iter().map(|t| tab_label(*t).to_string()).collect()
}

pub fn tab_index(tab: Tab) -> i32 {
    TABS.iter().position(|t| *t == tab).map_or(0, |i| i as i32)
}

pub fn tab_at(index: i32) -> Tab {
    usize::try_from(index)
        .ok()
        .and_then(|i| TABS.get(i))
        .copied()
        .unwrap_or(Tab::Import)
}

/// A leading `~` as the home directory, since nothing else will expand it:
/// the paths typed here are handed straight to the file APIs.
pub fn expand_home(path: &str) -> String {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok();
    expand_home_with(path, home.as_deref())
}

pub fn expand_home_with(path: &str, home: Option<&str>) -> String {
    let p = path.trim();
    match home {
        Some(h) if p == "~" => h.to_string(),
        Some(h) if p.starts_with("~/") || p.starts_with("~\\") => {
            format!("{}/{}", h.trim_end_matches(['/', '\\']), &p[2..])
        }
        _ => p.to_string(),
    }
}

/// A date-stamped path for a new file in `dir`: `<dir>/<stem>_<today>.<ext>`.
pub fn suggest_path(dir: &Path, stem: &str, extension: &str, today: &str) -> String {
    dir.join(default_file_name(stem, extension, today))
        .to_string_lossy()
        .to_string()
}

/// Why `path` cannot be written, or `None` when it can. Checked before a job
/// starts so the error appears next to the field, not after a spinner.
pub fn output_path_problem(path: &str, extension: &str) -> Option<String> {
    let expanded = expand_home(path);
    let path = expanded.as_str();
    if path.is_empty() {
        return Some("Enter the path of the file to write.".to_string());
    }
    let p = PathBuf::from(path);
    if !p.is_absolute() {
        return Some("Use a full path, starting from the root of the disk.".to_string());
    }
    if p.is_dir() {
        return Some("That path is a folder; add a file name to it.".to_string());
    }
    match p.parent() {
        Some(parent) if parent.is_dir() => {}
        _ => return Some("The folder does not exist.".to_string()),
    }
    let has_ext = p
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(extension));
    if !has_ext {
        return Some(format!("The file name should end in .{extension}."));
    }
    None
}

/// Why `path` cannot be read as an input file.
pub fn input_path_problem(path: &str) -> Option<String> {
    let expanded = expand_home(path);
    let path = expanded.as_str();
    if path.is_empty() {
        return Some("Enter the full path of the file.".to_string());
    }
    let p = Path::new(path);
    if p.is_dir() {
        return Some("That path is a folder, not a file.".to_string());
    }
    if !p.is_file() {
        return Some("No file exists at that path.".to_string());
    }
    None
}

// ---------------------------------------------------------------------------
// Export a table
// ---------------------------------------------------------------------------

/// Formats offered when exporting a table. SQL INSERT output is the dump's
/// job, so it is not repeated here.
pub const TABLE_FORMATS: [ExportFormat; 3] =
    [ExportFormat::Csv, ExportFormat::Json, ExportFormat::Excel];

pub fn table_format_labels() -> Vec<String> {
    TABLE_FORMATS
        .iter()
        .map(|f| f.label().to_string())
        .collect()
}

#[derive(Debug, Clone)]
pub struct TableExportForm {
    pub table: String,
    pub format: usize,
    pub path: String,
    pub delimiter: usize,
    pub include_headers: bool,
    pub limit_text: String,
}

impl Default for TableExportForm {
    fn default() -> Self {
        Self {
            table: String::new(),
            format: 0,
            path: String::new(),
            delimiter: 0,
            include_headers: true,
            limit_text: String::new(),
        }
    }
}

impl TableExportForm {
    pub fn format(&self) -> ExportFormat {
        TABLE_FORMATS
            .get(self.format)
            .copied()
            .unwrap_or(ExportFormat::Csv)
    }

    pub fn is_csv(&self) -> bool {
        self.format() == ExportFormat::Csv
    }

    /// Blank is "every row".
    pub fn row_limit(&self) -> Result<Option<usize>, String> {
        let t = self.limit_text.trim();
        if t.is_empty() {
            return Ok(None);
        }
        match t.parse::<usize>() {
            Ok(n) if n > 0 => Ok(Some(n)),
            _ => Err(format!(
                "Row limit must be a positive whole number, got \"{t}\""
            )),
        }
    }

    /// Everything wrong with the form, as one message, or the options to run.
    pub fn options(&self, schema: &str) -> Result<ExportOptions, String> {
        if self.table.trim().is_empty() {
            return Err("Choose a table to export.".to_string());
        }
        if let Some(p) = output_path_problem(&self.path, self.format().extension()) {
            return Err(p);
        }
        let limit = self.row_limit()?;
        Ok(ExportOptions {
            format: self.format(),
            path: expand_home(&self.path),
            csv: CsvOptions {
                delimiter: DELIMITERS.get(self.delimiter).map_or(',', |(_, c)| *c),
                include_headers: self.include_headers,
                ..CsvOptions::default()
            },
            sheet_name: self.table.clone(),
            sql_table: self.table.clone(),
            sql_schema: schema.to_string(),
            row_limit: limit,
            ..ExportOptions::default()
        })
    }

    /// The default file name for the current table and format.
    pub fn suggested_path(&self, dir: &Path, today: &str) -> String {
        let stem = if self.table.is_empty() {
            "export"
        } else {
            self.table.as_str()
        };
        suggest_path(dir, stem, self.format().extension(), today)
    }

    /// Swap the extension of the path when the format changes, so the path
    /// never says `.csv` over a JSON file. Only touches a path that was one
    /// of ours (its extension is one of the known export extensions).
    pub fn retarget_extension(&mut self) {
        let ext = self.format().extension();
        let p = Path::new(self.path.trim());
        let known = ["csv", "json", "xls"];
        if p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| known.contains(&e))
        {
            self.path = p.with_extension(ext).to_string_lossy().to_string();
        }
    }
}

// ---------------------------------------------------------------------------
// SQL dump
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DumpForm {
    pub include_drop: bool,
    pub include_create: bool,
    pub include_data: bool,
    /// Comma separated; blank means every table.
    pub tables_text: String,
    pub path: String,
}

impl Default for DumpForm {
    fn default() -> Self {
        let d = SqlExportOptions::default();
        Self {
            include_drop: d.include_drop,
            include_create: d.include_create,
            include_data: d.include_data,
            tables_text: String::new(),
            path: String::new(),
        }
    }
}

impl DumpForm {
    pub fn options(&self, schema: &str) -> Result<SqlExportOptions, String> {
        if !self.include_drop && !self.include_create && !self.include_data {
            return Err("Choose at least one of DROP, CREATE or INSERT.".to_string());
        }
        if let Some(p) = output_path_problem(&self.path, "sql") {
            return Err(p);
        }
        Ok(SqlExportOptions {
            include_drop: self.include_drop,
            include_create: self.include_create,
            include_data: self.include_data,
            tables: svc::parse_table_filter(&self.tables_text),
            schema: (!schema.is_empty()).then(|| schema.to_string()),
        })
    }

    pub fn suggested_path(&self, dir: &Path, database: &str, today: &str) -> String {
        suggest_path(
            dir,
            if database.is_empty() {
                "dump"
            } else {
                database
            },
            "sql",
            today,
        )
    }

    /// "All tables" or "3 tables: a, b, c".
    pub fn scope_summary(&self) -> String {
        let t = svc::parse_table_filter(&self.tables_text);
        match t.len() {
            0 => "Every table in the schema".to_string(),
            1 => format!("1 table: {}", t[0]),
            n => format!("{n} tables: {}", t.join(", ")),
        }
    }
}

/// What a dump cannot do for this kind of database, or blank.
pub fn dump_note(driver: &DbDriver) -> String {
    let s = svc::dump_support(driver);
    if !s.drop && !s.create && !s.data {
        "SQL dumps are not available for this kind of database.".to_string()
    } else if !s.create {
        "This database cannot write CREATE TABLE; the file carries a comment in its place."
            .to_string()
    } else {
        String::new()
    }
}

// ---------------------------------------------------------------------------
// Run a SQL file
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SqlFileForm {
    pub path: String,
    pub continue_on_error: bool,
    pub use_transaction: bool,
}

impl Default for SqlFileForm {
    fn default() -> Self {
        let d = SqlImportOptions::default();
        Self {
            path: String::new(),
            continue_on_error: d.continue_on_error,
            use_transaction: d.use_transaction,
        }
    }
}

impl SqlFileForm {
    pub fn options(&self) -> Result<SqlImportOptions, String> {
        if let Some(p) = input_path_problem(&self.path) {
            return Err(p);
        }
        Ok(SqlImportOptions {
            continue_on_error: self.continue_on_error,
            use_transaction: self.use_transaction,
        })
    }

    /// A transaction that continues past errors commits the statements that
    /// worked; say so, because "all or nothing" is what the word suggests.
    pub fn option_note(&self) -> String {
        match (self.use_transaction, self.continue_on_error) {
            (true, false) => "All or nothing: the first error rolls everything back.".to_string(),
            (true, true) => {
                "Errors are logged and skipped; the statements that worked are committed."
                    .to_string()
            }
            (false, false) => {
                "Stops at the first error. Statements already run stay applied.".to_string()
            }
            (false, true) => "Errors are logged and skipped. Nothing is rolled back.".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dbhive-forms-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn tabs_round_trip_through_their_index() {
        for (i, t) in TABS.iter().enumerate() {
            assert_eq!(tab_index(*t), i as i32);
            assert_eq!(tab_at(i as i32), *t);
        }
        assert_eq!(tab_at(-1), Tab::Import);
        assert_eq!(tab_at(99), Tab::Import);
        assert_eq!(tab_labels().len(), 4);
    }

    #[test]
    fn a_leading_tilde_is_the_home_directory() {
        assert_eq!(
            expand_home_with("~/Downloads/a.csv", Some("/Users/me")),
            "/Users/me/Downloads/a.csv"
        );
        assert_eq!(expand_home_with("  ~ ", Some("/Users/me")), "/Users/me");
        assert_eq!(expand_home_with("~/x", Some("/Users/me/")), "/Users/me/x");
        assert_eq!(expand_home_with("~other/x", Some("/Users/me")), "~other/x");
        assert_eq!(expand_home_with("/abs/~/x", Some("/Users/me")), "/abs/~/x");
        assert_eq!(expand_home_with("~/x", None), "~/x");
    }

    #[test]
    fn a_suggested_path_is_date_stamped_in_the_folder() {
        let p = suggest_path(
            Path::new("/home/u/Downloads"),
            "order details",
            "csv",
            "2026-10-21",
        );
        assert_eq!(p, "/home/u/Downloads/order_details_2026-10-21.csv");
    }

    #[test]
    fn output_paths_are_checked_before_anything_runs() {
        let dir = temp_dir("out");
        assert!(output_path_problem("", "sql")
            .unwrap()
            .contains("Enter the path"));
        assert!(output_path_problem("rel/x.sql", "sql")
            .unwrap()
            .contains("full path"));
        assert!(output_path_problem(dir.to_str().unwrap(), "sql")
            .unwrap()
            .contains("folder"));
        let missing = dir.join("nope").join("x.sql");
        assert!(output_path_problem(missing.to_str().unwrap(), "sql")
            .unwrap()
            .contains("does not exist"));
        let wrong_ext = dir.join("x.txt");
        assert!(output_path_problem(wrong_ext.to_str().unwrap(), "sql")
            .unwrap()
            .contains(".sql"));
        let good = dir.join("x.SQL");
        assert_eq!(output_path_problem(good.to_str().unwrap(), "sql"), None);
    }

    #[test]
    fn input_paths_must_be_existing_files() {
        let dir = temp_dir("in");
        assert!(input_path_problem("").is_some());
        assert!(input_path_problem(dir.to_str().unwrap())
            .unwrap()
            .contains("folder"));
        let f = dir.join("a.sql");
        assert!(input_path_problem(f.to_str().unwrap())
            .unwrap()
            .contains("No file"));
        std::fs::write(&f, "SELECT 1;").unwrap();
        assert_eq!(input_path_problem(f.to_str().unwrap()), None);
    }

    #[test]
    fn a_table_export_form_becomes_export_options() {
        let dir = temp_dir("tbl");
        let mut f = TableExportForm {
            table: "orders".into(),
            ..Default::default()
        };
        f.path = dir.join("orders.csv").to_string_lossy().to_string();
        f.delimiter = 1;
        f.include_headers = false;
        f.limit_text = "500".into();
        let o = f.options("public").unwrap();
        assert_eq!(o.format, ExportFormat::Csv);
        assert_eq!(o.csv.delimiter, ';');
        assert!(!o.csv.include_headers);
        assert_eq!(o.row_limit, Some(500));
        assert_eq!(o.sql_table, "orders");
    }

    #[test]
    fn a_table_export_form_reports_its_first_problem() {
        let mut f = TableExportForm::default();
        assert!(f.options("").unwrap_err().contains("Choose a table"));
        f.table = "t".into();
        assert!(f.options("").unwrap_err().contains("Enter the path"));
        let dir = temp_dir("tbl2");
        f.path = dir.join("t.csv").to_string_lossy().to_string();
        f.limit_text = "0".into();
        assert!(f.options("").unwrap_err().contains("positive"));
        f.limit_text = "".into();
        assert_eq!(f.row_limit(), Ok(None));
        f.limit_text = "abc".into();
        assert!(f.row_limit().is_err());
    }

    #[test]
    fn changing_the_format_retargets_the_extension_of_our_own_paths_only() {
        let mut f = TableExportForm {
            path: "/x/orders_2026.csv".into(),
            ..Default::default()
        };
        f.format = 1;
        f.retarget_extension();
        assert_eq!(f.path, "/x/orders_2026.json");
        f.format = 2;
        f.retarget_extension();
        assert_eq!(f.path, "/x/orders_2026.xls");
        // A path the user typed with some other extension is left alone.
        f.path = "/x/custom.dat".into();
        f.format = 0;
        f.retarget_extension();
        assert_eq!(f.path, "/x/custom.dat");
    }

    #[test]
    fn the_dump_form_needs_a_statement_kind_and_a_sql_path() {
        let dir = temp_dir("dump");
        let mut f = DumpForm::default();
        assert!(f.include_create && f.include_data && !f.include_drop);
        f.path = dir.join("shop.sql").to_string_lossy().to_string();
        f.tables_text = " a, b ,,".into();
        f.include_drop = true;
        let o = f.options("public").unwrap();
        assert!(o.include_drop && o.include_create && o.include_data);
        assert_eq!(o.tables, vec!["a", "b"]);
        assert_eq!(o.schema.as_deref(), Some("public"));
        assert_eq!(f.options("").unwrap().schema, None);

        f.include_drop = false;
        f.include_create = false;
        f.include_data = false;
        assert!(f.options("public").unwrap_err().contains("at least one"));
    }

    #[test]
    fn the_dump_scope_reads_back_what_was_typed() {
        let mut f = DumpForm::default();
        assert_eq!(f.scope_summary(), "Every table in the schema");
        f.tables_text = "orders".into();
        assert_eq!(f.scope_summary(), "1 table: orders");
        f.tables_text = "a, b".into();
        assert_eq!(f.scope_summary(), "2 tables: a, b");
        assert_eq!(
            f.suggested_path(Path::new("/d"), "shop", "2026-10-21"),
            "/d/shop_2026-10-21.sql"
        );
        assert_eq!(
            f.suggested_path(Path::new("/d"), "", "2026-10-21"),
            "/d/dump_2026-10-21.sql"
        );
    }

    #[test]
    fn the_dump_note_names_what_the_database_cannot_do() {
        assert_eq!(dump_note(&DbDriver::Postgres), "");
        assert_eq!(dump_note(&DbDriver::Sqlite), "");
        assert!(dump_note(&DbDriver::SqlServer).contains("CREATE TABLE"));
        assert!(dump_note(&DbDriver::Redis).contains("not available"));
    }

    #[test]
    fn a_sql_file_form_needs_an_existing_file_and_explains_its_options() {
        let dir = temp_dir("sqlfile");
        let mut f = SqlFileForm::default();
        assert!(f.options().is_err());
        let file = dir.join("d.sql");
        std::fs::write(&file, "SELECT 1;").unwrap();
        f.path = file.to_string_lossy().to_string();
        let o = f.options().unwrap();
        assert!(o.use_transaction && !o.continue_on_error);
        assert!(f.option_note().starts_with("All or nothing"));
        f.continue_on_error = true;
        assert!(f.option_note().contains("committed"));
        f.use_transaction = false;
        assert!(f.option_note().contains("Nothing is rolled back"));
        f.continue_on_error = false;
        assert!(f.option_note().contains("Stops at the first error"));
    }
}
