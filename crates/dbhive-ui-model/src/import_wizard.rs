//! The data-import wizard as a state machine, with no toolkit in sight.
//!
//! The egui screen kept this state inline with its drawing. Here it is a
//! plain struct: the shell feeds it what the jobs return (a preview, the
//! target table's columns, a built plan, a result) and reads back what to
//! draw. Every rule that decides which step is reachable, when a mapping is
//! complete, what is typed to confirm a truncate, and how a long error list
//! is cut lives here and is tested without a window.
//!
//! The engine (reading files, building the plan, running the inserts) stays
//! in `dbhive_core::service_import_export`.

use std::sync::Arc;

use dbhive_core::service_import_export as svc;
use svc::{
    ColumnMapping, DataImportOptions, ImportPlan, ImportPreview, ImportResult, ImportSource,
    PreviewOptions, TableColumnInfo,
};

use crate::import_export::{Step, DELIMITERS};

/// File extensions the wizard can read.
pub const SUPPORTED_EXTENSIONS: [&str; 5] = ["csv", "tsv", "txt", "xlsx", "xls"];

/// Failures listed on the result screen. The engine keeps the first ten; the
/// screen shows fewer so the counts stay above the fold.
pub const ERROR_LIST_SHOWN: usize = 6;

/// Longest error line shown before it is cut in the middle.
pub const ERROR_LINE_MAX: usize = 220;

pub const DEFAULT_BATCH_SIZE: usize = 1000;
pub const MAX_BATCH_SIZE: usize = 10_000;

#[derive(Debug, Clone)]
pub struct ImportWizard {
    pub step: Step,
    pub file_path: String,
    /// Schema (Postgres) or database (MySQL) the target table lives in.
    pub schema: String,
    pub options: DataImportOptions,
    /// "Skip rows" and "Rows per statement" as typed; validated on demand so
    /// a half-typed number never blocks typing.
    pub skip_rows_text: String,
    pub batch_size_text: String,
    pub preview: Option<ImportPreview>,
    pub tables: Vec<String>,
    pub target_columns: Vec<TableColumnInfo>,
    /// The parsed file, kept so going back and forth does not re-read it.
    pub source: Option<Arc<ImportSource>>,
    pub plan: Option<ImportPlan>,
    /// Typed table name that confirms a plan which deletes rows.
    pub confirm_input: String,
    pub result: Option<ImportResult>,
    /// The last failure of the step in progress; cleared when it succeeds.
    pub error: String,
    delimiter_touched: bool,
}

impl Default for ImportWizard {
    fn default() -> Self {
        Self {
            step: Step::File,
            file_path: String::new(),
            schema: String::new(),
            options: DataImportOptions::default(),
            skip_rows_text: String::new(),
            batch_size_text: String::new(),
            preview: None,
            tables: Vec::new(),
            target_columns: Vec::new(),
            source: None,
            plan: None,
            confirm_input: String::new(),
            result: None,
            error: String::new(),
            delimiter_touched: false,
        }
    }
}

impl ImportWizard {
    pub fn new(schema: &str) -> Self {
        let schema = schema.to_string();
        Self {
            options: DataImportOptions {
                schema: (!schema.is_empty()).then(|| schema.clone()),
                ..DataImportOptions::default()
            },
            schema,
            ..Self::default()
        }
    }

    // -- File ---------------------------------------------------------------

    /// Why this path cannot be read, or `None` when it looks readable.
    pub fn file_problem(&self) -> Option<String> {
        file_problem(&self.file_path)
    }

    /// A new path means the old preview, mapping and plan describe another
    /// file. A `.tsv` switches the delimiter to tab unless the user already
    /// chose one.
    pub fn set_file_path(&mut self, path: &str) {
        self.file_path = path.to_string();
        self.preview = None;
        self.source = None;
        self.options.column_mappings.clear();
        self.options.sheet_name = None;
        self.invalidate_plan();
        if self.step != Step::File {
            self.step = Step::File;
        }
        if !self.delimiter_touched {
            self.options.delimiter = Some(if svc::file_kind(path.trim()) == "tsv" {
                '\t'
            } else {
                ','
            });
        }
    }

    pub fn set_first_row_is_header(&mut self, on: bool) {
        if self.options.first_row_is_header != on {
            self.options.first_row_is_header = on;
            // The columns change, so the cached parse and the plan are stale;
            // the preview must be read again.
            self.source = None;
            self.invalidate_plan();
        }
    }

    pub fn delimiter_index(&self) -> i32 {
        let d = self.options.delimiter.unwrap_or(',');
        DELIMITERS.iter().position(|(_, c)| *c == d).map_or(0, |i| i as i32)
    }

    pub fn set_delimiter_index(&mut self, index: i32) {
        if let Some((_, c)) = usize::try_from(index).ok().and_then(|i| DELIMITERS.get(i)) {
            self.options.delimiter = Some(*c);
            self.delimiter_touched = true;
            self.source = None;
            self.invalidate_plan();
        }
    }

    pub fn is_spreadsheet(&self) -> bool {
        matches!(svc::file_kind(self.file_path.trim()).as_str(), "xlsx" | "xls")
    }

    pub fn preview_options(&self) -> PreviewOptions {
        PreviewOptions {
            sheet_name: self.options.sheet_name.clone(),
            delimiter: self.options.delimiter,
            max_rows: svc::MAX_PREVIEW_ROWS,
            first_row_is_header: self.options.first_row_is_header,
        }
    }

    // -- Preview ------------------------------------------------------------

    pub fn sheet_names(&self) -> Vec<String> {
        self.preview
            .as_ref()
            .and_then(|p| p.sheet_names.clone())
            .unwrap_or_default()
    }

    pub fn sheet_index(&self) -> i32 {
        let names = self.sheet_names();
        match &self.options.sheet_name {
            Some(n) => names.iter().position(|s| s == n).map_or(0, |i| i as i32),
            None => 0,
        }
    }

    /// Choosing a sheet changes the rows, so the file has to be read again.
    pub fn set_sheet_index(&mut self, index: i32) -> bool {
        let names = self.sheet_names();
        let Some(name) = usize::try_from(index).ok().and_then(|i| names.get(i)) else {
            return false;
        };
        self.options.sheet_name = Some(name.clone());
        self.source = None;
        self.invalidate_plan();
        true
    }

    /// A fresh file means a fresh mapping; keeping the old one would silently
    /// map columns that no longer exist.
    pub fn apply_preview(&mut self, preview: ImportPreview) {
        self.options.column_mappings = svc::auto_map_columns(
            &preview.columns,
            &preview.detected_types,
            &self.target_columns,
        );
        if self.options.sheet_name.is_none() {
            self.options.sheet_name = preview.sheet_names.as_ref().and_then(|n| n.first().cloned());
        }
        self.preview = Some(preview);
        self.error.clear();
        self.source = None;
        self.invalidate_plan();
        self.step = Step::Preview;
    }

    // -- Mapping ------------------------------------------------------------

    pub fn set_tables(&mut self, tables: Vec<String>) {
        self.tables = tables;
    }

    pub fn table_index(&self) -> i32 {
        self.tables
            .iter()
            .position(|t| *t == self.options.table_name)
            .map_or(-1, |i| i as i32)
    }

    /// Pick the target table. Its columns arrive later through
    /// [`apply_target_columns`](Self::apply_target_columns).
    pub fn set_target_table(&mut self, name: &str) {
        if self.options.table_name == name {
            return;
        }
        self.options.table_name = name.to_string();
        self.target_columns.clear();
        // The old suggestions pointed at the old table's columns.
        for m in &mut self.options.column_mappings {
            m.target_column.clear();
            m.skip = true;
        }
        self.invalidate_plan();
    }

    pub fn set_target_table_by_index(&mut self, index: i32) -> Option<String> {
        let name = usize::try_from(index).ok().and_then(|i| self.tables.get(i))?.clone();
        self.set_target_table(&name);
        Some(name)
    }

    /// Whether the chosen table is one that does not exist yet, so the import
    /// creates it from the file's columns.
    pub fn is_new_table(&self) -> bool {
        let t = self.options.table_name.trim();
        !t.is_empty() && !self.tables.iter().any(|x| x == t)
    }

    /// A table name typed rather than picked. An existing name is the same
    /// as picking it; a new one maps every file column onto a column of the
    /// same name, typed as detected, because there is no table to match.
    /// Returns whether the name is an existing table (its columns are
    /// needed).
    pub fn set_typed_table(&mut self, name: &str) -> bool {
        let name = name.trim();
        if self.tables.iter().any(|t| t == name) {
            self.set_target_table(name);
            return true;
        }
        self.options.table_name = name.to_string();
        self.target_columns.clear();
        if let Some(p) = &self.preview {
            self.options.column_mappings = p
                .columns
                .iter()
                .enumerate()
                .map(|(i, c)| ColumnMapping {
                    source_column: c.clone(),
                    target_column: c.clone(),
                    target_type: p.detected_types.get(i).cloned().or_else(|| Some("TEXT".into())),
                    default_value: None,
                    skip: false,
                })
                .collect();
        }
        self.invalidate_plan();
        false
    }

    /// The target's columns arrived: re-suggest the mapping, but only while
    /// the user has not started editing it.
    pub fn apply_target_columns(&mut self, columns: Vec<TableColumnInfo>) {
        if let Some(p) = &self.preview {
            let untouched = self.options.column_mappings.iter().all(|m| m.target_column.is_empty());
            if untouched {
                self.options.column_mappings =
                    svc::auto_map_columns(&p.columns, &p.detected_types, &columns);
            }
        }
        self.target_columns = columns;
        self.invalidate_plan();
    }

    /// Re-run the name matching, discarding manual choices.
    pub fn auto_map(&mut self) {
        if let Some(p) = &self.preview {
            self.options.column_mappings =
                svc::auto_map_columns(&p.columns, &p.detected_types, &self.target_columns);
            self.invalidate_plan();
        }
    }

    pub fn target_names(&self) -> Vec<String> {
        self.target_columns.iter().map(|c| c.name.clone()).collect()
    }

    /// The dropdown entries for a mapping row's target: "Not imported" first,
    /// then the table's columns.
    pub fn target_choices(&self) -> Vec<String> {
        let mut v = vec![SKIP_LABEL.to_string()];
        v.extend(self.target_names());
        v
    }

    /// `choice` indexes [`target_choices`](Self::target_choices).
    pub fn set_mapping_target(&mut self, row: usize, choice: usize) {
        let choices = self.target_choices();
        let Some(name) = choices.get(choice).cloned() else { return };
        let Some(m) = self.options.column_mappings.get_mut(row) else { return };
        if choice == 0 {
            m.target_column.clear();
            m.skip = true;
        } else {
            // Adopt the target's own type so the column is written as it is
            // declared, not as the guess from the file.
            if let Some(t) = self.target_columns.iter().find(|c| c.name == name) {
                m.target_type = Some(t.data_type.clone());
            }
            m.target_column = name;
            m.skip = false;
        }
        self.invalidate_plan();
    }

    pub fn set_mapping_type(&mut self, row: usize, type_index: usize) {
        if let (Some(m), Some(t)) = (self.options.column_mappings.get_mut(row), svc::DATA_TYPES.get(type_index)) {
            m.target_type = Some((*t).to_string());
            self.invalidate_plan();
        }
    }

    pub fn set_mapping_default(&mut self, row: usize, text: &str) {
        if let Some(m) = self.options.column_mappings.get_mut(row) {
            m.default_value = (!text.is_empty()).then(|| text.to_string());
            self.invalidate_plan();
        }
    }

    pub fn toggle_skip(&mut self, row: usize) {
        if let Some(m) = self.options.column_mappings.get_mut(row) {
            m.skip = !m.skip;
            self.invalidate_plan();
        }
    }

    pub fn mapping_summary(&self) -> String {
        crate::import_export::mapping_summary(&self.options.column_mappings)
    }

    /// Whether the mapping step can be left: a table is chosen and at least
    /// one column will be written.
    pub fn mapping_ready(&self) -> bool {
        !self.options.table_name.trim().is_empty()
            && !svc::active_mappings(&self.options.column_mappings).is_empty()
    }

    pub fn mapping_hint(&self) -> String {
        if self.options.table_name.trim().is_empty() {
            "Choose a target table.".to_string()
        } else if svc::active_mappings(&self.options.column_mappings).is_empty() {
            "Map at least one column.".to_string()
        } else {
            String::new()
        }
    }

    // -- Options ------------------------------------------------------------

    pub fn set_truncate(&mut self, on: bool) {
        self.options.truncate_before = on;
        self.invalidate_plan();
    }

    pub fn set_skip_rows_text(&mut self, text: &str) {
        self.skip_rows_text = text.to_string();
        self.invalidate_plan();
    }

    pub fn set_batch_size_text(&mut self, text: &str) {
        self.batch_size_text = text.to_string();
        self.invalidate_plan();
    }

    /// Read the two typed numbers into the options. Blank means the default.
    pub fn commit_options(&mut self) -> Result<(), String> {
        let skip = self.skip_rows_text.trim();
        self.options.skip_rows = if skip.is_empty() {
            0
        } else {
            skip.parse::<usize>()
                .map_err(|_| format!("Skip rows must be a whole number, got \"{skip}\""))?
        };
        let batch = self.batch_size_text.trim();
        self.options.batch_size = if batch.is_empty() {
            DEFAULT_BATCH_SIZE
        } else {
            match batch.parse::<usize>() {
                Ok(n) if (1..=MAX_BATCH_SIZE).contains(&n) => n,
                _ => {
                    return Err(format!(
                        "Rows per statement must be between 1 and {MAX_BATCH_SIZE}, got \"{batch}\""
                    ))
                }
            }
        };
        self.options.schema = (!self.schema.trim().is_empty()).then(|| self.schema.trim().to_string());
        // A name that is not a table yet is created from the file's columns.
        self.options.create_table = self.is_new_table();
        Ok(())
    }

    // -- Plan, confirm, run -------------------------------------------------

    /// Anything changed after a plan was built invalidates it, so the
    /// confirmation can never describe a stale import.
    pub fn invalidate_plan(&mut self) {
        self.plan = None;
        self.confirm_input.clear();
        if self.step == Step::Confirm {
            self.step = Step::Options;
        }
    }

    pub fn apply_plan(&mut self, source: ImportSource, plan: ImportPlan) {
        self.source = Some(Arc::new(source));
        self.plan = Some(plan);
        self.confirm_input.clear();
        self.error.clear();
        self.step = Step::Confirm;
    }

    pub fn plan_failed(&mut self, message: String) {
        self.plan = None;
        self.error = message;
        self.step = Step::Options;
    }

    pub fn needs_typed_confirmation(&self) -> bool {
        self.plan.as_ref().is_some_and(|p| p.is_destructive())
    }

    pub fn confirmation_ok(&self) -> bool {
        !self.needs_typed_confirmation() || self.confirm_input.trim() == self.options.table_name
    }

    /// Whether "Write rows" may be pressed.
    pub fn can_import(&self) -> bool {
        self.plan.as_ref().is_some_and(|p| p.row_count > 0) && self.confirmation_ok()
    }

    pub fn import_button_label(&self) -> String {
        match &self.plan {
            Some(p) => format!("Write {} rows", p.row_count),
            None => "Write rows".to_string(),
        }
    }

    pub fn import_started(&mut self) {
        self.result = None;
        self.error.clear();
        self.step = Step::Running;
    }

    pub fn import_finished(&mut self, result: ImportResult) {
        self.result = Some(result);
        self.step = Step::Complete;
    }

    pub fn import_failed(&mut self, message: String) {
        self.error = message;
        self.step = Step::Confirm;
    }

    /// "Import another file": keep the connection and schema, drop the rest.
    pub fn restart(&mut self) {
        let tables = std::mem::take(&mut self.tables);
        *self = Self::new(&self.schema.clone());
        self.tables = tables;
    }

    // -- Navigation ---------------------------------------------------------

    /// Whether the step bar may jump to `step`: backwards to anything already
    /// reached, never over a step that has not produced its input.
    pub fn can_enter(&self, step: Step) -> bool {
        if matches!(self.step, Step::Running | Step::Complete) {
            return false;
        }
        match step {
            Step::File => true,
            Step::Preview | Step::Mapping => self.preview.is_some(),
            Step::Options => self.preview.is_some() && self.mapping_ready(),
            Step::Confirm => self.plan.is_some(),
            Step::Running | Step::Complete => false,
        }
    }

    pub fn go_to(&mut self, step: Step) -> bool {
        if step.index() <= self.step.index() && self.can_enter(step) {
            self.step = step;
            true
        } else {
            false
        }
    }

    pub fn next(&mut self) -> bool {
        let target = match self.step {
            Step::File => Step::Preview,
            Step::Preview => Step::Mapping,
            Step::Mapping => Step::Options,
            _ => return false,
        };
        if self.can_enter(target) {
            self.step = target;
            true
        } else {
            false
        }
    }

    pub fn back(&mut self) {
        self.step = match self.step {
            Step::Preview => Step::File,
            Step::Mapping => Step::Preview,
            Step::Options => Step::Mapping,
            Step::Confirm => Step::Options,
            other => other,
        };
    }
}

/// First entry of a mapping row's target dropdown.
pub const SKIP_LABEL: &str = "Not imported";

/// Why `path` cannot be imported, or `None` when it looks fine. The file
/// itself is only opened by the preview job.
pub fn file_problem(path: &str) -> Option<String> {
    let path = path.trim();
    if path.is_empty() {
        return Some("Enter the full path of a CSV or Excel file.".to_string());
    }
    let kind = svc::file_kind(path);
    if !SUPPORTED_EXTENSIONS.contains(&kind.as_str()) {
        return Some(if kind.is_empty() {
            "The file needs an extension: CSV, TSV, TXT, XLSX or XLS.".to_string()
        } else {
            format!("Unsupported file type \".{kind}\". Use CSV, TSV, TXT, XLSX or XLS.")
        });
    }
    None
}

/// What the result screen lists: the first few failures, each cut to a sane
/// length, and a trailing note for the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorList {
    pub lines: Vec<String>,
    /// "...and 94 more", or empty.
    pub more: String,
}

pub fn error_list(result: &ImportResult) -> ErrorList {
    let lines: Vec<String> = result
        .errors
        .iter()
        .take(ERROR_LIST_SHOWN)
        .map(|e| crate::import_export::truncate_middle(e, ERROR_LINE_MAX))
        .collect();
    // The engine keeps only the first few messages, so "more" is counted
    // against the failed-row total, not against what it kept.
    let hidden = result.rows_failed.saturating_sub(lines.len());
    ErrorList {
        more: if hidden > 0 {
            format!("...and {hidden} more failed rows")
        } else {
            String::new()
        },
        lines,
    }
}

/// How the result screen's banner should read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultTone {
    Success,
    Warning,
    Failure,
}

pub fn result_tone(result: &ImportResult) -> ResultTone {
    if result.success {
        ResultTone::Success
    } else if result.rows_imported > 0 {
        ResultTone::Warning
    } else {
        ResultTone::Failure
    }
}

/// A mapping row's type as the position in `DATA_TYPES`, or -1 for a type
/// the list does not know (a target column's own, like `varchar(40)`).
pub fn type_index(mapping: &ColumnMapping) -> i32 {
    let t = mapping.target_type.as_deref().unwrap_or("TEXT");
    svc::DATA_TYPES
        .iter()
        .position(|d| d.eq_ignore_ascii_case(t))
        .map_or(-1, |i| i as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview(cols: &[&str]) -> ImportPreview {
        ImportPreview {
            columns: cols.iter().map(|c| c.to_string()).collect(),
            rows: vec![cols.iter().map(|_| "1".to_string()).collect()],
            total_rows: None,
            detected_types: cols.iter().map(|_| "INTEGER".to_string()).collect(),
            file_type: "csv".into(),
            sheet_names: None,
        }
    }

    fn target(name: &str, ty: &str) -> TableColumnInfo {
        TableColumnInfo {
            name: name.into(),
            data_type: ty.into(),
            nullable: true,
            default_value: None,
            is_primary_key: false,
        }
    }

    fn ready_wizard() -> ImportWizard {
        let mut w = ImportWizard::new("public");
        w.set_file_path("/tmp/a.csv");
        w.apply_target_columns(vec![target("id", "integer"), target("name", "text")]);
        w.apply_preview(preview(&["id", "name", "extra"]));
        w.set_tables(vec!["widgets".into()]);
        w.set_target_table("widgets");
        w.apply_target_columns(vec![target("id", "integer"), target("name", "text")]);
        w
    }

    #[test]
    fn a_path_is_checked_for_a_readable_extension() {
        assert!(file_problem("").unwrap().contains("full path"));
        assert!(file_problem("   ").is_some());
        assert!(file_problem("/x/data").unwrap().contains("extension"));
        assert!(file_problem("/x/data.pdf").unwrap().contains(".pdf"));
        for ok in ["/x/a.csv", "/x/a.TSV", "/x/a.txt", "/x/a.xlsx", "/x/a.xls"] {
            assert_eq!(file_problem(ok), None, "{ok}");
        }
    }

    #[test]
    fn a_new_file_clears_everything_derived_from_the_old_one() {
        let mut w = ready_wizard();
        w.set_file_path("/tmp/b.csv");
        assert!(w.preview.is_none());
        assert!(w.options.column_mappings.is_empty());
        assert_eq!(w.step, Step::File);
    }

    #[test]
    fn a_tsv_selects_the_tab_delimiter_until_the_user_chooses_one() {
        let mut w = ImportWizard::new("");
        w.set_file_path("/x/a.tsv");
        assert_eq!(w.options.delimiter, Some('\t'));
        w.set_file_path("/x/a.csv");
        assert_eq!(w.options.delimiter, Some(','));
        w.set_delimiter_index(1);
        assert_eq!(w.options.delimiter, Some(';'));
        w.set_file_path("/x/a.tsv");
        assert_eq!(w.options.delimiter, Some(';'), "a deliberate choice is kept");
    }

    #[test]
    fn the_preview_suggests_a_mapping_by_column_name() {
        let w = ready_wizard();
        let m = &w.options.column_mappings;
        assert_eq!(m.len(), 3);
        assert_eq!(m[0].target_column, "id");
        assert!(!m[0].skip);
        assert_eq!(m[1].target_column, "name");
        assert_eq!(m[2].target_column, "");
        assert!(m[2].skip, "an unmatched column is skipped until the user picks a target");
        assert_eq!(w.mapping_summary(), "2 of 3 columns will be imported");
        assert!(w.mapping_ready());
        assert_eq!(w.step, Step::Preview);
    }

    #[test]
    fn choosing_a_table_clears_suggestions_that_belonged_to_the_old_one() {
        let mut w = ready_wizard();
        w.set_target_table("gadgets");
        assert!(w.options.column_mappings.iter().all(|m| m.target_column.is_empty() && m.skip));
        assert!(!w.mapping_ready());
        // Its columns arrive: the untouched mapping is suggested again.
        w.apply_target_columns(vec![target("name", "text"), target("extra", "text")]);
        assert_eq!(w.options.column_mappings[1].target_column, "name");
        assert_eq!(w.options.column_mappings[2].target_column, "extra");
        assert!(w.mapping_ready());
    }

    #[test]
    fn arriving_columns_do_not_overwrite_manual_edits() {
        let mut w = ready_wizard();
        w.set_mapping_target(2, 2); // map "extra" onto "name"
        w.apply_target_columns(vec![target("id", "integer"), target("name", "text")]);
        assert_eq!(w.options.column_mappings[2].target_column, "name");
    }

    #[test]
    fn a_mapping_row_can_be_retargeted_typed_defaulted_and_skipped() {
        let mut w = ready_wizard();
        // Choice 0 is "Not imported".
        w.set_mapping_target(0, 0);
        assert!(w.options.column_mappings[0].skip);
        assert_eq!(w.options.column_mappings[0].target_column, "");
        w.set_mapping_target(0, 1);
        assert_eq!(w.options.column_mappings[0].target_column, "id");
        assert_eq!(w.options.column_mappings[0].target_type.as_deref(), Some("integer"));
        assert!(!w.options.column_mappings[0].skip);

        w.set_mapping_type(1, 0);
        assert_eq!(w.options.column_mappings[1].target_type.as_deref(), Some(svc::DATA_TYPES[0]));
        w.set_mapping_default(1, "n/a");
        assert_eq!(w.options.column_mappings[1].default_value.as_deref(), Some("n/a"));
        w.set_mapping_default(1, "");
        assert_eq!(w.options.column_mappings[1].default_value, None);

        w.toggle_skip(1);
        assert!(w.options.column_mappings[1].skip);
        assert_eq!(w.mapping_summary(), "1 of 3 columns will be imported");

        // Out-of-range indexes are ignored, not panics.
        w.set_mapping_target(99, 1);
        w.set_mapping_target(0, 99);
        w.set_mapping_type(0, 999);
        w.toggle_skip(99);
    }

    #[test]
    fn a_typed_new_table_maps_every_column_to_itself_and_is_created() {
        let mut w = ImportWizard::new("public");
        w.apply_preview(preview(&["id", "name"]));
        w.set_tables(vec!["widgets".into()]);
        assert!(!w.set_typed_table("fresh"));
        assert!(w.is_new_table());
        assert!(w.mapping_ready());
        let m = &w.options.column_mappings;
        assert_eq!((m[0].target_column.as_str(), m[0].skip), ("id", false));
        assert_eq!(m[0].target_type.as_deref(), Some("INTEGER"));
        w.commit_options().unwrap();
        assert!(w.options.create_table);

        // Typing an existing name is the same as picking it.
        assert!(w.set_typed_table(" widgets "));
        assert!(!w.is_new_table());
        w.commit_options().unwrap();
        assert!(!w.options.create_table);
    }

    #[test]
    fn the_mapping_hint_says_what_is_missing() {
        let mut w = ImportWizard::new("public");
        w.apply_preview(preview(&["a"]));
        assert_eq!(w.mapping_hint(), "Choose a target table.");
        w.set_target_table("t");
        assert_eq!(w.mapping_hint(), "Map at least one column.");
        w.apply_target_columns(vec![target("a", "text")]);
        assert_eq!(w.mapping_hint(), "");
    }

    #[test]
    fn typed_numbers_are_validated_only_when_committed() {
        let mut w = ImportWizard::new("public");
        w.commit_options().unwrap();
        assert_eq!(w.options.skip_rows, 0);
        assert_eq!(w.options.batch_size, DEFAULT_BATCH_SIZE);

        w.set_skip_rows_text("3");
        w.set_batch_size_text("50");
        w.commit_options().unwrap();
        assert_eq!((w.options.skip_rows, w.options.batch_size), (3, 50));

        w.set_skip_rows_text("-1");
        assert!(w.commit_options().unwrap_err().contains("Skip rows"));
        w.set_skip_rows_text("");
        w.set_batch_size_text("0");
        assert!(w.commit_options().unwrap_err().contains("between 1 and"));
        w.set_batch_size_text("10001");
        assert!(w.commit_options().is_err());
        w.set_batch_size_text("abc");
        assert!(w.commit_options().is_err());
    }

    #[test]
    fn an_empty_schema_is_not_sent_as_the_empty_string() {
        let mut w = ImportWizard::new("");
        w.commit_options().unwrap();
        assert_eq!(w.options.schema, None);
        let mut w = ImportWizard::new("shop");
        w.commit_options().unwrap();
        assert_eq!(w.options.schema.as_deref(), Some("shop"));
    }

    #[test]
    fn changing_any_option_after_review_sends_the_user_back_to_options() {
        let mut w = ready_wizard();
        w.apply_plan(ImportSource::default(), plan(10, false));
        assert_eq!(w.step, Step::Confirm);
        w.set_batch_size_text("5");
        assert!(w.plan.is_none());
        assert_eq!(w.step, Step::Options, "a stale plan must never be confirmable");
    }

    fn plan(rows: usize, destructive: bool) -> ImportPlan {
        ImportPlan {
            target: "\"widgets\"".into(),
            target_columns: vec!["id".into()],
            row_count: rows,
            batch_count: 1,
            truncate_statement: destructive.then(|| "DELETE FROM \"widgets\";".into()),
            create_statement: None,
            sample_statements: vec![],
            skipped_columns: vec![],
        }
    }

    #[test]
    fn a_truncating_plan_needs_the_table_name_typed() {
        let mut w = ready_wizard();
        w.apply_plan(ImportSource::default(), plan(5, true));
        assert!(w.needs_typed_confirmation());
        assert!(!w.can_import());
        w.confirm_input = "widget".into();
        assert!(!w.can_import());
        w.confirm_input = " widgets ".into();
        assert!(w.can_import());
    }

    #[test]
    fn an_append_plan_needs_no_typing_but_an_empty_one_cannot_run() {
        let mut w = ready_wizard();
        w.apply_plan(ImportSource::default(), plan(5, false));
        assert!(w.can_import());
        assert_eq!(w.import_button_label(), "Write 5 rows");
        w.apply_plan(ImportSource::default(), plan(0, false));
        assert!(!w.can_import());
    }

    #[test]
    fn steps_cannot_be_skipped_forward() {
        let mut w = ImportWizard::new("public");
        assert!(!w.next(), "no preview yet");
        assert!(!w.can_enter(Step::Mapping));
        w.apply_preview(preview(&["id"]));
        assert!(w.can_enter(Step::Mapping));
        assert!(!w.can_enter(Step::Options), "no target table or mapping yet");
        assert!(!w.can_enter(Step::Confirm));
        assert!(w.next()); // Preview -> Mapping
        assert_eq!(w.step, Step::Mapping);
        assert!(!w.next(), "mapping is incomplete");
        w.set_target_table("t");
        w.apply_target_columns(vec![target("id", "integer")]);
        assert!(w.next());
        assert_eq!(w.step, Step::Options);
        w.back();
        assert_eq!(w.step, Step::Mapping);
        assert!(w.go_to(Step::File));
        assert!(!w.go_to(Step::Options), "forward jumps go through the buttons");
    }

    #[test]
    fn a_failed_plan_returns_to_options_with_the_reason() {
        let mut w = ready_wizard();
        w.plan_failed("Failed to open CSV: no such file".into());
        assert_eq!(w.step, Step::Options);
        assert!(w.error.contains("no such file"));
    }

    #[test]
    fn running_and_finishing_lock_the_step_bar() {
        let mut w = ready_wizard();
        w.apply_plan(ImportSource::default(), plan(5, false));
        w.import_started();
        assert_eq!(w.step, Step::Running);
        assert!(!w.can_enter(Step::File));
        w.import_finished(ImportResult { rows_imported: 5, rows_failed: 0, errors: vec![], success: true });
        assert_eq!(w.step, Step::Complete);
        assert!(!w.can_enter(Step::Options));
        w.restart();
        assert_eq!(w.step, Step::File);
        assert_eq!(w.schema, "public");
        assert!(w.result.is_none());
        assert_eq!(w.tables, vec!["widgets"], "the table list survives a restart");
    }

    #[test]
    fn a_failed_run_returns_to_the_review_with_the_message() {
        let mut w = ready_wizard();
        w.apply_plan(ImportSource::default(), plan(5, false));
        w.import_started();
        w.import_failed("connection lost".into());
        assert_eq!(w.step, Step::Confirm);
        assert_eq!(w.error, "connection lost");
    }

    #[test]
    fn the_error_list_is_cut_and_counts_what_was_not_shown() {
        let errors: Vec<String> = (1..=10).map(|i| format!("Row {i}: bad")).collect();
        let r = ImportResult { rows_imported: 90, rows_failed: 100, errors, success: false };
        let list = error_list(&r);
        assert_eq!(list.lines.len(), ERROR_LIST_SHOWN);
        assert_eq!(list.more, "...and 94 more failed rows");

        let short = ImportResult { rows_imported: 0, rows_failed: 2, errors: vec!["a".into(), "b".into()], success: false };
        let list = error_list(&short);
        assert_eq!(list.lines, vec!["a", "b"]);
        assert_eq!(list.more, "");

        let long = ImportResult {
            rows_imported: 0,
            rows_failed: 1,
            errors: vec!["x".repeat(2000)],
            success: false,
        };
        assert!(error_list(&long).lines[0].chars().count() < 400);
    }

    #[test]
    fn the_result_banner_distinguishes_clean_partial_and_failed() {
        let ok = ImportResult { rows_imported: 3, rows_failed: 0, errors: vec![], success: true };
        assert_eq!(result_tone(&ok), ResultTone::Success);
        let partial = ImportResult { rows_imported: 3, rows_failed: 1, errors: vec![], success: false };
        assert_eq!(result_tone(&partial), ResultTone::Warning);
        let none = ImportResult { rows_imported: 0, rows_failed: 4, errors: vec![], success: false };
        assert_eq!(result_tone(&none), ResultTone::Failure);
    }

    #[test]
    fn a_header_toggle_forces_the_file_to_be_read_again() {
        let mut w = ready_wizard();
        w.source = Some(Arc::new(ImportSource::default()));
        w.set_first_row_is_header(true);
        assert!(w.source.is_some(), "no change, no invalidation");
        w.set_first_row_is_header(false);
        assert!(w.source.is_none());
    }

    #[test]
    fn spreadsheets_expose_their_sheets() {
        let mut w = ImportWizard::new("");
        w.set_file_path("/x/book.xlsx");
        assert!(w.is_spreadsheet());
        let mut p = preview(&["a"]);
        p.sheet_names = Some(vec!["Sales".into(), "Costs".into()]);
        w.apply_preview(p);
        assert_eq!(w.options.sheet_name.as_deref(), Some("Sales"));
        assert_eq!(w.sheet_index(), 0);
        assert!(w.set_sheet_index(1));
        assert_eq!(w.options.sheet_name.as_deref(), Some("Costs"));
        assert_eq!(w.sheet_index(), 1);
        assert!(!w.set_sheet_index(5));
    }

    #[test]
    fn a_mapping_type_the_list_does_not_know_reports_minus_one() {
        let mut m = ColumnMapping {
            source_column: "a".into(),
            target_column: "a".into(),
            target_type: Some("varchar(40)".into()),
            default_value: None,
            skip: false,
        };
        assert_eq!(type_index(&m), -1);
        m.target_type = Some(svc::DATA_TYPES[2].to_lowercase());
        assert_eq!(type_index(&m), 2);
    }
}
