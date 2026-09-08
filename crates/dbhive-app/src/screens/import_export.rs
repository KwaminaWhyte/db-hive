//! Import and export: result sets and tables out, CSV/Excel and SQL dumps in.
//!
//! Parity target: `DataImportWizard.tsx`, `SqlImportDialog.tsx` and
//! `SqlExportDialog.tsx` (2.11 in
//! docs/rust-ui-migration/01-frontend-inventory.md), plus the CSV/JSON export
//! that lived in `ResultsViewer.tsx`. All four are one screen here, because
//! they are one job with different endpoints and keeping them apart is what
//! made the React version three unrelated mental models.
//!
//! Four things shape this file:
//!
//! * **No file picker.** There is no `rfd` in this workspace, so every path
//!   is typed. Each path field has a "Suggest" button that fills in a
//!   date-stamped name under the user's Downloads (or home) directory, which
//!   is what the React `save()` dialog defaulted to, and the resolved path is
//!   always visible before anything is written.
//! * **Nothing blocks the frame.** Reading a 500 MB CSV, rendering an export
//!   and running an import are all `Job`s; file parsing goes through
//!   `spawn_blocking` so it does not occupy a driver's runtime thread.
//! * **An import is confirmed against a plan.** `build_import_plan` produces
//!   the exact target, column list, row count and the first INSERT
//!   statements verbatim. The user sees that before a single row is written,
//!   and a plan that truncates has to be confirmed by typing the table name.
//! * **Every table is `widgets::results_grid`.** The file preview and the
//!   export preview are projected into a `QueryResult` rather than getting a
//!   second hand-rolled table.
//!
//! All the logic lives in `dbhive_core::service_import_export`; this file is
//! the shell around it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dbhive_core::drivers::QueryResult;
use dbhive_core::models::{DbDriver, DbError};
use dbhive_core::service_import_export as svc;
use svc::{
    ColumnMapping, CsvQuoting, DataImportOptions, ExportFormat, ExportOptions, ImportPlan,
    ImportPreview, ImportResult, ImportSource, PreviewOptions, SqlExportOptions, SqlImportOptions,
    SqlImportResult, TableColumnInfo,
};

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::theme::{text, Palette};
use crate::widgets::results_grid;

/// Rows loaded when previewing a table before exporting it. Enough to see the
/// shape of the data without pulling the whole table twice.
const TABLE_PREVIEW_ROWS: usize = 100;

// ---------------------------------------------------------------------------
// Pure logic: paths, labels, projections
// ---------------------------------------------------------------------------

/// Delimiters the React wizard offered, as (label, char).
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

/// A default file name, date-stamped the way the React save dialog was.
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

/// Where a suggested path points. Downloads if the platform has one, else
/// the home directory, else the working directory.
fn suggest_dir() -> std::path::PathBuf {
    if let Some(dirs) = directories::UserDirs::new() {
        if let Some(dl) = dirs.download_dir() {
            return dl.to_path_buf();
        }
        return dirs.home_dir().to_path_buf();
    }
    std::path::PathBuf::from(".")
}

pub fn suggest_path(stem: &str, extension: &str) -> String {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    suggest_dir()
        .join(default_file_name(stem, extension, &today))
        .to_string_lossy()
        .to_string()
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

/// Outcome banner for a finished dump import, colour-coded like the React
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

pub fn outcome_colour(outcome: Outcome, palette: &Palette) -> egui::Color32 {
    match outcome {
        Outcome::Success => palette.success,
        Outcome::Partial => palette.warning,
        Outcome::Cancelled => palette.muted_foreground,
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
                serde_json::Value::String(
                    if c.nullable { "YES" } else { "NO" }.to_string(),
                ),
                serde_json::Value::String(
                    if c.is_primary_key { "PK" } else { "" }.to_string(),
                ),
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

// ---------------------------------------------------------------------------
// Screen state
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

/// The import wizard's linear steps, same shape as the React wizard.
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

pub struct ImportExport {
    /// Whether the screen is showing. Kept so a host can treat it as a panel.
    pub open: bool,
    pub tab: Tab,
    pub connection_id: Option<String>,
    /// Schema (Postgres) or database (MySQL) the screen defaults to.
    pub schema: String,

    // -- export ------------------------------------------------------------
    pub export_source: ExportSource,
    pub export_table: String,
    pub export: ExportOptions,
    pub export_limit_enabled: bool,
    pub export_limit: u32,
    pub export_grid: results_grid::GridState,
    pub table_preview: Option<QueryResult>,
    pub table_preview_job: Job<Result<QueryResult, DbError>>,
    pub export_job: Job<Result<usize, DbError>>,

    // -- SQL dump ----------------------------------------------------------
    pub dump: SqlExportOptions,
    pub dump_tables_input: String,
    pub dump_path: String,
    pub dump_job: Job<Result<usize, DbError>>,

    // -- SQL dump import ---------------------------------------------------
    pub sql_import_path: String,
    pub sql_import: SqlImportOptions,
    pub sql_import_cancel: Arc<AtomicBool>,
    pub sql_import_job: Job<Result<SqlImportResult, DbError>>,
    pub sql_import_result: Option<SqlImportResult>,

    // -- data import wizard ------------------------------------------------
    pub step: Step,
    pub file_path: String,
    pub options: DataImportOptions,
    pub preview: Option<ImportPreview>,
    pub preview_grid: results_grid::GridState,
    pub preview_job: Job<Result<ImportPreview, DbError>>,
    pub tables: Vec<String>,
    pub tables_job: Job<Result<Vec<String>, DbError>>,
    pub target_columns: Vec<TableColumnInfo>,
    pub target_grid: results_grid::GridState,
    pub columns_job: Job<Result<Vec<TableColumnInfo>, DbError>>,
    pub show_target_columns: bool,
    /// The parsed file, kept behind an Arc so a retry does not re-read it.
    pub source: Option<Arc<ImportSource>>,
    pub plan: Option<ImportPlan>,
    pub prepare_job: Job<Result<(ImportSource, ImportPlan), DbError>>,
    pub import_cancel: Arc<AtomicBool>,
    pub import_job: Job<Result<ImportResult, DbError>>,
    pub import_result: Option<ImportResult>,
    /// Typed confirmation for a plan that deletes existing rows.
    pub confirm_input: String,

    pub last_error: Option<String>,
    /// Table list is loaded once per (connection, schema).
    tables_requested: Option<(String, String)>,
    /// Column list is loaded once per (connection, schema, table).
    columns_requested: Option<(String, String, String)>,
}

impl Default for ImportExport {
    fn default() -> Self {
        Self {
            open: false,
            tab: Tab::Export,
            connection_id: None,
            schema: String::new(),

            export_source: ExportSource::CurrentResult,
            export_table: String::new(),
            export: ExportOptions::default(),
            export_limit_enabled: false,
            export_limit: 10_000,
            export_grid: results_grid::GridState::default(),
            table_preview: None,
            table_preview_job: Job::default(),
            export_job: Job::default(),

            dump: SqlExportOptions::default(),
            dump_tables_input: String::new(),
            dump_path: String::new(),
            dump_job: Job::default(),

            sql_import_path: String::new(),
            sql_import: SqlImportOptions::default(),
            sql_import_cancel: Arc::new(AtomicBool::new(false)),
            sql_import_job: Job::default(),
            sql_import_result: None,

            step: Step::File,
            file_path: String::new(),
            options: DataImportOptions::default(),
            preview: None,
            preview_grid: results_grid::GridState::default(),
            preview_job: Job::default(),
            tables: Vec::new(),
            tables_job: Job::default(),
            target_columns: Vec::new(),
            target_grid: results_grid::GridState::default(),
            columns_job: Job::default(),
            show_target_columns: false,
            source: None,
            plan: None,
            prepare_job: Job::default(),
            import_cancel: Arc::new(AtomicBool::new(false)),
            import_job: Job::default(),
            import_result: None,
            confirm_input: String::new(),

            last_error: None,
            tables_requested: None,
            columns_requested: None,
        }
    }
}

impl ImportExport {
    /// Point the screen at a connection. Re-opening the same one keeps the
    /// half-filled wizard, so switching away and back does not lose work.
    pub fn open_for(&mut self, connection_id: impl Into<String>, schema: impl Into<String>) {
        let connection_id = connection_id.into();
        let schema = schema.into();
        self.open = true;
        if self.connection_id.as_deref() == Some(connection_id.as_str()) && self.schema == schema {
            return;
        }
        *self = Self {
            open: true,
            connection_id: Some(connection_id),
            schema: schema.clone(),
            dump: SqlExportOptions {
                schema: Some(schema.clone()),
                ..SqlExportOptions::default()
            },
            options: DataImportOptions {
                schema: Some(schema),
                ..DataImportOptions::default()
            },
            ..Self::default()
        };
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn busy(&self) -> bool {
        self.export_job.is_running()
            || self.dump_job.is_running()
            || self.sql_import_job.is_running()
            || self.prepare_job.is_running()
            || self.import_job.is_running()
    }

    /// Anything the user changes after a plan was built invalidates it, so
    /// the confirmation can never describe a stale import.
    fn invalidate_plan(&mut self) {
        self.plan = None;
        self.confirm_input.clear();
        if self.step == Step::Confirm {
            self.step = Step::Options;
        }
    }

    fn row_limit(&self) -> Option<usize> {
        self.export_limit_enabled
            .then(|| self.export_limit.max(1) as usize)
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// Show as a centred window, matching the modal the React shell used.
pub fn show_window(app: &mut DbHiveApp, ctx: &egui::Context) {
    if !app.screens.import_export.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Import / Export")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size(egui::vec2(940.0, 660.0))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            show(app, ui);
        });
    if !open {
        app.screens.import_export.open = false;
    }
}

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    poll(app);

    // The screen has no connection of its own; it follows the workspace, so
    // opening it from the palette while connected just works.
    if app.screens.import_export.connection_id.is_none() {
        if let Some(ws) = app.workspace.as_ref() {
            let id = ws.connection_id.clone();
            let schema = crate::screens::workspace::current_schema_or_database(ws);
            app.screens.import_export.open_for(id, schema);
        }
    }

    let Some(connection_id) = app.screens.import_export.connection_id.clone() else {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(text::heading(palette, "Import / Export"));
            ui.label(text::muted(
                palette,
                "Connect to a database to import or export data.",
            ));
        });
        return;
    };

    header(app, ui, palette);
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(6.0);

    if let Some(err) = app.screens.import_export.last_error.clone() {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(palette.destructive, "Failed:");
            ui.label(err);
            if ui.small_button("Dismiss").clicked() {
                app.screens.import_export.last_error = None;
            }
        });
        ui.add_space(4.0);
    }

    egui::ScrollArea::vertical()
        .id_salt("import_export_body")
        .show(ui, |ui| match app.screens.import_export.tab {
            Tab::Export => export_tab(app, ui, palette, &connection_id),
            Tab::Dump => dump_tab(app, ui, palette, &connection_id),
            Tab::Import => import_tab(app, ui, palette, &connection_id),
            Tab::SqlImport => sql_import_tab(app, ui, palette, &connection_id),
        });
}

fn header(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let schema = app.screens.import_export.schema.clone();
    let busy = app.screens.import_export.busy();
    let s = &mut app.screens.import_export;

    ui.horizontal(|ui| {
        ui.label(text::heading(palette, "Import / Export"));
        if !schema.is_empty() {
            ui.label(text::muted(palette, schema));
        }
        if busy {
            ui.add(egui::Spinner::new().size(14.0));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                s.open = false;
            }
        });
    });

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut s.tab, Tab::Export, "Export data");
        ui.selectable_value(&mut s.tab, Tab::Dump, "SQL dump");
        ui.selectable_value(&mut s.tab, Tab::Import, "Import file");
        ui.selectable_value(&mut s.tab, Tab::SqlImport, "Run SQL file");
    });
}

// -- export ------------------------------------------------------------------

fn export_tab(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette, connection_id: &str) {
    let (result_columns, result_rows, result_label) = current_result_shape(app);
    let mut start = false;
    let mut load_table_preview = false;

    card(ui, palette, "Source", |ui| {
        let s = &mut app.screens.import_export;
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut s.export_source,
                ExportSource::CurrentResult,
                "Current query result",
            );
            ui.selectable_value(&mut s.export_source, ExportSource::Table, "A table");
        });
        ui.add_space(4.0);

        match s.export_source {
            ExportSource::CurrentResult => {
                ui.label(text::muted(palette, result_label.clone()));
            }
            ExportSource::Table => {
                ui.horizontal(|ui| {
                    ui.label("Schema");
                    ui.add(
                        egui::TextEdit::singleline(&mut s.schema)
                            .hint_text("public")
                            .desired_width(160.0),
                    );
                    ui.label("Table");
                    ui.add(
                        egui::TextEdit::singleline(&mut s.export_table)
                            .hint_text("users")
                            .desired_width(200.0),
                    );
                    if ui
                        .add_enabled(
                            !s.export_table.trim().is_empty()
                                && !s.table_preview_job.is_running(),
                            egui::Button::new("Preview"),
                        )
                        .on_hover_text("Read the first 100 rows")
                        .clicked()
                    {
                        load_table_preview = true;
                    }
                });
                ui.label(text::muted(
                    palette,
                    "The whole table is read when you export; the preview is the first 100 rows.",
                ));
            }
        }
    });

    ui.add_space(8.0);

    card(ui, palette, "Format", |ui| {
        let s = &mut app.screens.import_export;
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("ie_export_format")
                .selected_text(s.export.format.label())
                .width(220.0)
                .show_ui(ui, |ui| {
                    for f in ExportFormat::ALL {
                        ui.selectable_value(&mut s.export.format, f, f.label());
                    }
                });

            ui.checkbox(&mut s.export_limit_enabled, "Limit rows");
            ui.add_enabled(
                s.export_limit_enabled,
                egui::DragValue::new(&mut s.export_limit)
                    .range(1..=10_000_000u32)
                    .speed(50.0),
            );
        });
        ui.add_space(6.0);

        match s.export.format {
            ExportFormat::Csv => csv_options(ui, palette, s),
            ExportFormat::Json => {
                ui.checkbox(&mut s.export.json_pretty, "Pretty-print");
            }
            ExportFormat::Excel => {
                ui.horizontal(|ui| {
                    ui.label("Sheet name");
                    ui.add(
                        egui::TextEdit::singleline(&mut s.export.sheet_name)
                            .desired_width(200.0),
                    );
                });
                ui.label(text::muted(
                    palette,
                    "Written as SpreadsheetML (.xls), which Excel, Numbers and \
                     LibreOffice all open. Zipped .xlsx would need a zip writer, \
                     which this build does not have.",
                ));
            }
            ExportFormat::Sql => {
                ui.horizontal(|ui| {
                    ui.label("INSERT INTO");
                    ui.add(
                        egui::TextEdit::singleline(&mut s.export.sql_schema)
                            .hint_text("schema (optional)")
                            .desired_width(150.0),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut s.export.sql_table)
                            .hint_text("table")
                            .desired_width(180.0),
                    );
                });
                ui.label(text::muted(
                    palette,
                    "One INSERT per row, with an explicit column list. \
                     Use the SQL dump tab for CREATE TABLE and whole schemas.",
                ));
            }
        }
    });

    ui.add_space(8.0);

    card(ui, palette, "Destination", |ui| {
        let s = &mut app.screens.import_export;
        let extension = s.export.format.extension();
        let stem = match s.export_source {
            ExportSource::CurrentResult => "query_results".to_string(),
            ExportSource::Table => {
                if s.export_table.trim().is_empty() {
                    "table".to_string()
                } else {
                    s.export_table.clone()
                }
            }
        };
        path_row(ui, palette, "ie_export_path", &mut s.export.path, &stem, extension);

        let ready = !s.export.path.trim().is_empty()
            && match s.export_source {
                ExportSource::CurrentResult => !result_columns.is_empty(),
                ExportSource::Table => !s.export_table.trim().is_empty(),
            };
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    ready && !s.export_job.is_running(),
                    egui::Button::new("Export"),
                )
                .clicked()
            {
                start = true;
            }
            if s.export_job.is_running() {
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(text::muted(palette, "Writing…"));
            }
        });
    });

    ui.add_space(8.0);

    // Preview. Grid state is moved out first so the immutable borrow of the
    // result and the mutable borrow of the screen never overlap.
    let mut grid = std::mem::take(&mut app.screens.import_export.export_grid);
    let source = app.screens.import_export.export_source;
    card(ui, palette, "Preview", |ui| match source {
        ExportSource::CurrentResult => match current_result_ref(app) {
            Some(result) if !result.columns.is_empty() => {
                results_grid::show(ui, palette, result, &mut grid);
            }
            _ => {
                ui.label(text::muted(
                    palette,
                    "Run a query in the workspace to export its result.",
                ));
            }
        },
        ExportSource::Table => {
            if app.screens.import_export.table_preview_job.is_running() {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.label(text::muted(palette, "Reading rows"));
                });
            } else if let Some(result) = app.screens.import_export.table_preview.as_ref() {
                results_grid::show(ui, palette, result, &mut grid);
            } else {
                ui.label(text::muted(palette, "Choose a table and press Preview."));
            }
        }
    });
    app.screens.import_export.export_grid = grid;

    if load_table_preview {
        start_table_preview(app, ui.ctx(), connection_id);
    }
    if start {
        start_export(app, ui.ctx(), connection_id, result_columns, result_rows);
    }
}

fn csv_options(ui: &mut egui::Ui, palette: &Palette, s: &mut ImportExport) {
    ui.horizontal(|ui| {
        ui.label("Delimiter");
        egui::ComboBox::from_id_salt("ie_export_delim")
            .selected_text(delimiter_label(s.export.csv.delimiter))
            .width(150.0)
            .show_ui(ui, |ui| {
                for (label, ch) in DELIMITERS {
                    ui.selectable_value(&mut s.export.csv.delimiter, ch, label);
                }
            });

        ui.label("Quoting");
        egui::ComboBox::from_id_salt("ie_export_quoting")
            .selected_text(s.export.csv.quoting.label())
            .width(160.0)
            .show_ui(ui, |ui| {
                for q in [CsvQuoting::Minimal, CsvQuoting::All] {
                    ui.selectable_value(&mut s.export.csv.quoting, q, q.label());
                }
            });
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut s.export.csv.include_headers, "Include header row");
        ui.add_space(12.0);
        ui.checkbox(&mut s.export.csv.crlf, "CRLF line endings");
    });
    ui.horizontal(|ui| {
        ui.label("NULL as");
        ui.add(
            egui::TextEdit::singleline(&mut s.export.csv.null_text)
                .hint_text("(empty)")
                .desired_width(120.0),
        );
        ui.label(text::muted(
            palette,
            "Leave empty to write nothing, or use \\N to tell NULL from an empty string.",
        ));
    });
}

// -- SQL dump ----------------------------------------------------------------

fn dump_tab(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette, connection_id: &str) {
    let mut start = false;

    card(ui, palette, "What to dump", |ui| {
        let s = &mut app.screens.import_export;
        let mut schema = s.dump.schema.clone().unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label("Schema / database");
            if ui
                .add(
                    egui::TextEdit::singleline(&mut schema)
                        .hint_text("public")
                        .desired_width(220.0),
                )
                .changed()
            {
                s.dump.schema = Some(schema.clone());
            }
        });

        ui.horizontal(|ui| {
            ui.label("Tables");
            ui.add(
                egui::TextEdit::singleline(&mut s.dump_tables_input)
                    .hint_text("users, orders — leave empty for every table")
                    .desired_width(360.0),
            );
        });
        let parsed = svc::parse_table_filter(&s.dump_tables_input);
        ui.label(text::muted(
            palette,
            if parsed.is_empty() {
                "Every table in the schema".to_string()
            } else {
                format!("{} table(s): {}", parsed.len(), parsed.join(", "))
            },
        ));

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.checkbox(&mut s.dump.include_drop, "DROP TABLE");
            ui.add_space(12.0);
            ui.checkbox(&mut s.dump.include_create, "CREATE TABLE");
            ui.add_space(12.0);
            ui.checkbox(&mut s.dump.include_data, "INSERT data");
        });
    });

    ui.add_space(8.0);

    card(ui, palette, "Destination", |ui| {
        let s = &mut app.screens.import_export;
        path_row(
            ui,
            palette,
            "ie_dump_path",
            &mut s.dump_path,
            "database_dump",
            "sql",
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let ready = !s.dump_path.trim().is_empty()
                && (s.dump.include_drop || s.dump.include_create || s.dump.include_data);
            if ui
                .add_enabled(ready && !s.dump_job.is_running(), egui::Button::new("Export"))
                .clicked()
            {
                start = true;
            }
            if s.dump_job.is_running() {
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(text::muted(
                    palette,
                    "Reading tables and writing the dump…",
                ));
            }
        });
    });

    if start {
        let s = &mut app.screens.import_export;
        s.dump.tables = svc::parse_table_filter(&s.dump_tables_input);
        let options = s.dump.clone();
        let path = s.dump_path.clone();
        let state = app.state.clone();
        let id = connection_id.to_string();
        let handle = app.rt.handle().clone();
        app.screens
            .import_export
            .dump_job
            .start(&handle, ui.ctx(), async move {
                svc::export_to_sql(&state, &id, &path, options).await
            });
    }
}

// -- SQL dump import ---------------------------------------------------------

fn sql_import_tab(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette, connection_id: &str) {
    let mut start = false;
    let mut stop = false;

    card(ui, palette, "SQL file", |ui| {
        let s = &mut app.screens.import_export;
        let running = s.sql_import_job.is_running();

        ui.add_enabled_ui(!running, |ui| {
            ui.horizontal(|ui| {
                ui.label("Path");
                ui.add(
                    egui::TextEdit::singleline(&mut s.sql_import_path)
                        .hint_text("/path/to/dump.sql")
                        .desired_width(ui.available_width() - 20.0),
                );
            });
            ui.add_space(6.0);

            // The two are mutually exclusive: rolling everything back and
            // carrying on past an error cannot both be true.
            if ui
                .checkbox(&mut s.sql_import.use_transaction, "Run in a transaction")
                .on_hover_text("Roll everything back if any statement fails")
                .changed()
                && s.sql_import.use_transaction
            {
                s.sql_import.continue_on_error = false;
            }
            if ui
                .checkbox(&mut s.sql_import.continue_on_error, "Continue on error")
                .on_hover_text("Log failures and keep going")
                .changed()
                && s.sql_import.continue_on_error
            {
                s.sql_import.use_transaction = false;
            }
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !running && !s.sql_import_path.trim().is_empty(),
                    egui::Button::new("Run file"),
                )
                .clicked()
            {
                start = true;
            }
            if running {
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(text::muted(palette, "Executing statements…"));
                if ui
                    .button("Stop")
                    .on_hover_text("Stops after the statement that is running")
                    .clicked()
                {
                    stop = true;
                }
            }
        });

        ui.label(text::muted(
            palette,
            "Statements are streamed from the file, so a multi-gigabyte dump \
             does not have to fit in memory.",
        ));
    });

    if let Some(result) = app.screens.import_export.sql_import_result.clone() {
        ui.add_space(8.0);
        card(ui, palette, "Result", |ui| {
            let outcome = sql_import_outcome(&result);
            ui.colored_label(
                outcome_colour(outcome, palette),
                sql_import_summary(&result),
            );
            if let Some(first) = &result.first_error {
                ui.add_space(4.0);
                ui.label(text::muted(palette, "First error:"));
                ui.label(first);
            }
            if let Some(log) = &result.log_file {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(text::muted(palette, "Full error log:"));
                    // No opener crate here, so the path is selectable text
                    // the user can copy rather than a button that opens it.
                    ui.add(
                        egui::Label::new(log.clone()).sense(egui::Sense::click()),
                    )
                    .on_hover_text("Click to copy the path");
                    if ui.small_button("Copy path").clicked() {
                        ui.output_mut(|o| o.copied_text = log.clone());
                    }
                });
            }
        });
    }

    if stop {
        app.screens
            .import_export
            .sql_import_cancel
            .store(true, Ordering::Relaxed);
    }
    if start {
        let s = &mut app.screens.import_export;
        s.sql_import_result = None;
        let cancel = s.sql_import_cancel.clone();
        cancel.store(false, Ordering::Relaxed);
        let path = s.sql_import_path.clone();
        let options = s.sql_import;
        let state = app.state.clone();
        let id = connection_id.to_string();
        let handle = app.rt.handle().clone();
        app.screens
            .import_export
            .sql_import_job
            .start(&handle, ui.ctx(), async move {
                svc::import_from_sql(&state, &id, &path, options, cancel).await
            });
    }
}

// -- data import wizard ------------------------------------------------------

fn import_tab(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette, connection_id: &str) {
    ensure_tables(app, ui.ctx(), connection_id);
    ensure_target_columns(app, ui.ctx(), connection_id);

    steps_bar(app, ui, palette);
    ui.add_space(8.0);

    match app.screens.import_export.step {
        Step::File => step_file(app, ui, palette),
        Step::Preview => step_preview(app, ui, palette),
        Step::Mapping => step_mapping(app, ui, palette),
        Step::Options => step_options(app, ui, palette),
        Step::Confirm => step_confirm(app, ui, palette, connection_id),
        Step::Running => step_running(app, ui, palette),
        Step::Complete => step_complete(app, ui, palette),
    }
}

fn steps_bar(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let current = app.screens.import_export.step.index();
    ui.horizontal_wrapped(|ui| {
        for (i, step) in Step::ORDER.iter().enumerate() {
            let label = format!("{}. {}", i + 1, step.label());
            if i == current {
                ui.label(text::strong(palette, label));
            } else {
                ui.label(text::muted(palette, label));
            }
            if i + 1 < Step::ORDER.len() {
                ui.label(text::muted(palette, "›"));
            }
        }
    });
}

fn step_file(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let mut load = false;

    card(ui, palette, "Choose a file", |ui| {
        let s = &mut app.screens.import_export;
        ui.horizontal(|ui| {
            ui.label("Path");
            if ui
                .add(
                    egui::TextEdit::singleline(&mut s.file_path)
                        .hint_text("/path/to/data.csv")
                        .desired_width(ui.available_width() - 20.0),
                )
                .changed()
            {
                s.preview = None;
                s.source = None;
                s.invalidate_plan();
            }
        });
        ui.label(text::muted(
            palette,
            "CSV, TSV, TXT, XLSX and XLS. Type the full path: this build has \
             no native file picker.",
        ));

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.checkbox(&mut s.options.first_row_is_header, "First row is a header");
            ui.add_space(12.0);
            ui.label("Delimiter");
            let mut delim = s.options.delimiter.unwrap_or(',');
            egui::ComboBox::from_id_salt("ie_import_delim")
                .selected_text(delimiter_label(delim))
                .width(150.0)
                .show_ui(ui, |ui| {
                    for (label, ch) in DELIMITERS {
                        ui.selectable_value(&mut delim, ch, label);
                    }
                });
            s.options.delimiter = Some(delim);
        });

        if let Some(names) = s.preview.as_ref().and_then(|p| p.sheet_names.clone()) {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Sheet");
                let current = s
                    .options
                    .sheet_name
                    .clone()
                    .unwrap_or_else(|| names.first().cloned().unwrap_or_default());
                egui::ComboBox::from_id_salt("ie_sheet")
                    .selected_text(current.clone())
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for name in &names {
                            if ui.selectable_label(current == *name, name).clicked() {
                                s.options.sheet_name = Some(name.clone());
                                load = true;
                            }
                        }
                    });
            });
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !s.file_path.trim().is_empty() && !s.preview_job.is_running(),
                    egui::Button::new("Read preview"),
                )
                .clicked()
            {
                load = true;
            }
            if s.preview_job.is_running() {
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(text::muted(palette, "Reading the file…"));
            }
        });
    });

    if load {
        start_preview(app, ui.ctx());
    }
}

fn step_preview(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let mut back = false;
    let mut next = false;
    let mut reload = false;

    let summary = app
        .screens
        .import_export
        .preview
        .as_ref()
        .map(preview_summary)
        .unwrap_or_default();

    // Taken out so the borrow of the preview and of the grid state do not
    // overlap; put back below.
    let mut grid = std::mem::take(&mut app.screens.import_export.preview_grid);
    let projected = app
        .screens
        .import_export
        .preview
        .as_ref()
        .map(svc::preview_to_result);

    card(ui, palette, "Preview", |ui| {
        ui.label(text::muted(palette, summary));
        ui.add_space(4.0);
        match projected.as_ref() {
            Some(result) if !result.columns.is_empty() => {
                results_grid::show(ui, palette, result, &mut grid);
            }
            _ => {
                ui.label(text::muted(palette, "Nothing to preview."));
            }
        }
        ui.add_space(6.0);
        ui.label(text::muted(
            palette,
            "Column headers show the type guessed from these rows. You can \
             change any of them in the mapping step.",
        ));
    });
    app.screens.import_export.preview_grid = grid;

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button("Back").clicked() {
            back = true;
        }
        if ui.button("Re-read file").clicked() {
            reload = true;
        }
        if ui
            .add_enabled(
                app.screens.import_export.preview.is_some(),
                egui::Button::new("Continue to mapping"),
            )
            .clicked()
        {
            next = true;
        }
    });

    if back {
        app.screens.import_export.step = Step::File;
    }
    if reload {
        start_preview(app, ui.ctx());
    }
    if next {
        app.screens.import_export.step = Step::Mapping;
    }
}

fn step_mapping(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let mut back = false;
    let mut next = false;
    let mut auto_map = false;
    let mut changed = false;
    let tables = app.screens.import_export.tables.clone();
    let targets = app.screens.import_export.target_columns.clone();
    let target_names: Vec<String> = targets.iter().map(|c| c.name.clone()).collect();

    card(ui, palette, "Target table", |ui| {
        let s = &mut app.screens.import_export;
        ui.horizontal(|ui| {
            ui.label("Schema");
            if ui
                .add(
                    egui::TextEdit::singleline(&mut s.schema)
                        .hint_text("public")
                        .desired_width(150.0),
                )
                .changed()
            {
                s.options.schema = Some(s.schema.clone());
                changed = true;
            }

            ui.label("Table");
            egui::ComboBox::from_id_salt("ie_target_table")
                .selected_text(if s.options.table_name.is_empty() {
                    "Choose a table".to_string()
                } else {
                    s.options.table_name.clone()
                })
                .width(240.0)
                .show_ui(ui, |ui| {
                    for t in &tables {
                        if ui
                            .selectable_label(s.options.table_name == *t, t)
                            .clicked()
                        {
                            s.options.table_name = t.clone();
                            changed = true;
                        }
                    }
                });
            if s.tables_job.is_running() || s.columns_job.is_running() {
                ui.add(egui::Spinner::new().size(14.0));
            }
        });

        if tables.is_empty() && !s.tables_job.is_running() {
            ui.label(text::muted(
                palette,
                "No tables found in this schema. Check the schema name above.",
            ));
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!targets.is_empty(), egui::Button::new("Auto-map columns"))
                .on_hover_text("Match source and target columns by name")
                .clicked()
            {
                auto_map = true;
            }
            ui.checkbox(&mut s.show_target_columns, "Show the table's columns");
            ui.label(text::muted(palette, mapping_summary(&s.options.column_mappings)));
        });
    });

    if app.screens.import_export.show_target_columns && !targets.is_empty() {
        ui.add_space(8.0);
        let mut grid = std::mem::take(&mut app.screens.import_export.target_grid);
        let result = target_columns_result(&targets);
        card(ui, palette, "Table columns", |ui| {
            results_grid::show(ui, palette, &result, &mut grid);
        });
        app.screens.import_export.target_grid = grid;
    }

    ui.add_space(8.0);

    card(ui, palette, "Column mapping", |ui| {
        let s = &mut app.screens.import_export;
        if s.options.column_mappings.is_empty() {
            ui.label(text::muted(
                palette,
                "Preview a file to get its columns.",
            ));
            return;
        }

        egui::Grid::new("ie_mapping_grid")
            .num_columns(5)
            .spacing(egui::vec2(10.0, 6.0))
            .striped(true)
            .show(ui, |ui| {
                ui.label(text::strong(palette, "File column"));
                ui.label(text::strong(palette, "Table column"));
                ui.label(text::strong(palette, "Type"));
                ui.label(text::strong(palette, "Default if empty"));
                ui.label(text::strong(palette, "Skip"));
                ui.end_row();

                for (i, m) in s.options.column_mappings.iter_mut().enumerate() {
                    ui.label(m.source_column.clone());

                    egui::ComboBox::from_id_salt(("ie_map_target", i))
                        .selected_text(if m.target_column.is_empty() {
                            "—".to_string()
                        } else {
                            m.target_column.clone()
                        })
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(m.target_column.is_empty(), "—")
                                .clicked()
                            {
                                m.target_column.clear();
                                changed = true;
                            }
                            for name in &target_names {
                                if ui
                                    .selectable_label(m.target_column == *name, name)
                                    .clicked()
                                {
                                    m.target_column = name.clone();
                                    m.skip = false;
                                    changed = true;
                                }
                            }
                        });

                    let mut ty = m.target_type.clone().unwrap_or_else(|| "TEXT".to_string());
                    egui::ComboBox::from_id_salt(("ie_map_type", i))
                        .selected_text(ty.clone())
                        .width(150.0)
                        .show_ui(ui, |ui| {
                            for t in svc::DATA_TYPES {
                                if ui.selectable_label(ty == t, t).clicked() {
                                    ty = t.to_string();
                                    changed = true;
                                }
                            }
                        });
                    m.target_type = Some(ty);

                    let mut default = m.default_value.clone().unwrap_or_default();
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut default)
                                .hint_text("NULL")
                                .desired_width(140.0),
                        )
                        .changed()
                    {
                        m.default_value = (!default.is_empty()).then_some(default);
                        changed = true;
                    }

                    if ui.checkbox(&mut m.skip, "").changed() {
                        changed = true;
                    }
                    ui.end_row();
                }
            });
    });

    ui.add_space(8.0);
    let can_continue = !svc::active_mappings(&app.screens.import_export.options.column_mappings)
        .is_empty()
        && !app.screens.import_export.options.table_name.trim().is_empty();
    ui.horizontal(|ui| {
        if ui.button("Back").clicked() {
            back = true;
        }
        if ui
            .add_enabled(can_continue, egui::Button::new("Continue to options"))
            .clicked()
        {
            next = true;
        }
        if !can_continue {
            ui.label(text::muted(
                palette,
                "Choose a target table and map at least one column.",
            ));
        }
    });

    if auto_map {
        let s = &mut app.screens.import_export;
        if let Some(p) = s.preview.as_ref() {
            s.options.column_mappings =
                svc::auto_map_columns(&p.columns, &p.detected_types, &targets);
        }
        changed = true;
    }
    if changed {
        app.screens.import_export.invalidate_plan();
    }
    if back {
        app.screens.import_export.step = Step::Preview;
    }
    if next {
        app.screens.import_export.step = Step::Options;
    }
}

fn step_options(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let mut back = false;
    let mut next = false;
    let mut changed = false;
    // Header handling decides how the file is parsed, not just how it is
    // written, so changing it has to throw away the rows already read.
    let mut reparse = false;

    card(ui, palette, "Import settings", |ui| {
        let s = &mut app.screens.import_export;

        if ui
            .checkbox(&mut s.options.first_row_is_header, "First row is a header")
            .on_hover_text("Uncheck for a file whose first line is already data")
            .changed()
        {
            changed = true;
            reparse = true;
        }

        changed |= ui
            .checkbox(
                &mut s.options.truncate_before,
                "Empty the table before importing",
            )
            .on_hover_text("Deletes every existing row first")
            .changed();
        if s.options.truncate_before {
            ui.label(egui::RichText::new(
                "This deletes all existing rows in the target table.",
            )
            .color(palette.destructive));
        }

        changed |= ui
            .checkbox(
                &mut s.options.create_table,
                "Create the table if it does not exist",
            )
            .changed();

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Rows per statement");
            let mut batch = s.options.batch_size.max(1) as u32;
            if ui
                .add(
                    egui::DragValue::new(&mut batch)
                        .range(1..=10_000u32)
                        .speed(10.0),
                )
                .changed()
            {
                s.options.batch_size = batch as usize;
                changed = true;
            }
            ui.label(text::muted(
                palette,
                "A failed batch is retried row by row, so one bad row costs \
                 only itself.",
            ));
        });

        ui.horizontal(|ui| {
            ui.label("Skip rows");
            let mut skip = s.options.skip_rows as u32;
            if ui
                .add(egui::DragValue::new(&mut skip).range(0..=1_000_000u32))
                .changed()
            {
                s.options.skip_rows = skip as usize;
                changed = true;
            }
            ui.label(text::muted(
                palette,
                "Data rows discarded from the top, after any header row.",
            ));
        });
    });

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button("Back").clicked() {
            back = true;
        }
        if ui
            .add_enabled(
                !app.screens.import_export.prepare_job.is_running(),
                egui::Button::new("Review what will be written"),
            )
            .clicked()
        {
            next = true;
        }
        if app.screens.import_export.prepare_job.is_running() {
            ui.add(egui::Spinner::new().size(14.0));
            ui.label(text::muted(palette, "Reading the whole file…"));
        }
    });

    if changed {
        app.screens.import_export.invalidate_plan();
    }
    if back {
        app.screens.import_export.step = Step::Mapping;
    }
    if next {
        start_prepare(app, ui.ctx());
    }
}

fn step_confirm(
    app: &mut DbHiveApp,
    ui: &mut egui::Ui,
    palette: &Palette,
    connection_id: &str,
) {
    let mut back = false;
    let mut go = false;

    let Some(plan) = app.screens.import_export.plan.clone() else {
        ui.label(text::muted(palette, "Nothing prepared yet."));
        if ui.button("Back").clicked() {
            app.screens.import_export.step = Step::Options;
        }
        return;
    };

    card(ui, palette, "This is what will be written", |ui| {
        egui::Grid::new("ie_plan_grid")
            .num_columns(2)
            .spacing(egui::vec2(12.0, 4.0))
            .show(ui, |ui| {
                ui.label(text::muted(palette, "Target"));
                ui.label(text::strong(palette, plan.target.clone()));
                ui.end_row();

                ui.label(text::muted(palette, "Columns"));
                ui.label(plan.target_columns.join(", "));
                ui.end_row();

                if !plan.skipped_columns.is_empty() {
                    ui.label(text::muted(palette, "Not imported"));
                    ui.label(text::muted(palette, plan.skipped_columns.join(", ")));
                    ui.end_row();
                }

                ui.label(text::muted(palette, "Rows"));
                ui.label(text::strong(
                    palette,
                    format!("{} in {} statement(s)", plan.row_count, plan.batch_count),
                ));
                ui.end_row();

                ui.label(text::muted(palette, "Source"));
                ui.label(app.screens.import_export.file_path.clone());
                ui.end_row();
            });

        if let Some(create) = &plan.create_statement {
            ui.add_space(6.0);
            ui.label(text::muted(palette, "Runs first:"));
            ui.code(create);
        }
        if let Some(truncate) = &plan.truncate_statement {
            ui.add_space(6.0);
            ui.colored_label(
                palette.destructive,
                "Every existing row in this table is deleted first:",
            );
            ui.code(truncate);
        }

        if !plan.sample_statements.is_empty() {
            ui.add_space(6.0);
            ui.label(text::muted(
                palette,
                format!(
                    "First {} of {} statement(s), verbatim:",
                    plan.sample_statements.len(),
                    plan.batch_count
                ),
            ));
            for stmt in &plan.sample_statements {
                ui.code(truncate_middle(stmt, 600));
            }
        }
    });

    ui.add_space(8.0);

    let needs_typed = plan.is_destructive();
    let table_name = app.screens.import_export.options.table_name.clone();

    card(ui, palette, "Confirm", |ui| {
        let s = &mut app.screens.import_export;
        if needs_typed {
            ui.label(egui::RichText::new(format!(
                "Type {} to confirm deleting its existing rows.",
                table_name
            ))
            .color(palette.warning));
            ui.add(
                egui::TextEdit::singleline(&mut s.confirm_input)
                    .hint_text(table_name.clone())
                    .desired_width(240.0),
            );
            ui.add_space(4.0);
        }

        let typed_ok = !needs_typed || s.confirm_input.trim() == table_name;
        ui.horizontal(|ui| {
            if ui.button("Back").clicked() {
                back = true;
            }
            let label = format!("Write {} rows to {}", plan.row_count, plan.target);
            if ui
                .add_enabled(
                    typed_ok && plan.row_count > 0 && !s.import_job.is_running(),
                    egui::Button::new(label),
                )
                .clicked()
            {
                go = true;
            }
            if plan.row_count == 0 {
                ui.label(text::muted(palette, "The file has no rows to import."));
            }
        });
    });

    if back {
        app.screens.import_export.step = Step::Options;
    }
    if go {
        start_import(app, ui.ctx(), connection_id);
    }
}

fn step_running(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let s = &app.screens.import_export;
    let rows = s.plan.as_ref().map(|p| p.row_count).unwrap_or(0);
    let target = s.plan.as_ref().map(|p| p.target.clone()).unwrap_or_default();
    let mut stop = false;

    card(ui, palette, "Importing", |ui| {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new());
            ui.label(text::strong(
                palette,
                format!("Writing {} rows to {}", rows, target),
            ));
        });
        ui.add_space(6.0);
        ui.label(text::muted(
            palette,
            "Rows already written stay written; stopping leaves a partial \
             import rather than rolling back.",
        ));
        ui.add_space(6.0);
        if ui.button("Stop").clicked() {
            stop = true;
        }
    });

    if stop {
        app.screens
            .import_export
            .import_cancel
            .store(true, Ordering::Relaxed);
    }
}

fn step_complete(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let Some(result) = app.screens.import_export.import_result.clone() else {
        app.screens.import_export.step = Step::Options;
        return;
    };
    let mut again = false;
    let mut done = false;

    card(ui, palette, "Import finished", |ui| {
        let colour = if result.success {
            palette.success
        } else if result.rows_imported > 0 {
            palette.warning
        } else {
            palette.destructive
        };
        ui.colored_label(colour, import_summary(&result));

        if !result.errors.is_empty() {
            ui.add_space(6.0);
            ui.label(text::muted(
                palette,
                format!("First {} failure(s):", result.errors.len()),
            ));
            for err in &result.errors {
                ui.label(truncate_middle(err, 300));
            }
            if result.rows_failed > result.errors.len() {
                ui.label(text::muted(
                    palette,
                    format!(
                        "…and {} more",
                        result.rows_failed - result.errors.len()
                    ),
                ));
            }
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Import another file").clicked() {
                again = true;
            }
            if ui.button("Back to options").clicked() {
                done = true;
            }
        });
    });

    if again {
        let s = &mut app.screens.import_export;
        s.step = Step::File;
        s.preview = None;
        s.source = None;
        s.plan = None;
        s.import_result = None;
        s.confirm_input.clear();
        s.file_path.clear();
    }
    if done {
        app.screens.import_export.step = Step::Options;
    }
}

// -- shared widgets ----------------------------------------------------------

fn path_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    id: &str,
    path: &mut String,
    stem: &str,
    extension: &str,
) {
    ui.horizontal(|ui| {
        ui.label("Save to");
        ui.add(
            egui::TextEdit::singleline(path)
                .id_salt(id)
                .hint_text("/path/to/file")
                .desired_width(ui.available_width() - 110.0),
        );
        if ui
            .button("Suggest")
            .on_hover_text("Fill in a date-stamped name in your downloads folder")
            .clicked()
        {
            *path = suggest_path(stem, extension);
        }
    });
    if !path.trim().is_empty() && !path.ends_with(extension) {
        ui.label(text::muted(
            palette,
            format!("Tip: the usual extension for this format is .{}", extension),
        ));
    }
}

fn card<R>(
    ui: &mut egui::Ui,
    palette: &Palette,
    title: &str,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::none()
        .fill(palette.card)
        .stroke(egui::Stroke::new(1.0_f32, palette.border))
        .rounding(egui::Rounding::same(8.0))
        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width() - 4.0);
            ui.label(text::strong(palette, title));
            ui.add_space(6.0);
            body(ui)
        })
        .inner
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
// Async work
// ---------------------------------------------------------------------------

/// The active workspace tab's result, borrowed for drawing only.
fn current_result_ref(app: &DbHiveApp) -> Option<&QueryResult> {
    let ws = app.workspace.as_ref()?;
    ws.tabs.get(ws.active_tab)?.result.as_ref()
}

/// Columns, rows and a label for the current result. Cloned, because starting
/// an export moves them into a task.
fn current_result_shape(app: &DbHiveApp) -> (Vec<String>, Vec<Vec<serde_json::Value>>, String) {
    match current_result_ref(app) {
        Some(r) => {
            let label = format!("{} rows, {} columns", r.rows.len(), r.columns.len());
            (r.columns.clone(), r.rows.clone(), label)
        }
        None => (
            Vec::new(),
            Vec::new(),
            "No query has been run in this window yet.".to_string(),
        ),
    }
}

fn start_table_preview(app: &mut DbHiveApp, ctx: &egui::Context, connection_id: &str) {
    let s = &app.screens.import_export;
    let schema = s.schema.clone();
    let table = s.export_table.trim().to_string();

    let Some(driver) = app
        .state
        .lock()
        .unwrap()
        .get_connection(connection_id)
        .cloned()
    else {
        app.screens.import_export.last_error =
            Some("Connection is no longer open".to_string());
        return;
    };
    let kind = svc::driver_kind(&app.state, connection_id).unwrap_or(DbDriver::Postgres);
    let sql = format!(
        "SELECT * FROM {} LIMIT {}",
        svc::qualify(&kind, &schema, &table),
        TABLE_PREVIEW_ROWS
    );

    let handle = app.rt.handle().clone();
    app.screens
        .import_export
        .table_preview_job
        .start(&handle, ctx, async move { driver.execute_query(&sql).await });
}

fn start_export(
    app: &mut DbHiveApp,
    ctx: &egui::Context,
    connection_id: &str,
    columns: Vec<String>,
    rows: Vec<Vec<serde_json::Value>>,
) {
    let kind = match svc::driver_kind(&app.state, connection_id) {
        Ok(k) => k,
        Err(e) => {
            app.screens.import_export.last_error = Some(e.to_string());
            return;
        }
    };

    let s = &mut app.screens.import_export;
    let mut opts = s.export.clone();
    opts.row_limit = s.row_limit();
    let source = s.export_source;
    let schema = s.schema.clone();
    let table = s.export_table.trim().to_string();

    let handle = app.rt.handle().clone();
    let state = app.state.clone();
    let id = connection_id.to_string();

    app.screens
        .import_export
        .export_job
        .start(&handle, ctx, async move {
            match source {
                ExportSource::CurrentResult => {
                    svc::export_rows_to_file(kind, columns, rows, opts).await
                }
                ExportSource::Table => {
                    svc::export_table_to_file(&state, &id, &schema, &table, opts).await
                }
            }
        });
}

fn start_preview(app: &mut DbHiveApp, ctx: &egui::Context) {
    let s = &mut app.screens.import_export;
    let path = s.file_path.trim().to_string();
    let opts = PreviewOptions {
        sheet_name: s.options.sheet_name.clone(),
        delimiter: s.options.delimiter,
        max_rows: svc::MAX_PREVIEW_ROWS,
        first_row_is_header: s.options.first_row_is_header,
    };
    s.source = None;
    s.invalidate_plan();

    let handle = app.rt.handle().clone();
    app.screens
        .import_export
        .preview_job
        .start(&handle, ctx, async move {
            // Parsing a large file is CPU- and IO-bound; keeping it off the
            // async worker threads leaves the drivers responsive.
            tokio::task::spawn_blocking(move || svc::preview_import_file(&path, &opts))
                .await
                .unwrap_or_else(|e| {
                    Err(DbError::InternalError(format!("Preview task failed: {}", e)))
                })
        });
}

fn start_prepare(app: &mut DbHiveApp, ctx: &egui::Context) {
    let Some(connection_id) = app.screens.import_export.connection_id.clone() else {
        return;
    };
    let kind = match svc::driver_kind(&app.state, &connection_id) {
        Ok(k) => k,
        Err(e) => {
            app.screens.import_export.last_error = Some(e.to_string());
            return;
        }
    };

    let s = &mut app.screens.import_export;
    s.options.schema = Some(s.schema.clone());
    let path = s.file_path.trim().to_string();
    let options = s.options.clone();
    let cached = s.source.clone();

    let handle = app.rt.handle().clone();
    app.screens
        .import_export
        .prepare_job
        .start(&handle, ctx, async move {
            tokio::task::spawn_blocking(move || {
                // Re-reading is skipped when the file was already parsed with
                // the same header and delimiter settings.
                let source = match cached {
                    Some(s) => (*s).clone(),
                    None => svc::read_import_file(&path, &options)?,
                };
                let plan = svc::build_import_plan(&kind, &source, &options)?;
                Ok((source, plan))
            })
            .await
            .unwrap_or_else(|e| {
                Err(DbError::InternalError(format!("Read task failed: {}", e)))
            })
        });
}

fn start_import(app: &mut DbHiveApp, ctx: &egui::Context, connection_id: &str) {
    let s = &mut app.screens.import_export;
    let Some(source) = s.source.clone() else {
        s.last_error = Some("The file has not been read yet".to_string());
        return;
    };
    let options = s.options.clone();
    let cancel = s.import_cancel.clone();
    cancel.store(false, Ordering::Relaxed);
    s.import_result = None;
    s.step = Step::Running;

    let state = app.state.clone();
    let id = connection_id.to_string();
    let handle = app.rt.handle().clone();
    app.screens
        .import_export
        .import_job
        .start(&handle, ctx, async move {
            svc::run_import(&state, &id, source, options, cancel).await
        });
}

fn ensure_tables(app: &mut DbHiveApp, ctx: &egui::Context, connection_id: &str) {
    let schema = app.screens.import_export.schema.clone();
    let key = (connection_id.to_string(), schema.clone());
    if app.screens.import_export.tables_requested.as_ref() == Some(&key) {
        return;
    }
    app.screens.import_export.tables_requested = Some(key);

    let state = app.state.clone();
    let id = connection_id.to_string();
    let handle = app.rt.handle().clone();
    app.screens
        .import_export
        .tables_job
        .start(&handle, ctx, async move {
            let schema = (!schema.is_empty()).then_some(schema);
            svc::get_tables_for_import(&state, &id, schema.as_deref()).await
        });
}

fn ensure_target_columns(app: &mut DbHiveApp, ctx: &egui::Context, connection_id: &str) {
    let s = &app.screens.import_export;
    let table = s.options.table_name.clone();
    if table.trim().is_empty() {
        return;
    }
    let schema = s.schema.clone();
    let key = (connection_id.to_string(), schema.clone(), table.clone());
    if s.columns_requested.as_ref() == Some(&key) {
        return;
    }
    app.screens.import_export.columns_requested = Some(key);

    let state = app.state.clone();
    let id = connection_id.to_string();
    let handle = app.rt.handle().clone();
    app.screens
        .import_export
        .columns_job
        .start(&handle, ctx, async move {
            let schema = (!schema.is_empty()).then_some(schema);
            svc::get_table_columns_for_import(&state, &id, &table, schema.as_deref()).await
        });
}

/// Drain everything that finished since the last frame.
fn poll(app: &mut DbHiveApp) {
    if let Some(result) = app.screens.import_export.export_job.take_result() {
        match result {
            Ok(rows) => {
                let path = app.screens.import_export.export.path.clone();
                app.toasts
                    .success(format!("Exported {} rows to {}", rows, path));
            }
            Err(e) => {
                let msg = e.to_string();
                app.screens.import_export.last_error = Some(msg.clone());
                app.toasts.error(format!("Export failed: {}", msg));
            }
        }
    }

    if let Some(result) = app.screens.import_export.dump_job.take_result() {
        match result {
            Ok(tables) => {
                let path = app.screens.import_export.dump_path.clone();
                app.toasts
                    .success(format!("Dumped {} table(s) to {}", tables, path));
            }
            Err(e) => {
                let msg = e.to_string();
                app.screens.import_export.last_error = Some(msg.clone());
                app.toasts.error(format!("Dump failed: {}", msg));
            }
        }
    }

    if let Some(result) = app.screens.import_export.table_preview_job.take_result() {
        match result {
            Ok(r) => {
                let s = &mut app.screens.import_export;
                s.export_grid = results_grid::GridState::default();
                s.table_preview = Some(r);
            }
            Err(e) => app.screens.import_export.last_error = Some(e.to_string()),
        }
    }

    if let Some(result) = app.screens.import_export.preview_job.take_result() {
        match result {
            Ok(preview) => {
                let targets = app.screens.import_export.target_columns.clone();
                let s = &mut app.screens.import_export;
                s.preview_grid = results_grid::GridState::default();
                // A fresh file means a fresh mapping; keeping the old one
                // would silently map columns that no longer exist.
                s.options.column_mappings =
                    svc::auto_map_columns(&preview.columns, &preview.detected_types, &targets);
                if s.options.sheet_name.is_none() {
                    s.options.sheet_name =
                        preview.sheet_names.as_ref().and_then(|n| n.first().cloned());
                }
                s.preview = Some(preview);
                s.last_error = None;
                s.step = Step::Preview;
            }
            Err(e) => {
                let msg = e.to_string();
                app.screens.import_export.last_error = Some(msg.clone());
                app.toasts.error(format!("Could not read the file: {}", msg));
            }
        }
    }

    if let Some(result) = app.screens.import_export.tables_job.take_result() {
        match result {
            Ok(tables) => app.screens.import_export.tables = tables,
            // Not fatal: the table can still be typed on the export tab.
            Err(e) => app.screens.import_export.last_error = Some(e.to_string()),
        }
    }

    if let Some(result) = app.screens.import_export.columns_job.take_result() {
        match result {
            Ok(columns) => {
                let s = &mut app.screens.import_export;
                s.target_grid = results_grid::GridState::default();
                // Re-suggest the mapping now that the target is known, but
                // only while the user has not started editing it.
                if let Some(p) = s.preview.as_ref() {
                    let untouched = s
                        .options
                        .column_mappings
                        .iter()
                        .all(|m| m.target_column.is_empty());
                    if untouched {
                        s.options.column_mappings =
                            svc::auto_map_columns(&p.columns, &p.detected_types, &columns);
                    }
                }
                s.target_columns = columns;
            }
            Err(e) => app.screens.import_export.last_error = Some(e.to_string()),
        }
    }

    if let Some(result) = app.screens.import_export.prepare_job.take_result() {
        match result {
            Ok((source, plan)) => {
                let s = &mut app.screens.import_export;
                s.source = Some(Arc::new(source));
                s.plan = Some(plan);
                s.confirm_input.clear();
                s.last_error = None;
                s.step = Step::Confirm;
            }
            Err(e) => {
                let msg = e.to_string();
                let s = &mut app.screens.import_export;
                s.plan = None;
                s.step = Step::Options;
                s.last_error = Some(msg.clone());
                app.toasts.error(msg);
            }
        }
    }

    if let Some(result) = app.screens.import_export.import_job.take_result() {
        match result {
            Ok(r) => {
                let summary = import_summary(&r);
                let success = r.success;
                let s = &mut app.screens.import_export;
                s.import_result = Some(r);
                s.step = Step::Complete;
                if success {
                    app.toasts.success(summary);
                } else {
                    app.toasts.error(summary);
                }
            }
            Err(e) => {
                let msg = e.to_string();
                let s = &mut app.screens.import_export;
                s.step = Step::Confirm;
                s.last_error = Some(msg.clone());
                app.toasts.error(format!("Import failed: {}", msg));
            }
        }
    }

    if let Some(result) = app.screens.import_export.sql_import_job.take_result() {
        match result {
            Ok(r) => {
                let summary = sql_import_summary(&r);
                let outcome = sql_import_outcome(&r);
                app.screens.import_export.sql_import_result = Some(r);
                match outcome {
                    Outcome::Success => app.toasts.success(summary),
                    _ => app.toasts.error(summary),
                }
            }
            Err(e) => {
                let msg = e.to_string();
                app.screens.import_export.last_error = Some(msg.clone());
                app.toasts.error(format!("SQL import failed: {}", msg));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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

    #[test]
    fn opening_the_same_connection_twice_keeps_the_wizard_state() {
        let mut s = ImportExport::default();
        s.open_for("conn-1", "public");
        s.file_path = "/tmp/data.csv".to_string();
        s.step = Step::Mapping;

        s.open_for("conn-1", "public");
        assert_eq!(s.file_path, "/tmp/data.csv");
        assert_eq!(s.step, Step::Mapping);

        // A different connection starts clean: the old table list and
        // mapping mean nothing there.
        s.open_for("conn-2", "app");
        assert!(s.file_path.is_empty());
        assert_eq!(s.step, Step::File);
        assert_eq!(s.schema, "app");
        assert_eq!(s.options.schema.as_deref(), Some("app"));
    }

    #[test]
    fn changing_an_option_invalidates_a_plan_that_was_already_reviewed() {
        let mut s = ImportExport::default();
        s.step = Step::Confirm;
        s.confirm_input = "users".to_string();
        s.plan = Some(ImportPlan {
            target: "\"users\"".into(),
            target_columns: vec!["a".into()],
            row_count: 1,
            batch_count: 1,
            truncate_statement: None,
            create_statement: None,
            sample_statements: Vec::new(),
            skipped_columns: Vec::new(),
        });

        s.invalidate_plan();
        assert!(s.plan.is_none());
        assert!(s.confirm_input.is_empty());
        assert_eq!(
            s.step,
            Step::Options,
            "the user is sent back rather than confirming a stale plan"
        );
    }

    #[test]
    fn the_row_limit_only_applies_when_enabled() {
        let mut s = ImportExport::default();
        assert_eq!(s.row_limit(), None);
        s.export_limit_enabled = true;
        s.export_limit = 500;
        assert_eq!(s.row_limit(), Some(500));
        // Zero would export nothing at all, which is never what was meant.
        s.export_limit = 0;
        assert_eq!(s.row_limit(), Some(1));
    }
}
