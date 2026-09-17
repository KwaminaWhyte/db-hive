//! Schema migrations: diff two connections and sync the target.
//!
//! Parity target: the React `MigrationsDialog` (2.9 in
//! docs/rust-ui-migration/01-frontend-inventory.md). The same three-step
//! wizard, driving the same three operations, now in
//! `dbhive_core::service_migrations`:
//!
//! ```text
//! Select  pick source + target connection and a schema name
//! Diff    compute_schema_diff, reviewed as a tree or as a flat grid
//! SQL     generate_migration, reviewed in the editor, then apply_migration
//! ```
//!
//! The pure logic (statement splitting, diff projection, validation,
//! formatting) and the plain-data wizard state live in
//! `dbhive_ui_model::migrations` and are re-exported here; this file keeps
//! the drawing, the widget state and the background jobs.
//!
//! Two deliberate differences from the web shell, both forced by the widgets
//! this crate standardises on:
//!
//! * The generated SQL is **editable**. `widgets::sql_editor` has no
//!   read-only mode, and hiding the statements behind a plain label would be
//!   worse. What gets executed is therefore re-parsed from the buffer with
//!   [`split_statements`] rather than being the generator's `Vec<String>`,
//!   and "Reset to generated" undoes any edit.
//! * Apply is gated twice, not once: the acknowledgement checkbox the web
//!   shell had, *and* a confirmation dialog naming the target and the
//!   statement count. This is the only operation in the screen that writes.
//!
//! There is no rollback action, because there is nothing to roll back to:
//! the backend keeps no migration history (`apply_migration` executes and
//! forgets). The transaction toggle is the only undo, and it only covers the
//! run in progress.

use std::collections::HashSet;
use std::ops::{Deref, DerefMut};

use dbhive_core::migrations::{SchemaDiff, TableDiff};
use dbhive_core::models::{ConnectionProfile, DbError};
use dbhive_core::service_migrations::ApplyResult;

pub use dbhive_ui_model::migrations::*;

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::widgets::{results_grid, sql_editor};

// ===========================================================================
// State
// ===========================================================================

/// The egui wizard: the shared plain-data [`Wizard`] plus this shell's
/// widget state and jobs. Derefs to `Wizard`, so `migrations.step` and the
/// other plain fields read as they did before the split.
pub struct Migrations {
    pub open: bool,

    pub wizard: Wizard,

    pub diff_grid: results_grid::GridState,
    pub editor: sql_editor::EditorState,

    pub diff_job: Job<Result<SchemaDiff, DbError>>,
    pub apply_job: Job<Result<ApplyResult, DbError>>,

    /// Tracks the open -> closed edge so reopening starts a fresh wizard,
    /// the way remounting the React dialog did.
    was_open: bool,
}

impl Default for Migrations {
    fn default() -> Self {
        Self {
            open: false,
            wizard: Wizard::default(),
            diff_grid: results_grid::GridState::default(),
            editor: sql_editor::EditorState::default(),
            diff_job: Job::default(),
            apply_job: Job::default(),
            was_open: false,
        }
    }
}

impl Deref for Migrations {
    type Target = Wizard;
    fn deref(&self) -> &Wizard {
        &self.wizard
    }
}

impl DerefMut for Migrations {
    fn deref_mut(&mut self) -> &mut Wizard {
        &mut self.wizard
    }
}

impl Migrations {
    /// Open the wizard at step one, discarding whatever the last run left.
    pub fn open_dialog(&mut self) {
        self.open = true;
        self.reset();
    }

    /// Clear everything derived from a previous run, keeping the connection
    /// and schema choices so reopening after a failure is not a retype.
    pub fn reset(&mut self) {
        self.wizard.reset();
    }

    fn set_diff(&mut self, diff: SchemaDiff) {
        self.wizard.set_diff(diff);
        self.diff_grid.filter.clear();
    }

    fn busy(&self) -> bool {
        self.diff_job.is_running() || self.apply_job.is_running()
    }
}

// ===========================================================================
// Drawing
// ===========================================================================

/// What the footer asked for, acted on after the borrows end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    ComputeDiff,
    GenerateSql,
    Back,
    RequestApply,
    ConfirmApply,
    CancelApply,
    ResetSql,
    Close,
}

/// Draw the wizard as a centred modal window. Hosts that only flip
/// `screens.migrations.open` should call this.
pub fn show_window(app: &mut DbHiveApp, ctx: &egui::Context) {
    if !app.screens.migrations.open {
        app.screens.migrations.was_open = false;
        return;
    }
    if !app.screens.migrations.was_open {
        app.screens.migrations.reset();
        app.screens.migrations.was_open = true;
    }

    let mut open = true;
    egui::Window::new("Schema Migrations")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size(egui::vec2(880.0, 620.0))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| show(app, ui));
    if !open {
        app.screens.migrations.open = false;
    }
}

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    // Collected first so the state lock is never held while drawing.
    let (profiles, connected) = snapshot(app);

    poll_jobs(app);

    let palette = app.mode.palette();
    let mut act: Option<Act> = None;

    egui::TopBottomPanel::top("migrations_header")
        .frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(0.0, 6.0)))
        .show_inside(ui, |ui| {
            ui.label(crate::theme::text::heading(palette, "Schema Migrations"));
            ui.label(crate::theme::text::muted(
                palette,
                "Compare two connections and generate migration SQL to sync schemas.",
            ));
            ui.add_space(6.0);
            step_bar(ui, palette, app.screens.migrations.step);
        });

    egui::TopBottomPanel::bottom("migrations_footer")
        .frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(0.0, 8.0)))
        .show_inside(ui, |ui| {
            ui.separator();
            act = footer(app, ui, &profiles, &connected);
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::none())
        .show_inside(ui, |ui| {
            if let Some(err) = app.screens.migrations.last_error.clone() {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(palette.destructive, "Error:");
                    ui.label(err);
                    if ui.small_button("Dismiss").clicked() {
                        app.screens.migrations.last_error = None;
                    }
                });
                ui.add_space(6.0);
            }
            if let Some(outcome) = app.screens.migrations.outcome.clone() {
                outcome_banner(ui, palette, &outcome);
            }

            match app.screens.migrations.step {
                Step::Select => select_step(app, ui, &profiles, &connected),
                Step::Diff => diff_step(app, ui),
                Step::Sql => sql_step(app, ui, &profiles),
            }
        });

    if let Some(act) = act {
        dispatch(app, ui.ctx(), act);
    }

    // Drawn from here rather than from `show_window` so the confirmation is
    // present however this screen is hosted. It is a top-level area, not a
    // child of whatever `ui` belongs to, and sits in the foreground layer so
    // it cannot end up behind the wizard it is confirming.
    confirm_dialog(app, ui.ctx());
}

/// Profiles and the ids of the connections currently open.
fn snapshot(app: &DbHiveApp) -> (Vec<ConnectionProfile>, HashSet<String>) {
    let guard = app.state.lock().unwrap();
    let mut profiles: Vec<ConnectionProfile> =
        guard.list_profiles().into_iter().cloned().collect();
    profiles.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    let connected: HashSet<String> = guard.connections.keys().cloned().collect();
    (profiles, connected)
}

fn poll_jobs(app: &mut DbHiveApp) {
    if let Some(result) = app.screens.migrations.diff_job.take_result() {
        match result {
            Ok(diff) => {
                let empty = is_empty_diff(&diff);
                app.screens.migrations.set_diff(diff);
                app.screens.migrations.step = Step::Diff;
                app.screens.migrations.last_error = None;
                if empty {
                    app.toasts.info("Schemas are identical");
                }
            }
            Err(e) => {
                let msg = format!("Diff failed: {}", e);
                app.screens.migrations.last_error = Some(msg.clone());
                app.toasts.error(msg);
            }
        }
    }

    if let Some(result) = app.screens.migrations.apply_job.take_result() {
        match result {
            Ok(outcome) => {
                let summary = apply_summary(&outcome);
                if outcome.is_success() {
                    app.toasts.success(summary);
                } else {
                    app.toasts.error(summary);
                }
                app.screens.migrations.outcome = Some(outcome);
            }
            Err(e) => {
                let msg = format!("Apply failed: {}", e);
                app.screens.migrations.last_error = Some(msg.clone());
                app.toasts.error(msg);
            }
        }
    }
}

fn step_bar(ui: &mut egui::Ui, palette: &crate::theme::Palette, current: Step) {
    ui.horizontal(|ui| {
        for (i, step) in [Step::Select, Step::Diff, Step::Sql].into_iter().enumerate() {
            if i > 0 {
                ui.label(crate::theme::text::muted(palette, "›"));
            }
            let label = format!("{}. {}", i + 1, step.title());
            if step == current {
                ui.label(crate::theme::text::strong(palette, label));
            } else if step.index() < current.index() {
                ui.label(egui::RichText::new(label).color(palette.primary));
            } else {
                ui.label(crate::theme::text::muted(palette, label));
            }
        }
    });
}

fn outcome_banner(ui: &mut egui::Ui, palette: &crate::theme::Palette, outcome: &ApplyResult) {
    let colour = if outcome.is_success() {
        palette.success
    } else {
        palette.destructive
    };
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(colour, apply_summary(outcome));
    });
    if let Some(stmt) = &outcome.failed_statement {
        ui.label(crate::theme::text::muted(
            palette,
            format!("Failed on: {}", truncate(stmt, 160)),
        ));
    }
    ui.add_space(6.0);
}


// --- Step 1: select -------------------------------------------------------

fn select_step(
    app: &mut DbHiveApp,
    ui: &mut egui::Ui,
    profiles: &[ConnectionProfile],
    connected: &HashSet<String>,
) {
    let palette = app.mode.palette();

    if profiles.is_empty() {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(crate::theme::text::muted(
                palette,
                "No saved connections. Add two connections before migrating.",
            ));
        });
        return;
    }

    ui.add_space(8.0);
    ui.label(crate::theme::text::strong(palette, "Source (desired schema)"));
    profile_picker(
        ui,
        palette,
        "migrations_source",
        profiles,
        connected,
        &mut app.screens.migrations.source_id,
    );

    ui.add_space(10.0);
    ui.label(crate::theme::text::strong(palette, "Target (will be modified)"));
    profile_picker(
        ui,
        palette,
        "migrations_target",
        profiles,
        connected,
        &mut app.screens.migrations.target_id,
    );

    ui.add_space(10.0);
    ui.label(crate::theme::text::strong(palette, "Schema"));
    ui.add(
        egui::TextEdit::singleline(&mut app.screens.migrations.schema)
            .hint_text("public")
            .desired_width(360.0),
    );

    ui.add_space(10.0);
    ui.label(crate::theme::text::muted(
        palette,
        "Both connections must be open before running. Views are ignored: only base tables are diffed.",
    ));

    let source = profile_of(profiles, app.screens.migrations.source_id.as_deref());
    let target = profile_of(profiles, app.screens.migrations.target_id.as_deref());
    let offline = offline_selection(source, target, connected);
    if !offline.is_empty() {
        ui.add_space(4.0);
        ui.colored_label(
            palette.warning,
            format!("Not connected: {}", offline.join(", ")),
        );
    }
    if let (Some(s), Some(t)) = (source, target) {
        if s.driver != t.driver {
            ui.add_space(4.0);
            ui.colored_label(
                palette.warning,
                format!(
                    "{} and {} are different database types; generated SQL uses the target's dialect and may not translate cleanly.",
                    crate::screens::connections::driver_label(&s.driver),
                    crate::screens::connections::driver_label(&t.driver),
                ),
            );
        }
    }
}

fn profile_of<'a>(
    profiles: &'a [ConnectionProfile],
    id: Option<&str>,
) -> Option<&'a ConnectionProfile> {
    let id = id?;
    profiles.iter().find(|p| p.id == id)
}

fn profile_picker(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    id_salt: &str,
    profiles: &[ConnectionProfile],
    connected: &HashSet<String>,
    selected: &mut Option<String>,
) {
    let label = profile_of(profiles, selected.as_deref())
        .map(|p| {
            format!(
                "{} ({})",
                p.name,
                crate::screens::connections::driver_label(&p.driver)
            )
        })
        .unwrap_or_else(|| "Select connection".to_string());

    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(label)
        .width(360.0)
        .show_ui(ui, |ui| {
            for p in profiles {
                let is_selected = selected.as_deref() == Some(p.id.as_str());
                let mark = if connected.contains(&p.id) { "•" } else { "○" };
                let text = format!(
                    "{} {} ({})",
                    mark,
                    p.name,
                    crate::screens::connections::driver_label(&p.driver)
                );
                let colour = if connected.contains(&p.id) {
                    palette.success
                } else {
                    palette.muted_foreground
                };
                let response =
                    ui.selectable_label(is_selected, egui::RichText::new(text).color(colour));
                if response.clicked() {
                    *selected = Some(p.id.clone());
                }
            }
        });
}

// --- Step 2: diff ---------------------------------------------------------

fn diff_step(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    let Some(diff) = app.screens.migrations.diff.clone() else {
        ui.label(crate::theme::text::muted(palette, "No diff computed yet."));
        return;
    };

    if is_empty_diff(&diff) {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(crate::theme::text::strong(
                palette,
                "Schemas are identical. No migration needed.",
            ));
        });
        return;
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let view = &mut app.screens.migrations.diff_view;
        ui.selectable_value(view, DiffView::Tree, "Tree");
        ui.selectable_value(view, DiffView::Grid, "All changes");
        ui.separator();
        ui.label(crate::theme::text::muted(
            palette,
            format!(
                "{} added, {} removed, {} modified table(s)",
                diff.added_tables.len(),
                diff.removed_tables.len(),
                diff.modified_tables.len()
            ),
        ));
    });
    ui.add_space(6.0);

    match app.screens.migrations.diff_view {
        DiffView::Tree => {
            egui::ScrollArea::vertical()
                .id_salt("migrations_diff_tree")
                .auto_shrink([false, false])
                .show(ui, |ui| diff_tree(ui, palette, &diff));
        }
        DiffView::Grid => {
            if let Some(rows) = app.screens.migrations.diff_rows.clone() {
                results_grid::show(ui, palette, &rows, &mut app.screens.migrations.diff_grid);
            }
        }
    }
}

fn diff_tree(ui: &mut egui::Ui, palette: &crate::theme::Palette, diff: &SchemaDiff) {
    if !diff.added_tables.is_empty() {
        section(
            ui,
            "migrations_added",
            palette.success,
            format!("Added tables ({})", diff.added_tables.len()),
            |ui| {
                for t in &diff.added_tables {
                    ui.colored_label(
                        palette.success,
                        format!(
                            "+ {}.{} ({} columns)",
                            t.table.schema,
                            t.table.name,
                            t.columns.len()
                        ),
                    );
                }
            },
        );
    }

    if !diff.removed_tables.is_empty() {
        section(
            ui,
            "migrations_removed",
            palette.destructive,
            format!("Removed tables ({})", diff.removed_tables.len()),
            |ui| {
                for t in &diff.removed_tables {
                    ui.colored_label(
                        palette.destructive,
                        format!("- {}.{}", t.table.schema, t.table.name),
                    );
                }
            },
        );
    }

    if !diff.modified_tables.is_empty() {
        section(
            ui,
            "migrations_modified",
            palette.warning,
            format!("Modified tables ({})", diff.modified_tables.len()),
            |ui| {
                for td in &diff.modified_tables {
                    table_diff_row(ui, palette, td);
                }
            },
        );
    }
}

/// A coloured heading over an indented block, matching the web shell's
/// left-bordered sections. `Ui::indent` draws the rule itself, so the block
/// cannot end up with a line longer than its content.
fn section(
    ui: &mut egui::Ui,
    id_salt: &str,
    colour: egui::Color32,
    title: String,
    body: impl FnOnce(&mut egui::Ui),
) {
    ui.label(egui::RichText::new(title).strong().color(colour));
    ui.indent(id_salt, body);
    ui.add_space(8.0);
}

fn table_diff_row(ui: &mut egui::Ui, palette: &crate::theme::Palette, td: &TableDiff) {
    let header = format!(
        "{}.{}  ({} changes)",
        td.schema,
        td.name,
        change_count(td)
    );
    egui::CollapsingHeader::new(egui::RichText::new(header).color(palette.foreground))
        .id_salt(format!("migdiff:{}.{}", td.schema, td.name))
        .default_open(false)
        .show(ui, |ui| {
            let mono = |s: String, c: egui::Color32| egui::RichText::new(s).monospace().color(c);
            for c in &td.added_columns {
                ui.label(mono(
                    format!("+ column {} {}", c.name, c.data_type),
                    palette.success,
                ));
            }
            for c in &td.removed_columns {
                ui.label(mono(format!("- column {}", c.name), palette.destructive));
            }
            for c in &td.modified_columns {
                ui.label(mono(
                    format!(
                        "~ column {}: {} ➡ {}",
                        c.name, c.target.data_type, c.source.data_type
                    ),
                    palette.warning,
                ));
            }
            for i in &td.added_indexes {
                ui.label(mono(
                    format!("+ index {} ({})", i.name, i.columns.join(", ")),
                    palette.success,
                ));
            }
            for i in &td.removed_indexes {
                ui.label(mono(format!("- index {}", i.name), palette.destructive));
            }
            for f in &td.added_fks {
                ui.label(mono(
                    format!("+ fk {} ➡ {}", f.name, f.referenced_table),
                    palette.success,
                ));
            }
            for f in &td.removed_fks {
                ui.label(mono(format!("- fk {}", f.name), palette.destructive));
            }
        });
}

// --- Step 3: SQL ----------------------------------------------------------

fn sql_step(app: &mut DbHiveApp, ui: &mut egui::Ui, profiles: &[ConnectionProfile]) {
    let palette = app.mode.palette();
    let target_name = profile_of(profiles, app.screens.migrations.target_id.as_deref())
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "the target".to_string());

    let statements = split_statements(&app.screens.migrations.sql);
    let executable = executable_count(&statements);
    let edited = app.screens.migrations.sql != statements_to_text(&app.screens.migrations.generated);

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(crate::theme::text::muted(
            palette,
            format!(
                "{} statement(s) will be executed against {}.",
                executable, target_name
            ),
        ));
        if edited {
            ui.label(egui::RichText::new("edited").color(palette.warning).small());
        }
    });
    ui.add_space(4.0);

    // The editor is the standard widget; there is no read-only variant, so
    // edits are allowed and re-parsed on apply.
    let available = (ui.available_height() - 92.0).max(140.0);
    egui::Frame::none()
        .stroke(egui::Stroke::new(1.0_f32, palette.border))
        .inner_margin(egui::Margin::same(4.0))
        .show(ui, |ui| {
            ui.set_min_height(available);
            ui.set_max_height(available);
            let schema_index = sql_editor::SchemaIndex::default();
            // Named fields, not the Deref: the editor and the buffer live on
            // different structs and are borrowed mutably together.
            let m = &mut app.screens.migrations;
            let _ = sql_editor::show(
                ui,
                palette,
                &mut m.editor,
                &schema_index,
                &mut m.wizard.sql,
            );
        });

    ui.add_space(6.0);
    ui.checkbox(
        &mut app.screens.migrations.use_transaction,
        "Run inside a transaction",
    );
    ui.checkbox(
        &mut app.screens.migrations.acknowledged,
        "I understand this will modify the target database",
    );
}

// --- Footer and confirmation ---------------------------------------------

fn footer(
    app: &mut DbHiveApp,
    ui: &mut egui::Ui,
    profiles: &[ConnectionProfile],
    connected: &HashSet<String>,
) -> Option<Act> {
    let palette = app.mode.palette();
    let m = &app.screens.migrations;
    let busy = m.busy();
    let step = m.step;
    let mut act: Option<Act> = None;

    let blocked = match step {
        Step::Select => {
            let problem = selection_problem(
                m.source_id.as_deref(),
                m.target_id.as_deref(),
                &m.schema,
            );
            let offline = offline_selection(
                profile_of(profiles, m.source_id.as_deref()),
                profile_of(profiles, m.target_id.as_deref()),
                connected,
            );
            problem.map(|p| p.to_string()).or_else(|| {
                (!offline.is_empty())
                    .then(|| format!("Connect first: {}", offline.join(", ")))
            })
        }
        Step::Diff => match &m.diff {
            Some(d) if is_empty_diff(d) => Some("Nothing to generate".to_string()),
            Some(_) => None,
            None => Some("No diff".to_string()),
        },
        Step::Sql => {
            if !m.acknowledged {
                Some("Confirm the checkbox to enable Apply".to_string())
            } else if executable_count(&split_statements(&m.sql)) == 0 {
                Some("No executable statements".to_string())
            } else {
                None
            }
        }
    };

    ui.horizontal(|ui| {
        if busy {
            ui.spinner();
        }
        if let Some(reason) = &blocked {
            ui.label(crate::theme::text::muted(palette, reason.clone()));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            match step {
                Step::Select => {
                    let btn = ui.add_enabled(
                        !busy && blocked.is_none(),
                        egui::Button::new("Compute Diff"),
                    );
                    if btn.clicked() {
                        act = Some(Act::ComputeDiff);
                    }
                    if ui.button("Cancel").clicked() {
                        act = Some(Act::Close);
                    }
                }
                Step::Diff => {
                    let btn = ui
                        .add_enabled(!busy && blocked.is_none(), egui::Button::new("Generate SQL"));
                    if btn.clicked() {
                        act = Some(Act::GenerateSql);
                    }
                    if ui.button("Back").clicked() {
                        act = Some(Act::Back);
                    }
                }
                Step::Sql => {
                    let apply = ui.add_enabled(
                        !busy && blocked.is_none(),
                        egui::Button::new(
                            egui::RichText::new("Apply Migration")
                                .color(palette.destructive)
                                .strong(),
                        ),
                    );
                    if apply.clicked() {
                        act = Some(Act::RequestApply);
                    }
                    if ui.button("Back").clicked() {
                        act = Some(Act::Back);
                    }
                    if ui
                        .add_enabled(!busy, egui::Button::new("Reset to generated"))
                        .clicked()
                    {
                        act = Some(Act::ResetSql);
                    }
                }
            }
        });
    });

    act
}

/// The second gate on the only destructive operation here.
fn confirm_dialog(app: &mut DbHiveApp, ctx: &egui::Context) {
    if !app.screens.migrations.confirming {
        return;
    }
    let palette = app.mode.palette();
    let (profiles, _) = snapshot(app);
    let target_name = profile_of(profiles.as_slice(), app.screens.migrations.target_id.as_deref())
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "the target".to_string());
    let statements = split_statements(&app.screens.migrations.sql);
    let count = executable_count(&statements);
    let use_transaction = app.screens.migrations.use_transaction;

    let mut act: Option<Act> = None;
    let mut open = true;
    egui::Window::new("Apply migration?")
        .open(&mut open)
        .order(egui::Order::Foreground)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(format!(
                "{} statement(s) will run against {}.",
                count, target_name
            ))
            .color(palette.foreground));
            ui.add_space(4.0);
            ui.label(crate::theme::text::muted(
                palette,
                if use_transaction {
                    "Wrapped in a transaction: the first failure rolls the whole migration back."
                } else {
                    "No transaction: a failure part-way leaves earlier statements applied."
                },
            ));
            ui.add_space(4.0);
            ui.colored_label(palette.destructive, "This cannot be undone.");
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    act = Some(Act::CancelApply);
                }
                if ui
                    .button(
                        egui::RichText::new(format!("Apply {} statement(s)", count))
                            .color(palette.destructive)
                            .strong(),
                    )
                    .clicked()
                {
                    act = Some(Act::ConfirmApply);
                }
            });
        });
    if !open {
        act = Some(Act::CancelApply);
    }

    if let Some(act) = act {
        dispatch(app, ctx, act);
    }
}

fn dispatch(app: &mut DbHiveApp, ctx: &egui::Context, act: Act) {
    match act {
        Act::Close => {
            app.screens.migrations.open = false;
        }
        Act::Back => {
            app.screens.migrations.step = match app.screens.migrations.step {
                Step::Sql => Step::Diff,
                _ => Step::Select,
            };
            app.screens.migrations.outcome = None;
        }
        Act::ComputeDiff => {
            let (Some(source), Some(target)) = (
                app.screens.migrations.source_id.clone(),
                app.screens.migrations.target_id.clone(),
            ) else {
                return;
            };
            let schema = app.screens.migrations.schema.trim().to_string();
            let state = app.state.clone();
            let handle = app.rt.handle().clone();
            app.screens.migrations.last_error = None;
            app.screens.migrations.outcome = None;
            app.screens
                .migrations
                .diff_job
                .start(&handle, ctx, async move {
                    dbhive_core::service_migrations::compute_schema_diff(
                        &state, &source, &target, &schema,
                    )
                    .await
                });
        }
        Act::GenerateSql => {
            let Some(diff) = app.screens.migrations.diff.clone() else {
                return;
            };
            let Some(target) = app.screens.migrations.target_id.clone() else {
                return;
            };
            // Pure string work once the target's dialect is read out of
            // state, so it runs inline rather than through a Job.
            match dbhive_core::service_migrations::generate_migration(&app.state, &diff, &target) {
                Ok(statements) => {
                    app.screens.migrations.sql = statements_to_text(&statements);
                    app.screens.migrations.generated = statements;
                    app.screens.migrations.acknowledged = false;
                    app.screens.migrations.outcome = None;
                    app.screens.migrations.last_error = None;
                    app.screens.migrations.step = Step::Sql;
                }
                Err(e) => {
                    let msg = format!("Generate failed: {}", e);
                    app.screens.migrations.last_error = Some(msg.clone());
                    app.toasts.error(msg);
                }
            }
        }
        Act::ResetSql => {
            let text = statements_to_text(&app.screens.migrations.generated);
            app.screens.migrations.sql = text;
        }
        Act::RequestApply => {
            app.screens.migrations.confirming = true;
        }
        Act::CancelApply => {
            app.screens.migrations.confirming = false;
        }
        Act::ConfirmApply => {
            app.screens.migrations.confirming = false;
            let Some(target) = app.screens.migrations.target_id.clone() else {
                return;
            };
            let statements = split_statements(&app.screens.migrations.sql);
            if statements.is_empty() {
                return;
            }
            let use_transaction = app.screens.migrations.use_transaction;
            let state = app.state.clone();
            let handle = app.rt.handle().clone();
            app.screens.migrations.outcome = None;
            app.screens.migrations.last_error = None;
            app.screens
                .migrations
                .apply_job
                .start(&handle, ctx, async move {
                    dbhive_core::service_migrations::apply_migration(
                        &state,
                        &target,
                        &statements,
                        use_transaction,
                    )
                    .await
                });
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================
//
// The logic tests live with the logic in `dbhive_ui_model::migrations`.
// What is left here is the egui wrapper's delegation.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_reset_delegates_to_the_wizard_and_keeps_choices() {
        let mut m = Migrations::default();
        assert_eq!(m.schema, "public");
        m.source_id = Some("1".into());
        m.step = Step::Sql;
        m.sql = "DROP TABLE t;".into();
        m.confirming = true;

        m.open_dialog();

        assert!(m.open);
        assert_eq!(m.step, Step::Select);
        assert!(m.sql.is_empty());
        assert!(!m.confirming);
        assert_eq!(m.source_id.as_deref(), Some("1"));
    }
}
