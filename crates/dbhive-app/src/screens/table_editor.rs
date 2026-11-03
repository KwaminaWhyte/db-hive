//! Create, alter and drop tables, and create databases.
//!
//! The SQL is produced by `dbhive_core::ddl`, which already knows each
//! dialect, so this screen never writes DDL by hand. It shows the generated
//! statements before running them: DDL is not undoable, and a user who
//! cannot see what is about to run has no way to catch a mistake.
//!
//! Four modes share one form, because they share one shape: pick a target,
//! edit it, read the SQL, confirm, run.
//!
//! * **Create table** builds a `TableDefinition` from an empty form.
//! * **Alter table** loads the live table, lets the same column editor edit
//!   it, and *diffs* the result: the operations are derived from what
//!   changed, never from what the form happens to contain. A form that is
//!   untouched produces no SQL at all.
//! * **Drop table** is unrecoverable, so beyond the usual confirmation it
//!   makes the user type the table's name. A misplaced click cannot do it.
//! * **Create database** is here rather than in its own screen because it is
//!   the same preview-then-confirm flow over `dbhive_core::service`.

use dbhive_core::models::{DbDriver, DbError, TableSchema};

// The pure logic lives in `dbhive_ui_model::table_editor`. Re-exported so
// existing paths (`screens::table_editor::ColumnRow`, used by the schema
// designer) keep resolving.
pub use dbhive_ui_model::table_editor::{
    column_type, drop_confirmed, preview_alter_sql, preview_drop_sql, preview_sql, validate,
    ColumnRow, EditRow, Mode, TypeKind,
};
use dbhive_ui_model::table_editor::{
    database_name_errors, drop_table_errors, preview_create_database_sql, qualified_display,
};

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::theme::Palette;

#[derive(Default)]
pub struct TableEditor {
    pub open: bool,
    pub mode: Mode,
    pub connection_id: Option<String>,
    pub schema: String,
    pub table_name: String,
    pub columns: Vec<EditRow>,
    pub preview: Vec<String>,
    pub error: Option<String>,
    pub run_job: Job<Result<(), DbError>>,
    pub confirming: bool,

    /// Loading the table being altered. Alter mode cannot diff against
    /// something it has not read, so the form waits for this.
    pub load_job: Job<Result<TableSchema, DbError>>,
    /// The table exactly as loaded. Kept separately from the rows so a
    /// column the user deleted is still known to have existed.
    pub loaded: Vec<ColumnRow>,
    /// Set when the table being altered still has to be read. A flag rather
    /// than "loaded is empty", so a load that comes back with nothing does
    /// not restart itself on every frame.
    needs_load: bool,
    /// The user's typed confirmation for a drop.
    pub drop_confirm: String,
    pub drop_cascade: bool,
    /// The name for a new database.
    pub database_name: String,
}

impl TableEditor {
    /// Reset everything that is per-target, keeping nothing from the last
    /// use. Every `open_*` goes through here so a stale preview or a
    /// half-typed drop confirmation cannot survive into the next target.
    fn reset(&mut self, mode: Mode, connection_id: impl Into<String>) {
        self.open = true;
        self.mode = mode;
        self.connection_id = Some(connection_id.into());
        self.error = None;
        self.preview.clear();
        self.confirming = false;
        self.columns.clear();
        self.loaded.clear();
        self.needs_load = false;
        self.drop_confirm.clear();
        self.drop_cascade = false;
        self.database_name.clear();
        self.table_name.clear();
    }

    pub fn open_new(&mut self, connection_id: impl Into<String>, schema: impl Into<String>) {
        self.reset(Mode::CreateTable, connection_id);
        self.schema = schema.into();
        // A table with no columns cannot be created, so start with the one
        // almost every table has.
        self.columns = vec![EditRow::new(ColumnRow {
            name: "id".into(),
            type_kind: TypeKind::BigInt,
            nullable: false,
            primary_key: true,
            auto_increment: true,
            ..Default::default()
        })];
    }

    /// Edit an existing table. The columns arrive from a background load.
    pub fn open_alter(
        &mut self,
        connection_id: impl Into<String>,
        schema: impl Into<String>,
        table: impl Into<String>,
    ) {
        self.reset(Mode::AlterTable, connection_id);
        self.schema = schema.into();
        self.table_name = table.into();
        self.needs_load = true;
    }

    /// Drop an existing table. No load is needed: dropping does not depend
    /// on the columns.
    pub fn open_drop(
        &mut self,
        connection_id: impl Into<String>,
        schema: impl Into<String>,
        table: impl Into<String>,
    ) {
        self.reset(Mode::DropTable, connection_id);
        self.schema = schema.into();
        self.table_name = table.into();
    }

    pub fn open_create_database(&mut self, connection_id: impl Into<String>) {
        self.reset(Mode::CreateDatabase, connection_id);
        self.schema = String::new();
    }

    /// The rows as plain columns, for validation and definition building.
    fn plain_columns(&self) -> Vec<ColumnRow> {
        self.columns.iter().map(|r| r.col.clone()).collect()
    }
}

/// Turn a live table's schema into editor rows.
///
/// The type string is mapped back onto a picker choice by
/// `schema_designer::parse_type`, which keeps `VARCHAR(50)`'s 50 and falls
/// back to a custom type carrying the original text. Anything it cannot name
/// round-trips unchanged rather than silently becoming something else.
pub fn rows_from_schema(schema: &TableSchema) -> Vec<EditRow> {
    schema
        .columns
        .iter()
        .map(|c| {
            let (kind, length, precision, scale, custom) =
                crate::screens::schema_designer::parse_type(&c.data_type);
            EditRow::existing(ColumnRow {
                name: c.name.clone(),
                type_kind: kind,
                length,
                precision,
                scale,
                custom,
                nullable: c.nullable && !c.is_primary_key,
                primary_key: c.is_primary_key,
                auto_increment: c.is_auto_increment,
                default: c.default_value.clone().unwrap_or_default(),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    if !app.screens.table_editor.open {
        return;
    }
    let Some(connection_id) = app.screens.table_editor.connection_id.clone() else {
        return;
    };
    let driver_kind = app
        .state
        .lock()
        .unwrap()
        .get_profile(&connection_id)
        .map(|p| p.driver.clone());

    let mode = app.screens.table_editor.mode;

    // Drain finished work before drawing, so this frame shows the newest
    // state rather than last frame's.
    if let Some(result) = app.screens.table_editor.load_job.take_result() {
        let ed = &mut app.screens.table_editor;
        match result {
            Ok(schema) => {
                ed.columns = rows_from_schema(&schema);
                ed.loaded = ed.columns.iter().map(|r| r.col.clone()).collect();
                ed.error = None;
            }
            Err(e) => ed.error = Some(e.to_string()),
        }
    }
    if let Some(result) = app.screens.table_editor.run_job.take_result() {
        match result {
            Ok(()) => {
                app.toasts.success(mode.done_message());
                app.screens.table_editor.open = false;
            }
            Err(e) => app.screens.table_editor.error = Some(e.to_string()),
        }
    }

    // Alter mode needs the live table before it can diff anything.
    if app.screens.table_editor.needs_load {
        app.screens.table_editor.needs_load = false;
        start_load(app, ui.ctx());
    }

    ui.horizontal(|ui| {
        ui.label(crate::theme::text::heading(palette, mode.title()));
        if mode != Mode::CreateTable && mode != Mode::CreateDatabase {
            ui.label(crate::theme::text::muted(
                palette,
                qualified_display(
                    &app.screens.table_editor.schema,
                    &app.screens.table_editor.table_name,
                ),
            ));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                app.screens.table_editor.open = false;
            }
        });
    });
    ui.add_space(6.0);

    match mode {
        Mode::CreateTable => create_table_form(app, ui, palette),
        Mode::AlterTable => alter_table_form(app, ui, palette),
        Mode::DropTable => drop_table_form(app, ui, palette),
        Mode::CreateDatabase => create_database_form(app, ui, palette),
    }

    // The form can switch mode: alter offers a drop. Everything below acts on
    // the mode as it is now, so a frame can never preview a DROP while still
    // gating it as an ALTER.
    let mode = app.screens.table_editor.mode;

    // Recompute the preview, then release the borrow before drawing: the
    // confirm button needs `app` again to launch the job.
    let (errors, preview, error, confirming, running) = {
        let ed = &mut app.screens.table_editor;
        let errors = form_errors(ed);
        if let Some(driver) = driver_kind.as_ref() {
            if errors.is_empty() {
                match build_preview(ed, driver) {
                    Ok(sql) => {
                        ed.preview = sql;
                        ed.error = None;
                    }
                    Err(e) => {
                        ed.preview.clear();
                        ed.error = Some(e.to_string());
                    }
                }
            } else {
                ed.preview.clear();
            }
        }
        (
            errors,
            ed.preview.clone(),
            ed.error.clone(),
            ed.confirming,
            ed.run_job.is_running(),
        )
    };

    ui.add_space(8.0);
    for e in &errors {
        ui.colored_label(palette.destructive, e);
    }
    if let Some(err) = error {
        ui.colored_label(palette.destructive, err);
    }

    if !preview.is_empty() {
        ui.add_space(6.0);
        ui.label(crate::theme::text::strong(palette, "SQL to run"));
        egui::Frame::none()
            .fill(palette.card)
            .stroke(egui::Stroke::new(1.0_f32, palette.border))
            .inner_margin(egui::Margin::same(8.0))
            .rounding(egui::Rounding::same(6.0))
            .show(ui, |ui| {
                for stmt in &preview {
                    ui.label(
                        egui::RichText::new(stmt)
                            .monospace()
                            .color(palette.foreground),
                    );
                }
            });
    } else if mode == Mode::AlterTable && errors.is_empty() {
        ui.add_space(6.0);
        ui.label(crate::theme::text::muted(
            palette,
            "No changes yet. Edit, add or remove a column to see the SQL.",
        ));
    }

    ui.add_space(8.0);
    let mut start = false;
    let mut set_confirming: Option<bool> = None;
    let confirmed_name = drop_confirmed(
        &app.screens.table_editor.table_name,
        &app.screens.table_editor.drop_confirm,
    );

    ui.horizontal(|ui| {
        let mut ready = errors.is_empty() && !preview.is_empty() && !running;
        if mode == Mode::DropTable {
            ready = ready && confirmed_name;
        }
        if confirming {
            ui.label(crate::theme::text::strong(palette, "Run this DDL?"));
            if mode == Mode::DropTable {
                ui.colored_label(palette.destructive, "This cannot be undone.");
            }
            if ui.button(mode.confirm_label()).clicked() {
                set_confirming = Some(false);
                start = true;
            }
            if ui.button("Cancel").clicked() {
                set_confirming = Some(false);
            }
        } else if ui
            .add_enabled(ready, egui::Button::new(mode.action_label()))
            .clicked()
        {
            set_confirming = Some(true);
        }
        if running {
            ui.add(egui::Spinner::new());
        }
    });

    if let Some(v) = set_confirming {
        app.screens.table_editor.confirming = v;
    }
    if start {
        run(app, ui.ctx());
    }
}

/// Form-level validation for the active mode.
fn form_errors(ed: &TableEditor) -> Vec<String> {
    match ed.mode {
        Mode::CreateTable => validate(&ed.table_name, &ed.plain_columns()),
        Mode::AlterTable => {
            if ed.loaded.is_empty() {
                // Nothing loaded yet: the diff has nothing to compare
                // against, so complaining about the form would be noise.
                Vec::new()
            } else {
                validate(&ed.table_name, &ed.plain_columns())
            }
        }
        Mode::DropTable => drop_table_errors(&ed.table_name),
        Mode::CreateDatabase => database_name_errors(&ed.database_name),
    }
}

/// The statements the active mode would run.
fn build_preview(ed: &TableEditor, driver: &DbDriver) -> Result<Vec<String>, DbError> {
    match ed.mode {
        Mode::CreateTable => preview_sql(driver, &ed.schema, &ed.table_name, &ed.plain_columns()),
        Mode::AlterTable => {
            preview_alter_sql(driver, &ed.schema, &ed.table_name, &ed.loaded, &ed.columns)
        }
        Mode::DropTable => preview_drop_sql(driver, &ed.schema, &ed.table_name, ed.drop_cascade),
        Mode::CreateDatabase => preview_create_database_sql(driver, &ed.database_name),
    }
}

fn create_table_form(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    ui.horizontal(|ui| {
        ui.label("Name");
        ui.add(
            egui::TextEdit::singleline(&mut app.screens.table_editor.table_name)
                .hint_text("table_name")
                .desired_width(240.0),
        );
        ui.label(crate::theme::text::muted(
            palette,
            format!("in {}", app.screens.table_editor.schema),
        ));
    });
    ui.add_space(8.0);
    columns_editor(app, ui, palette);
}

fn alter_table_form(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    if app.screens.table_editor.load_job.is_running() {
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            ui.add(egui::Spinner::new());
            ui.label(crate::theme::text::muted(palette, "Loading the table"));
        });
        return;
    }
    if app.screens.table_editor.loaded.is_empty() {
        ui.label(crate::theme::text::muted(
            palette,
            "The table's columns could not be read, so there is nothing to diff against.",
        ));
        return;
    }

    let mut drop_instead = false;
    ui.horizontal(|ui| {
        ui.label(crate::theme::text::muted(
            palette,
            format!("{} columns loaded", app.screens.table_editor.loaded.len()),
        ));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button("Drop table…")
                .on_hover_text("Delete this table and everything in it")
                .clicked()
            {
                drop_instead = true;
            }
        });
    });
    ui.add_space(6.0);
    columns_editor(app, ui, palette);

    if drop_instead {
        let ed = &app.screens.table_editor;
        let (conn, schema, table) = (
            ed.connection_id.clone().unwrap_or_default(),
            ed.schema.clone(),
            ed.table_name.clone(),
        );
        app.screens.table_editor.open_drop(conn, schema, table);
    }
}

fn drop_table_form(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let name = app.screens.table_editor.table_name.clone();
    ui.colored_label(
        palette.destructive,
        format!(
            "Dropping {} deletes the table and every row in it. This cannot be undone.",
            qualified_display(&app.screens.table_editor.schema, &name)
        ),
    );
    ui.add_space(6.0);

    ui.checkbox(
        &mut app.screens.table_editor.drop_cascade,
        "Also drop dependent objects (CASCADE)",
    );
    ui.add_space(6.0);

    ui.label(crate::theme::text::strong(
        palette,
        format!("Type {} to confirm", name),
    ));
    let matched = drop_confirmed(&name, &app.screens.table_editor.drop_confirm);
    ui.add(
        egui::TextEdit::singleline(&mut app.screens.table_editor.drop_confirm)
            .hint_text(name.clone())
            .desired_width(240.0),
    );
    if !matched && !app.screens.table_editor.drop_confirm.is_empty() {
        ui.label(crate::theme::text::muted(
            palette,
            "The name has to match exactly, including case.",
        ));
    }
}

fn create_database_form(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    ui.horizontal(|ui| {
        ui.label("Name");
        ui.add(
            egui::TextEdit::singleline(&mut app.screens.table_editor.database_name)
                .hint_text("database_name")
                .desired_width(240.0),
        );
    });
    ui.add_space(4.0);
    ui.label(crate::theme::text::muted(
        palette,
        "The new database is created on the server this connection is already on. \
         Switch to it from the database picker once it exists.",
    ));
}

fn columns_editor(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let mut remove: Option<usize> = None;
    let mut add = false;

    egui::ScrollArea::vertical()
        .max_height(280.0)
        .show(ui, |ui| {
            let ed = &mut app.screens.table_editor;
            for (i, row) in ed.columns.iter_mut().enumerate() {
                // Read before the mutable borrow: the "was x" hint needs the
                // original name while the editor holds the current one.
                let origin_name = row.origin.as_ref().map(|o| o.name.trim().to_string());
                let col = &mut row.col;
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut col.name)
                            .hint_text("column")
                            .desired_width(140.0),
                    );

                    egui::ComboBox::from_id_salt(("coltype", i))
                        .selected_text(col.type_kind.label())
                        .width(160.0)
                        .show_ui(ui, |ui| {
                            for kind in TypeKind::ALL {
                                ui.selectable_value(&mut col.type_kind, *kind, kind.label());
                            }
                        });

                    if col.type_kind.takes_length() {
                        ui.add(egui::DragValue::new(&mut col.length).range(1..=65535));
                    }
                    if col.type_kind.takes_precision() {
                        ui.add(egui::DragValue::new(&mut col.precision).range(1..=38));
                        ui.add(egui::DragValue::new(&mut col.scale).range(0..=38));
                    }
                    if col.type_kind.takes_custom_name() {
                        ui.add(
                            egui::TextEdit::singleline(&mut col.custom)
                                .hint_text("type name")
                                .desired_width(120.0),
                        );
                    }

                    if ui.checkbox(&mut col.primary_key, "PK").changed() && col.primary_key {
                        // Primary keys are NOT NULL; keep the form honest
                        // rather than generating DDL the server will reject.
                        col.nullable = false;
                    }
                    ui.add_enabled(
                        !col.primary_key,
                        egui::Checkbox::new(&mut col.nullable, "Null"),
                    );
                    ui.add_enabled(
                        col.type_kind.can_auto_increment(),
                        egui::Checkbox::new(&mut col.auto_increment, "Auto"),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut col.default)
                            .hint_text("default")
                            .desired_width(100.0),
                    );
                    if ui
                        .small_button("✖")
                        .on_hover_text("Remove column")
                        .clicked()
                    {
                        remove = Some(i);
                    }
                    // An existing column that has been renamed says so, since
                    // the SQL will carry a RENAME the user should expect.
                    if let Some(was) = origin_name.as_ref() {
                        if was != col.name.trim() && !col.name.trim().is_empty() {
                            ui.label(crate::theme::text::muted(palette, format!("was {}", was)));
                        }
                    }
                });
            }
        });

    ui.add_space(4.0);
    if ui.button("Add column").clicked() {
        add = true;
    }

    let ed = &mut app.screens.table_editor;
    if let Some(i) = remove {
        ed.columns.remove(i);
    }
    if add {
        ed.columns.push(EditRow::new(ColumnRow::default()));
    }
}

fn start_load(app: &mut DbHiveApp, ctx: &egui::Context) {
    let ed = &app.screens.table_editor;
    let (Some(connection_id), schema, table) = (
        ed.connection_id.clone(),
        ed.schema.clone(),
        ed.table_name.clone(),
    ) else {
        return;
    };
    let Some(driver) = app
        .state
        .lock()
        .unwrap()
        .get_connection(&connection_id)
        .cloned()
    else {
        app.screens.table_editor.error = Some("Connection is no longer open".into());
        return;
    };
    let handle = app.rt.handle().clone();
    app.screens
        .table_editor
        .load_job
        .start(&handle, ctx, async move {
            driver.get_table_schema(&schema, &table).await
        });
}

fn run(app: &mut DbHiveApp, ctx: &egui::Context) {
    let ed = &app.screens.table_editor;
    let Some(connection_id) = ed.connection_id.clone() else {
        return;
    };
    let mode = ed.mode;
    let statements = ed.preview.clone();
    let database_name = ed.database_name.clone();
    let handle = app.rt.handle().clone();

    if mode == Mode::CreateDatabase {
        // Creating a database goes through the service so the statement that
        // runs is the one the core builds from the live driver's quoting,
        // not the preview's reconstruction of it.
        let state = app.state.clone();
        app.screens
            .table_editor
            .run_job
            .start(&handle, ctx, async move {
                dbhive_core::service::create_database(&state, &connection_id, &database_name)
                    .await
                    .map(|_| ())
            });
        return;
    }

    let Some(driver) = app
        .state
        .lock()
        .unwrap()
        .get_connection(&connection_id)
        .cloned()
    else {
        app.screens.table_editor.error = Some("Connection is no longer open".into());
        return;
    };
    app.screens
        .table_editor
        .run_job
        .start(&handle, ctx, async move {
            // Statements run in order and stop at the first failure, so a
            // broken index does not leave later statements applied on top.
            for stmt in statements {
                driver.execute_query(&stmt).await?;
            }
            Ok(())
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── modes ────────────────────────────────────────────────────────────

    #[test]
    fn opening_a_mode_clears_the_previous_one() {
        let mut ed = TableEditor::default();
        ed.open_new("conn", "public");
        ed.preview = vec!["CREATE TABLE ...".into()];
        ed.confirming = true;

        ed.open_drop("conn", "public", "users");
        assert_eq!(ed.mode, Mode::DropTable);
        assert!(ed.preview.is_empty(), "a stale preview survived");
        assert!(!ed.confirming, "a stale confirmation survived");
        assert_eq!(ed.table_name, "users");

        // A drop confirmation must never carry over to the next table.
        ed.drop_confirm = "users".into();
        ed.open_drop("conn", "public", "orders");
        assert!(!drop_confirmed(&ed.table_name, &ed.drop_confirm));
    }

    #[test]
    fn create_table_starts_with_an_id_column() {
        let mut ed = TableEditor::default();
        ed.open_new("conn", "public");
        assert_eq!(ed.mode, Mode::CreateTable);
        assert_eq!(ed.columns.len(), 1);
        assert_eq!(ed.columns[0].col.name, "id");
        // It is new, not loaded: it must diff as an addition, never a change.
        assert!(ed.columns[0].origin.is_none());
    }

    #[test]
    fn alter_starts_empty_and_waits_for_the_load() {
        let mut ed = TableEditor::default();
        ed.open_alter("conn", "public", "users");
        assert_eq!(ed.mode, Mode::AlterTable);
        assert!(ed.columns.is_empty());
        assert!(ed.loaded.is_empty());
        assert!(ed.needs_load, "alter mode must ask for the live table");
        // With nothing loaded there is nothing to complain about yet.
        assert!(form_errors(&ed).is_empty());

        // Creating or dropping needs no load, so neither asks for one.
        ed.open_new("conn", "public");
        assert!(!ed.needs_load);
        ed.open_drop("conn", "public", "users");
        assert!(!ed.needs_load);
    }

    #[test]
    fn a_database_name_is_validated_by_the_core() {
        let mut ed = TableEditor::default();
        ed.open_create_database("conn");
        // Empty is a prompt, not an error.
        assert!(form_errors(&ed).is_empty());
        ed.database_name = "1bad".into();
        assert!(!form_errors(&ed).is_empty());
        ed.database_name = "analytics".into();
        assert!(form_errors(&ed).is_empty());
        assert_eq!(
            build_preview(&ed, &DbDriver::Postgres).unwrap(),
            vec!["CREATE DATABASE \"analytics\"".to_string()]
        );
    }

    #[test]
    fn create_database_is_refused_where_it_makes_no_sense() {
        let mut ed = TableEditor::default();
        ed.open_create_database("conn");
        ed.database_name = "analytics".into();
        assert!(build_preview(&ed, &DbDriver::Sqlite).is_err());
        assert!(build_preview(&ed, &DbDriver::MongoDb).is_err());
    }
}
