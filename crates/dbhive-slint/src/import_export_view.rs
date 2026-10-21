//! Maps the import wizard's plain Rust state (`dbhive_ui_model::import_wizard`)
//! to the Slint structs the Import / Export panel draws.
//!
//! Kept apart from the controller so the snapshot tool can build the same
//! models without a controller behind them.

use std::rc::Rc;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use dbhive_core::models::DbDriver;
use dbhive_core::service_import_export::{ColumnMapping, ImportPreview, SqlImportResult};
use dbhive_ui_model::data_forms::{self as forms, DumpForm, SqlFileForm, TableExportForm};
use dbhive_ui_model::import_export::{
    import_summary, sql_import_outcome, sql_import_summary, truncate_middle, Outcome,
};
use dbhive_ui_model::import_wizard::{error_list, result_tone, ImportWizard, ResultTone};
use slint::{ModelRc, SharedString, VecModel};

use crate::grid_model::{layout_columns, Cell, LazyRows};
use crate::{runtime, GridRow, ImportExportState, MappingRow, Tone};

/// Smallest and largest preview column, in logical pixels.
const MIN_COLUMN: f32 = 96.0;
const MAX_COLUMN: f32 = 240.0;
/// A rough advance per character at the body size; the grid elides anyway.
const CHAR_WIDTH: f32 = 7.5;
/// Headers are small caps in a bold face, wider per character than the cells.
const HEADER_CHAR_WIDTH: f32 = 9.0;

/// Everything the panel remembers between openings for one connection.
#[derive(Default)]
pub struct DataSession {
    pub tab: i32,
    pub connection_id: String,
    pub wizard: ImportWizard,

    pub tables_loading: bool,
    pub tables_error: String,
    pub tables_latest: runtime::Latest,
    pub preview_latest: runtime::Latest,
    pub columns_latest: runtime::Latest,
    pub plan_latest: runtime::Latest,

    /// Shown while the wizard reads a file or builds a plan.
    pub busy: String,
    pub columns_loading: bool,
    pub import_path_error: String,
    pub import_cancel: Arc<AtomicBool>,

    pub sql: SqlFileForm,
    pub sql_cancel: Arc<AtomicBool>,
    pub sql_running: bool,
    pub sql_error: String,
    pub sql_path_error: String,
    pub sql_result: Option<SqlImportResult>,

    pub export: TableExportForm,
    pub export_running: bool,
    pub export_error: String,
    pub export_result: String,
    pub export_path_touched: bool,

    pub dump: DumpForm,
    pub dump_running: bool,
    pub dump_error: String,
    pub dump_result: String,
    pub dump_path_touched: bool,
}

/// "Widgets · shop" - the connection and where in it the panel acts. A SQLite
/// file shows its file name, not its path.
pub fn location_label(connection: &str, place: &str) -> String {
    let place = if place.contains('/') || place.contains('\\') {
        std::path::Path::new(place)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| place.to_string())
    } else {
        place.to_string()
    };
    if place.is_empty() || place == connection {
        connection.to_string()
    } else {
        format!("{connection} · {place}")
    }
}

fn outcome_tone(o: Outcome) -> Tone {
    match o {
        Outcome::Success => Tone::Success,
        Outcome::Partial => Tone::Warning,
        Outcome::Cancelled => Tone::Neutral,
    }
}

/// Everything the panel shows, from the session. The text fields the user
/// types into are left alone: they are set when a session starts or a
/// suggestion is applied, never here.
pub fn project(
    g: &ImportExportState,
    d: &DataSession,
    location: &str,
    driver: Option<&DbDriver>,
) {
    let wz = &d.wizard;
    g.set_location(location.into());
        g.set_tab(d.tab);
        g.set_tab_labels(strings(&forms::tab_labels()));
        g.set_tables(strings(&wz.tables));
        g.set_tables_loading(d.tables_loading);
        g.set_tables_error(d.tables_error.clone().into());

        // -- Import wizard
        g.set_step(wz.step.index() as i32);
        g.set_reachable_step(wz.step.index() as i32);
        g.set_busy(!d.busy.is_empty());
        g.set_busy_text(d.busy.clone().into());
        g.set_import_path_error(d.import_path_error.clone().into());
        g.set_import_header(wz.options.first_row_is_header);
        g.set_delimiter_index(wz.delimiter_index());
        g.set_spreadsheet(wz.is_spreadsheet());
        g.set_sheet_options(strings(&wz.sheet_names()));
        g.set_sheet_index(wz.sheet_index());
        g.set_preview_summary(
            wz.preview
                .as_ref()
                .map(dbhive_ui_model::import_export::preview_summary)
                .unwrap_or_default()
                .into(),
        );
        g.set_table_value(
            if wz.options.table_name.is_empty() {
                "Choose a table".to_string()
            } else if wz.is_new_table() {
                format!("{} (new)", wz.options.table_name)
            } else {
                wz.options.table_name.clone()
            }
            .into(),
        );
        g.set_mapping(mapping_rows(wz));
        g.set_target_choices(strings(&wz.target_choices()));
        g.set_mapping_summary(wz.mapping_summary().into());
        g.set_mapping_hint(wz.mapping_hint().into());
        g.set_mapping_ready(wz.mapping_ready());
        g.set_loading_columns(d.columns_loading);
        g.set_truncate(wz.options.truncate_before);
        g.set_options_note(
            if wz.is_new_table() {
                format!(
                    "\"{}\" does not exist yet. The import creates it from the file's columns, then writes the rows.",
                    wz.options.table_name
                )
            } else {
                String::new()
            }
            .into(),
        );
        g.set_import_error(wz.error.clone().into());

        match &wz.plan {
            Some(p) => {
                g.set_plan_target(p.target.clone().into());
                g.set_plan_columns(p.target_columns.join(", ").into());
                g.set_plan_rows(format!("{} in {} statement(s)", p.row_count, p.batch_count).into());
                g.set_plan_skipped(p.skipped_columns.join(", ").into());
                g.set_plan_source(wz.file_path.trim().to_string().into());
                g.set_plan_create(p.create_statement.clone().unwrap_or_default().into());
                g.set_plan_truncate(p.truncate_statement.clone().unwrap_or_default().into());
                g.set_plan_samples_label(
                    format!(
                        "First {} of {} statement(s), verbatim:",
                        p.sample_statements.len(),
                        p.batch_count
                    )
                    .into(),
                );
                g.set_plan_samples(strings(
                    &p.sample_statements.iter().map(|s| truncate_middle(s, 600)).collect::<Vec<_>>(),
                ));
            }
            None => {
                g.set_plan_target("".into());
                g.set_plan_columns("".into());
                g.set_plan_rows("".into());
                g.set_plan_skipped("".into());
                g.set_plan_source("".into());
                g.set_plan_create("".into());
                g.set_plan_truncate("".into());
                g.set_plan_samples(strings(&[]));
            }
        }
        g.set_needs_typed(wz.needs_typed_confirmation());
        g.set_confirm_prompt(
            format!("Type {} to confirm deleting its existing rows.", wz.options.table_name).into(),
        );
        g.set_can_import(wz.can_import());
        g.set_import_label(wz.import_button_label().into());
        g.set_running_text(
            wz.plan
                .as_ref()
                .map(|p| format!("Writing {} rows to {}", p.row_count, p.target))
                .unwrap_or_default()
                .into(),
        );
        if let Some(r) = &wz.result {
            let list = error_list(r);
            g.set_result_tone(tone(result_tone(r)));
            g.set_result_summary(import_summary(r).into());
            g.set_result_errors(strings(&list.lines));
            g.set_result_more(list.more.into());
        }

        // -- Run a SQL file
        g.set_sql_path_error(d.sql_path_error.clone().into());
        g.set_sql_transaction(d.sql.use_transaction);
        g.set_sql_continue(d.sql.continue_on_error);
        g.set_sql_note(d.sql.option_note().into());
        g.set_sql_running(d.sql_running);
        g.set_sql_error(d.sql_error.clone().into());
        match &d.sql_result {
            Some(r) => {
                g.set_sql_has_result(true);
                g.set_sql_result_tone(outcome_tone(sql_import_outcome(r)));
                g.set_sql_result_summary(sql_import_summary(r).into());
                g.set_sql_result_detail(
                    r.first_error.as_deref().map(|e| truncate_middle(e, 400)).unwrap_or_default().into(),
                );
                g.set_sql_log_file(r.log_file.clone().unwrap_or_default().into());
            }
            None => g.set_sql_has_result(false),
        }

        // -- Export a table
        g.set_export_table_value(
            if d.export.table.is_empty() { "Choose a table".to_string() } else { d.export.table.clone() }
                .into(),
        );
        g.set_export_format_options(strings(&forms::table_format_labels()));
        g.set_export_format_index(d.export.format as i32);
        g.set_export_is_csv(d.export.is_csv());
        g.set_export_delimiter_index(d.export.delimiter as i32);
        g.set_export_headers(d.export.include_headers);
        g.set_export_error(d.export_error.clone().into());
        g.set_export_running(d.export_running);
        g.set_export_result(d.export_result.clone().into());

        // -- SQL dump
        g.set_dump_drop(d.dump.include_drop);
        g.set_dump_create(d.dump.include_create);
        g.set_dump_data(d.dump.include_data);
        g.set_dump_scope(d.dump.scope_summary().into());
        g.set_dump_note(
            driver.map(forms::dump_note).unwrap_or_default().into(),
        );
        g.set_dump_error(d.dump_error.clone().into());
        g.set_dump_running(d.dump_running);
        g.set_dump_result(d.dump_result.clone().into());
}

pub fn tone(t: ResultTone) -> Tone {
    match t {
        ResultTone::Success => Tone::Success,
        ResultTone::Warning => Tone::Warning,
        ResultTone::Failure => Tone::Danger,
    }
}

/// Column widths for a preview: wide enough for the header and the first
/// rows, clamped so one long cell cannot push the rest off screen.
pub fn column_widths(titles: &[String], rows: &[Vec<String>]) -> Vec<f32> {
    titles
        .iter()
        .enumerate()
        .map(|(i, title)| {
            let cells = rows
                .iter()
                .take(20)
                .filter_map(|r| r.get(i))
                .map(|c| c.chars().count())
                .max()
                .unwrap_or(0);
            let width = (cells as f32 * CHAR_WIDTH).max(title.chars().count() as f32 * HEADER_CHAR_WIDTH);
            (width + 28.0).clamp(MIN_COLUMN, MAX_COLUMN)
        })
        .collect()
}

/// The preview as grid columns, rows and the total width. Rows are lazy, as
/// for any result, though a preview is at most 100 of them.
pub fn preview_grid(
    preview: &ImportPreview,
) -> (ModelRc<crate::GridColumn>, ModelRc<GridRow>, f32) {
    let widths = column_widths(&preview.columns, &preview.rows);
    let (columns, total) = layout_columns(&preview.columns, &widths);
    let rows: Vec<Vec<Cell>> = preview
        .rows
        .iter()
        .map(|r| {
            (0..preview.columns.len())
                .map(|i| r.get(i).map(|c| c.to_string()))
                .collect()
        })
        .collect();
    (columns, ModelRc::new(LazyRows::new(Rc::new(rows))), total)
}

/// Put the wizard's file preview in the grid, or empty it.
pub fn set_preview(g: &ImportExportState, wz: &ImportWizard) {
    match &wz.preview {
        Some(p) => {
            let (cols, rows, total) = preview_grid(p);
            g.set_preview_columns(cols);
            g.set_preview_rows(rows);
            g.set_preview_width(total);
        }
        None => {
            g.set_preview_columns(ModelRc::default());
            g.set_preview_rows(ModelRc::default());
            g.set_preview_width(0.0);
        }
    }
}

fn mapping_row(m: &ColumnMapping, sample: &str) -> MappingRow {
    MappingRow {
        source: m.source_column.as_str().into(),
        sample: sample.into(),
        target: m.target_column.as_str().into(),
        type_label: m.target_type.clone().unwrap_or_else(|| "TEXT".to_string()).into(),
        default_value: m.default_value.clone().unwrap_or_default().into(),
        skip: m.skip || m.target_column.trim().is_empty(),
    }
}

/// One row per file column, with the first value of the file as a sample.
pub fn mapping_rows(w: &ImportWizard) -> ModelRc<MappingRow> {
    let first = w.preview.as_ref().and_then(|p| p.rows.first());
    let rows: Vec<MappingRow> = w
        .options
        .column_mappings
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let sample = first.and_then(|r| r.get(i)).map(String::as_str).unwrap_or("");
            mapping_row(m, sample)
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

pub fn strings(items: &[String]) -> ModelRc<SharedString> {
    crate::history_view::strings_model(items)
}

/// The data types a mapping can be set to.
pub fn type_options() -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        dbhive_core::service_import_export::DATA_TYPES
            .iter()
            .map(|t| SharedString::from(*t))
            .collect::<Vec<_>>(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::Model;

    fn preview() -> ImportPreview {
        ImportPreview {
            columns: vec!["id".into(), "a_rather_long_column_title_here".into(), "n".into()],
            rows: vec![
                vec!["1".into(), "x".into(), "y".into()],
                vec!["2".into(), "z".into()],
            ],
            total_rows: Some(2),
            detected_types: vec!["INTEGER".into(), "TEXT".into(), "TEXT".into()],
            file_type: "csv".into(),
            sheet_names: None,
        }
    }

    #[test]
    fn widths_follow_content_within_bounds() {
        let p = preview();
        let w = column_widths(&p.columns, &p.rows);
        assert_eq!(w[0], MIN_COLUMN, "short columns do not shrink below the minimum");
        assert!(w[1] > w[0] && w[1] <= MAX_COLUMN);
        let huge = vec![vec!["x".repeat(500)]];
        assert_eq!(column_widths(&["c".to_string()], &huge)[0], MAX_COLUMN);
    }

    #[test]
    fn a_ragged_row_is_padded_with_nulls_not_dropped() {
        let (cols, rows, total) = preview_grid(&preview());
        assert_eq!(cols.row_count(), 3);
        assert_eq!(rows.row_count(), 2);
        assert!(total >= 3.0 * MIN_COLUMN);
        let second = rows.row_data(1).unwrap();
        assert_eq!(second.cells.row_count(), 3);
        assert!(second.cells.row_data(2).unwrap().is_null);
    }

    #[test]
    fn mapping_rows_carry_the_first_value_as_a_sample() {
        let mut w = ImportWizard::new("");
        w.apply_preview(preview());
        let rows = mapping_rows(&w);
        assert_eq!(rows.row_count(), 3);
        assert_eq!(rows.row_data(0).unwrap().sample, "1");
        assert_eq!(rows.row_data(1).unwrap().sample, "x");
        // No targets yet, so every row reads as not imported.
        assert!(rows.row_data(0).unwrap().skip);
    }

    #[test]
    fn the_location_names_the_connection_and_where_in_it() {
        assert_eq!(location_label("Shop", "public"), "Shop · public");
        assert_eq!(location_label("Shop", "Shop"), "Shop");
        assert_eq!(location_label("Shop", ""), "Shop");
        assert_eq!(location_label("Local", "/Users/me/data/shop.sqlite"), "Local · shop.sqlite");
    }

    #[test]
    fn result_tones_map_one_to_one() {
        assert_eq!(tone(ResultTone::Success), Tone::Success);
        assert_eq!(tone(ResultTone::Warning), Tone::Warning);
        assert_eq!(tone(ResultTone::Failure), Tone::Danger);
    }

    #[test]
    fn every_data_type_is_offered() {
        assert_eq!(
            type_options().row_count(),
            dbhive_core::service_import_export::DATA_TYPES.len()
        );
    }
}
