//! Stored procedures and functions.
//!
//! Parity target: the React `StoredProceduresPanel.tsx`, reached from the
//! connected sidebar's "Procedures" entry.
//!
//! The screen lists the routines the server knows about, shows one's source,
//! and runs it with arguments. All three operations live in
//! `dbhive_core::service_procedures`, which owns the per-dialect catalogue
//! SQL, the identifier quoting and the CALL/SELECT/EXEC syntax; this file
//! only draws and collects input.
//!
//! The pure logic (signature splitting, argument parsing, search, grouping)
//! and the plain-data screen state (`ProceduresModel`) live in
//! `dbhive_ui_model::procedures`, with their tests. This file keeps what is
//! egui-bound: the jobs, the editor and grid state, and the drawing.
//!
//! Two rules shape it, the same two as the table inspector:
//!
//! * Nothing blocks the frame. The list, the definition and the execution are
//!   three independent `Job`s and the UI draws whatever has landed.
//! * Nothing reaches SQL unquoted. Arguments are rendered as escaped literals
//!   and names are quoted by the live driver, both inside the core, so a
//!   routine called `select` or an argument containing a quote is fine.
//!
//! The statement that will run is shown before it runs. A stored procedure
//! can do anything, including delete everything, and "Execute" with no
//! visible statement is a blind click.

use dbhive_core::drivers::QueryResult;
use dbhive_core::models::DbError;
use dbhive_core::service_procedures::{self as svc, ProcedureInfo};

pub use dbhive_ui_model::procedures::*;

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::theme::Palette;
use crate::widgets::{results_grid, sql_editor};

// ---------------------------------------------------------------------------
// Screen state
// ---------------------------------------------------------------------------

/// The egui screen: the plain-data model plus what only this shell owns.
///
/// Derefs to `ProceduresModel`, so `procedures.open`, `open_for` and the rest
/// keep resolving exactly as they did before the model was lifted out.
#[derive(Default)]
pub struct Procedures {
    pub model: ProceduresModel,

    pub definition_editor: sql_editor::EditorState,
    pub grid: results_grid::GridState,

    pub list_job: Job<Result<Vec<ProcedureInfo>, DbError>>,
    pub definition_job: Job<Result<String, DbError>>,
    pub run_job: Job<(Result<QueryResult, DbError>, u128)>,
}

impl std::ops::Deref for Procedures {
    type Target = ProceduresModel;
    fn deref(&self) -> &ProceduresModel {
        &self.model
    }
}

impl std::ops::DerefMut for Procedures {
    fn deref_mut(&mut self) -> &mut ProceduresModel {
        &mut self.model
    }
}

impl Procedures {
    /// Select a routine, and reset the result grid when it is a different
    /// one: the previous routine's sort and filter do not apply to this one.
    fn select(&mut self, p: ProcedureInfo) {
        if self.model.select(p) {
            self.grid = results_grid::GridState::default();
        }
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    if !app.screens.procedures.open {
        return;
    }
    let Some(connection_id) = app.screens.procedures.connection_id.clone() else {
        return;
    };

    // Drain finished work before drawing, so this frame shows the newest data.
    {
        let p = &mut app.screens.procedures;
        if let Some(result) = p.list_job.take_result() {
            match result {
                Ok(list) => {
                    p.procedures = list;
                    p.last_error = None;
                }
                Err(e) => p.last_error = Some(e.to_string()),
            }
        }
        if let Some(result) = p.definition_job.take_result() {
            match result {
                Ok(text) => p.definition = text,
                // Keeping the driver's message: "PROCEDURE does not exist"
                // is the whole diagnosis when a name is overloaded.
                Err(e) => p.definition = format!("-- Could not read the definition: {}", e),
            }
        }
    }
    if let Some((result, elapsed)) = app.screens.procedures.run_job.take_result() {
        app.screens.procedures.elapsed_ms = Some(elapsed);
        match result {
            Ok(r) => {
                let rows = r.rows.len();
                app.screens.procedures.grid = results_grid::GridState::default();
                app.screens.procedures.result = Some(r);
                app.screens.procedures.last_error = None;
                app.toasts.success(format!("Executed, {} row(s)", rows));
            }
            Err(e) => {
                app.screens.procedures.result = None;
                let msg = e.to_string();
                app.screens.procedures.last_error = Some(msg.clone());
                app.toasts.error(msg);
            }
        }
    }

    header(app, ui, palette);
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);

    if let Some(err) = app.screens.procedures.last_error.clone() {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(palette.destructive, "Failed:");
            ui.label(err);
            if ui.button("Dismiss").clicked() {
                app.screens.procedures.last_error = None;
            }
        });
        ui.add_space(4.0);
    }

    if app.screens.procedures.unsupported {
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            ui.label(crate::theme::text::muted(
                palette,
                "This database has no stored procedures or functions.",
            ));
        });
        return;
    }

    ui.columns(2, |cols| {
        list_pane(app, &mut cols[0], palette);
        detail_pane(app, &mut cols[1], palette);
    });

    // Started after every borrow of the screen has ended.
    start_pending(app, ui.ctx(), &connection_id);
}

fn header(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let p = &mut app.screens.procedures;
    let busy = p.list_job.is_running();
    let count = p.procedures.len();
    let schema = p.schema.clone();

    let mut refresh = false;
    let mut close = false;

    ui.horizontal(|ui| {
        ui.label(crate::theme::text::heading(palette, "Procedures & functions"));
        if !schema.is_empty() {
            ui.label(crate::theme::text::muted(palette, schema));
        }
        if count > 0 {
            ui.label(crate::theme::text::muted(palette, format!("{}", count)));
        }
        if busy {
            ui.add(egui::Spinner::new().size(14.0));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                close = true;
            }
            if ui.add_enabled(!busy, egui::Button::new("Refresh")).clicked() {
                refresh = true;
            }
        });
    });

    ui.add_space(4.0);
    ui.add(
        egui::TextEdit::singleline(&mut p.search)
            .hint_text("Search by name or schema...")
            .desired_width(260.0),
    );

    if refresh {
        p.refresh();
    }
    if close {
        p.close();
    }
}

fn list_pane(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let (groups, selected, loading) = {
        let p = &app.screens.procedures;
        (
            grouped(&p.procedures, &p.search),
            p.selected.clone(),
            p.list_job.is_running(),
        )
    };

    if groups.is_empty() {
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            if loading {
                ui.add(egui::Spinner::new());
                ui.label(crate::theme::text::muted(palette, "Loading routines"));
            } else if app.screens.procedures.search.trim().is_empty() {
                ui.label(crate::theme::text::muted(
                    palette,
                    "No user-defined routines in this database",
                ));
            } else {
                ui.label(crate::theme::text::muted(palette, "Nothing matches"));
            }
        });
        return;
    }

    // Immediate mode: record the click, act once the list is no longer
    // borrowed.
    let mut to_select: Option<ProcedureInfo> = None;

    egui::ScrollArea::vertical()
        .id_salt("procedures_list")
        .show(ui, |ui| {
            for (schema, items) in &groups {
                ui.label(crate::theme::text::muted(palette, schema.to_uppercase()));
                for p in items {
                    let is_selected = selected.as_ref() == Some(p);
                    let label = format!("{}  {}", p.name, p.kind.label());
                    if ui.selectable_label(is_selected, label).clicked() {
                        to_select = Some(p.clone());
                    }
                    ui.label(crate::theme::text::muted(palette, summary(p)));
                    ui.add_space(2.0);
                }
                ui.add_space(6.0);
            }
        });

    if let Some(p) = to_select {
        app.screens.procedures.select(p);
    }
}

fn detail_pane(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let Some(selected) = app.screens.procedures.selected.clone() else {
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            ui.label(crate::theme::text::muted(
                palette,
                "Select a routine to see its source and run it",
            ));
        });
        return;
    };

    ui.label(crate::theme::text::strong(palette, selected.qualified()));
    ui.label(crate::theme::text::muted(
        palette,
        match &selected.language {
            Some(l) => format!("{} · {}", selected.kind.label(), l),
            None => selected.kind.label().to_string(),
        },
    ));
    ui.add_space(4.0);

    arguments(app, ui, palette, &selected);
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);
    definition(app, ui, palette);
    ui.add_space(6.0);
    result(app, ui, palette);
}

fn arguments(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette, selected: &ProcedureInfo) {
    let names = parse_signature(&selected.argument_signature);
    let running = app.screens.procedures.run_job.is_running();

    {
        let p = &mut app.screens.procedures;
        // A signature that came back later than the selection, or a list
        // refresh that changed it, must not leave stale boxes behind.
        if p.args.len() != names.len() {
            p.args.resize(names.len(), String::new());
        }
        if names.is_empty() {
            ui.label(crate::theme::text::muted(palette, "Takes no arguments"));
        }
        for (i, name) in names.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(name).monospace().color(palette.muted_foreground));
                ui.add(
                    egui::TextEdit::singleline(&mut p.args[i])
                        .hint_text("42, \"text\", true, or empty for NULL")
                        .desired_width(200.0),
                );
            });
        }
    }

    // The statement is built by the core, from the live driver's quoting, so
    // what is shown is exactly what will run.
    let args: Vec<serde_json::Value> = app
        .screens
        .procedures
        .args
        .iter()
        .map(|a| parse_arg(a))
        .collect();
    let preview = app
        .screens
        .procedures
        .connection_id
        .clone()
        .map(|id| {
            svc::preview_call(
                &app.state,
                &id,
                &selected.schema,
                &selected.name,
                selected.kind,
                &args,
            )
        })
        .unwrap_or_else(|| Err(DbError::NotFound("No connection".into())));

    ui.add_space(4.0);
    match &preview {
        Ok(sql) => {
            ui.label(crate::theme::text::muted(palette, "Statement"));
            egui::Frame::none()
                .fill(palette.card)
                .stroke(egui::Stroke::new(1.0_f32, palette.border))
                .rounding(egui::Rounding::same(6.0))
                .inner_margin(egui::Margin::same(6.0))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(sql).monospace().color(palette.foreground));
                });
        }
        Err(e) => {
            ui.colored_label(palette.destructive, e.to_string());
        }
    }

    ui.add_space(4.0);
    let mut run = false;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                preview.is_ok() && !running,
                egui::Button::new(format!("Run {}", selected.kind.label())),
            )
            .clicked()
        {
            run = true;
        }
        if running {
            ui.add(egui::Spinner::new());
        }
        if let Some(ms) = app.screens.procedures.elapsed_ms {
            ui.label(crate::theme::text::muted(palette, format!("{} ms", ms)));
        }
    });

    if run {
        start_run(app, ui.ctx(), selected, args);
    }
}

fn definition(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let loading = app.screens.procedures.definition_job.is_running();
    let empty = app.screens.procedures.definition.is_empty();

    let mut load = false;
    ui.horizontal(|ui| {
        ui.label(crate::theme::text::strong(palette, "Source"));
        if loading {
            ui.add(egui::Spinner::new().size(14.0));
        } else if ui
            .small_button(if empty { "Show" } else { "Reload" })
            .clicked()
        {
            load = true;
        }
    });

    if !empty {
        // The SQL editor, so the source is highlighted and searchable. It is
        // editable, but nothing writes it back: a routine is changed by
        // running a CREATE OR REPLACE, not by typing over the listing.
        let schema_index = sql_editor::SchemaIndex::default();
        let p = &mut app.screens.procedures;
        let mut text = std::mem::take(&mut p.definition);
        sql_editor::show(ui, palette, &mut p.definition_editor, &schema_index, &mut text);
        p.definition = text;
    }

    if load {
        start_definition(app, ui.ctx());
    }
}

fn result(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let p = &mut app.screens.procedures;
    let Some(result) = p.result.take() else {
        return;
    };
    ui.label(crate::theme::text::strong(palette, "Result"));
    results_grid::show(ui, palette, &result, &mut p.grid);
    p.result = Some(result);
}

// ---------------------------------------------------------------------------
// Async work
// ---------------------------------------------------------------------------

fn start_pending(app: &mut DbHiveApp, ctx: &egui::Context, connection_id: &str) {
    if !app.screens.procedures.needs_list || app.screens.procedures.list_job.is_running() {
        return;
    }
    app.screens.procedures.needs_list = false;

    // A driver with no routines is answered here rather than with an empty
    // list that looks like a failed load.
    let supported = {
        let guard = app.state.lock().unwrap();
        match guard.get_profile(connection_id) {
            Some(profile) => svc::supports_routines(&profile.driver),
            None => {
                drop(guard);
                app.screens.procedures.last_error = Some("Connection is no longer open".into());
                return;
            }
        }
    };
    app.screens.procedures.unsupported = !supported;
    if !supported {
        app.screens.procedures.procedures.clear();
        return;
    }

    let state = app.state.clone();
    let id = connection_id.to_string();
    let schema = app.screens.procedures.schema.clone();
    let handle = app.rt.handle().clone();
    app.screens
        .procedures
        .list_job
        .start(&handle, ctx, async move {
            let schema = if schema.trim().is_empty() {
                None
            } else {
                Some(schema)
            };
            svc::list_procedures(&state, &id, schema.as_deref()).await
        });
}

fn start_definition(app: &mut DbHiveApp, ctx: &egui::Context) {
    let (Some(id), Some(selected)) = (
        app.screens.procedures.connection_id.clone(),
        app.screens.procedures.selected.clone(),
    ) else {
        return;
    };
    let state = app.state.clone();
    let handle = app.rt.handle().clone();
    app.screens
        .procedures
        .definition_job
        .start(&handle, ctx, async move {
            svc::get_procedure_definition(
                &state,
                &id,
                &selected.schema,
                &selected.name,
                Some(selected.kind),
            )
            .await
        });
}

fn start_run(
    app: &mut DbHiveApp,
    ctx: &egui::Context,
    selected: &ProcedureInfo,
    args: Vec<serde_json::Value>,
) {
    let Some(id) = app.screens.procedures.connection_id.clone() else {
        return;
    };
    let state = app.state.clone();
    let handle = app.rt.handle().clone();
    let (schema, name, kind) = (
        selected.schema.clone(),
        selected.name.clone(),
        selected.kind,
    );
    app.screens
        .procedures
        .run_job
        .start(&handle, ctx, async move {
            let started = std::time::Instant::now();
            let out = svc::execute_procedure(&state, &id, &schema, &name, kind, &args).await;
            (out, started.elapsed().as_millis())
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbhive_core::service_procedures::RoutineKind;

    fn proc(schema: &str, name: &str, kind: RoutineKind, sig: &str) -> ProcedureInfo {
        ProcedureInfo {
            schema: schema.into(),
            name: name.into(),
            kind,
            language: None,
            return_type: None,
            argument_signature: sig.into(),
        }
    }

    #[test]
    fn selecting_another_routine_resets_the_result_grid() {
        let mut screen = Procedures::default();
        screen.select(proc("public", "add", RoutineKind::Function, "(a integer)"));
        screen.grid.selected_row = Some(3);
        screen.grid.filter = "x".into();

        // Reselecting the same routine keeps the grid as the user left it.
        screen.select(proc("public", "add", RoutineKind::Function, "(a integer)"));
        assert_eq!(screen.grid.selected_row, Some(3));

        screen.select(proc("public", "sub", RoutineKind::Function, "()"));
        assert_eq!(screen.grid.selected_row, None);
        assert!(screen.grid.filter.is_empty());
        assert!(screen.args.is_empty());
    }

    #[test]
    fn the_screen_exposes_the_model_through_its_old_paths() {
        let mut screen = Procedures::default();
        screen.open_for("conn", "public");
        assert!(screen.open);
        assert_eq!(screen.connection_id.as_deref(), Some("conn"));
        screen.open = false;
        assert!(!screen.model.open);
    }
}
