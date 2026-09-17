//! Backup manager: create, list, restore and delete database backups.
//!
//! Parity target: the React `BackupManagerDialog.tsx` (2.10 in
//! docs/rust-ui-migration/01-frontend-inventory.md).
//!
//! All the work lives in `dbhive_core::service_backup`; this file is the
//! shell around it. Three things shape the layout:
//!
//! * **Progress.** The Tauri shell had none: `create_backup` returned when
//!   `pg_dump` did, and the UI showed a button spinner for however long that
//!   took. egui has no event bus to emit to, so the service instead reports
//!   stages down an mpsc channel. A forwarder task drains it into a shared
//!   slot and calls `ctx.request_repaint()`, so a new stage is drawn the
//!   moment it arrives rather than whenever the user next moves the mouse.
//!   See `progress_channel` below.
//! * **No file picker.** There is no `rfd` in this workspace, so the output
//!   directory and any table list are typed, validated before anything is
//!   spawned, and the resolved directory is always shown.
//! * **Destructive actions are confirmed by name.** Restore and delete both
//!   go through a modal that names the exact file, and restore also names
//!   the database it will overwrite.
//!
//! The pure logic (formatting, the create form and its `BackupOptions`
//! mapping, profile annotation, the grid projection) lives in
//! `dbhive_ui_model::backup`, with its tests. This file keeps the jobs, the
//! progress plumbing and the drawing.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dbhive_core::models::backup::{
    BackupEntry, BackupOptions, BackupProgress, BackupStatus, RestoreOptions,
};
use dbhive_core::models::{DbDriver, DbError};
use dbhive_core::service_backup as svc;

pub use dbhive_ui_model::backup::*;

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::theme::{text, Palette};
use crate::widgets::results_grid;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Result of the listing job: the directory that was read, and its contents.
type Listing = Result<(String, Vec<BackupEntry>), DbError>;

pub struct Backup {
    pub open: bool,

    /// Directory the last successful listing came from.
    directory: String,
    entries: Vec<BackupEntry>,
    grid: results_grid::GridState,
    requested: bool,

    /// The create form. The output directory is typed; there is no file
    /// picker crate in this workspace beyond `dir_picker`.
    form: BackupForm,
    dir_picker: crate::widgets::file_dialog::PathPicker,
    show_advanced: bool,

    list_job: Job<Listing>,
    create_job: Job<Result<BackupEntry, DbError>>,
    /// Carries the file name so the toast can name what was restored.
    restore_job: Job<(String, Result<(), DbError>)>,
    delete_job: Job<(String, Result<(), DbError>)>,

    /// Latest stage from the running operation. Written by a forwarder task,
    /// read once per frame.
    progress: Arc<Mutex<Option<BackupProgress>>>,
    started: Option<Instant>,
    last_elapsed: Option<Duration>,

    /// Backup awaiting restore confirmation, and the drop toggle for it.
    confirm_restore: Option<BackupEntry>,
    drop_existing: bool,
    /// Backup awaiting delete confirmation.
    confirm_delete: Option<BackupEntry>,

    last_error: Option<String>,
}

impl Default for Backup {
    fn default() -> Self {
        Self {
            open: false,
            directory: String::new(),
            entries: Vec::new(),
            grid: results_grid::GridState::default(),
            requested: false,
            form: BackupForm::default(),
            dir_picker: Default::default(),
            show_advanced: false,
            list_job: Job::default(),
            create_job: Job::default(),
            restore_job: Job::default(),
            delete_job: Job::default(),
            progress: Arc::new(Mutex::new(None)),
            started: None,
            last_elapsed: None,
            confirm_restore: None,
            drop_existing: false,
            confirm_delete: None,
            last_error: None,
        }
    }
}

impl Backup {
    fn busy(&self) -> bool {
        self.create_job.is_running() || self.restore_job.is_running() || self.delete_job.is_running()
    }

    /// The options the create form currently describes.
    fn options(&self) -> BackupOptions {
        self.form.options()
    }
}

/// What a widget asked for this frame, acted on after the borrows end.
enum Action {
    Refresh,
    OpenFolder,
    Create,
    AskRestore(usize),
    AskDelete(usize),
    CancelRestore,
    CancelDelete,
    ConfirmRestore,
    ConfirmDelete,
}

// ---------------------------------------------------------------------------
// Async plumbing
// ---------------------------------------------------------------------------

/// Wire a progress channel into a slot the UI reads each frame.
///
/// The Tauri shell reported progress by emitting an event to the webview.
/// egui has no event bus and no way to push a value into the frame loop, so
/// the service sends `BackupProgress` down an unbounded mpsc and a tiny
/// forwarder task moves each one into an `Arc<Mutex<Option<_>>>`, then calls
/// `ctx.request_repaint()`.
///
/// The repaint is the whole point: egui only redraws on activity, so without
/// it "Running pg_dump…" would sit on screen unchanged until the user
/// happened to move the mouse. The alternative (polling the receiver from
/// `show`) needs a timer-driven repaint anyway and adds latency for nothing.
/// A single slot rather than a queue is deliberate: the UI only ever draws
/// the newest stage, so an older one being overwritten between frames costs
/// nothing.
fn progress_channel(
    rt: &tokio::runtime::Handle,
    ctx: &egui::Context,
    slot: Arc<Mutex<Option<BackupProgress>>>,
) -> svc::ProgressSink {
    // A stale "done" from the previous operation must not be drawn as the
    // opening stage of this one.
    *slot.lock().unwrap() = None;
    let (sink, mut rx) = svc::ProgressSink::channel();
    let ctx = ctx.clone();
    rt.spawn(async move {
        while let Some(p) = rx.recv().await {
            *slot.lock().unwrap() = Some(p);
            ctx.request_repaint();
        }
    });
    sink
}

/// Every saved profile as (id, name, driver label), for annotating listings.
fn profile_refs(app: &DbHiveApp) -> Vec<(String, String, String)> {
    let guard = app.state.lock().unwrap();
    guard
        .list_profiles()
        .into_iter()
        .map(|p| (p.id.clone(), p.name.clone(), format!("{:?}", p.driver)))
        .collect()
}

/// The connection a backup would be taken from: the connected workspace,
/// exactly as the web shell did (there is no connection picker).
fn current_target(app: &DbHiveApp) -> Option<(String, String, DbDriver)> {
    let connection_id = app.workspace.as_ref()?.connection_id.clone();
    let guard = app.state.lock().unwrap();
    let p = guard.get_profile(&connection_id)?;
    Some((p.id.clone(), p.name.clone(), p.driver.clone()))
}

/// Start (or restart) the directory listing.
fn refresh(app: &mut DbHiveApp, ctx: &egui::Context) {
    let dir = optional(&app.screens.backup.form.output_dir);
    let profiles = profile_refs(app);
    let handle = app.rt.handle().clone();
    app.screens.backup.list_job.start(&handle, ctx, async move {
        // The listing stats every file, so it goes on a blocking thread
        // rather than parking a runtime worker.
        tokio::task::spawn_blocking(move || {
            let resolved = match dir.clone() {
                Some(d) => d,
                None => svc::get_backup_directory()?,
            };
            let mut entries = svc::list_backups(Some(&resolved))?;
            annotate_entries(&mut entries, &profiles);
            Ok((resolved, entries))
        })
        .await
        .unwrap_or_else(|e| Err(DbError::InternalError(format!("Listing failed: {}", e))))
    });
}

/// Poll every job and fold the results into screen state.
fn poll(app: &mut DbHiveApp) {
    if let Some(result) = app.screens.backup.list_job.take_result() {
        match result {
            Ok((dir, entries)) => {
                let b = &mut app.screens.backup;
                b.directory = dir;
                b.entries = entries;
                b.grid.selected_row = None;
                b.last_error = None;
            }
            Err(e) => {
                let msg = format!("Failed to load backups: {}", e);
                app.screens.backup.last_error = Some(msg.clone());
                app.toasts.error(msg);
            }
        }
    }

    if let Some(result) = app.screens.backup.create_job.take_result() {
        finish_timing(app);
        match result {
            Ok(entry) => {
                match &entry.status {
                    BackupStatus::Failed(reason) => {
                        app.toasts.error(format!("Backup failed: {}", reason))
                    }
                    BackupStatus::Completed => app
                        .toasts
                        .success(format!("Backup created: {}", entry.file_name)),
                }
                app.screens.backup.form.note.clear();
                // Clearing `requested` makes the next frame re-list, so the
                // new file appears without the user pressing Refresh.
                app.screens.backup.requested = false;
            }
            Err(e) => {
                let msg = format!("Backup failed: {}", e);
                app.screens.backup.last_error = Some(msg.clone());
                app.toasts.error(msg);
            }
        }
    }

    if let Some((file_name, result)) = app.screens.backup.restore_job.take_result() {
        finish_timing(app);
        match result {
            Ok(()) => app.toasts.success(format!("Restored from {}", file_name)),
            Err(e) => {
                let msg = format!("Restore failed: {}", e);
                app.screens.backup.last_error = Some(msg.clone());
                app.toasts.error(msg);
            }
        }
    }

    if let Some((file_name, result)) = app.screens.backup.delete_job.take_result() {
        finish_timing(app);
        match result {
            Ok(()) => {
                app.toasts.success(format!("Deleted {}", file_name));
                app.screens.backup.requested = false;
            }
            Err(e) => {
                let msg = format!("Delete failed: {}", e);
                app.screens.backup.last_error = Some(msg.clone());
                app.toasts.error(msg);
            }
        }
    }
}

fn finish_timing(app: &mut DbHiveApp) {
    let b = &mut app.screens.backup;
    b.last_elapsed = b.started.take().map(|t| t.elapsed());
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Draw the backup manager as an overlay dialog when `open` is set.
///
/// This is how the web shell presented it (a modal over the workspace), and
/// how `app.rs` should call it.
pub fn show_window(app: &mut DbHiveApp, ctx: &egui::Context) {
    if !app.screens.backup.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Backup Manager")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size(egui::vec2(900.0, 640.0))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            show(app, ui);
        });
    if !open {
        app.screens.backup.open = false;
    }
}

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    poll(app);

    // First draw loads the directory and its contents.
    if !app.screens.backup.requested {
        app.screens.backup.requested = true;
        refresh(app, ui.ctx());
    }

    let palette = app.mode.palette();
    let target = current_target(app);
    let mut actions: Vec<Action> = Vec::new();

    header(app, ui, palette, &mut actions);
    ui.add_space(8.0);
    create_panel(app, ui, palette, target.as_ref(), &mut actions);
    ui.add_space(8.0);
    list_panel(app, ui, palette, target.is_some(), &mut actions);

    if app.screens.backup.confirm_restore.is_some() {
        restore_dialog(app, ui.ctx(), palette, target.as_ref(), &mut actions);
    }
    if app.screens.backup.confirm_delete.is_some() {
        delete_dialog(app, ui.ctx(), palette, &mut actions);
    }

    for action in actions {
        apply(app, ui.ctx(), action, target.as_ref());
    }
}

fn apply(
    app: &mut DbHiveApp,
    ctx: &egui::Context,
    action: Action,
    target: Option<&(String, String, DbDriver)>,
) {
    match action {
        Action::Refresh => refresh(app, ctx),

        Action::OpenFolder => {
            let dir = app.screens.backup.directory.clone();
            if let Err(e) = dbhive_core::service_backup::open_directory(&dir) {
                app.toasts.error(format!("Failed to open folder: {}", e));
            }
        }

        Action::Create => {
            let Some((connection_id, _, _)) = target.cloned() else {
                app.toasts.info("Connect to a database first");
                return;
            };
            let options = app.screens.backup.options();
            let state = app.state.clone();
            let handle = app.rt.handle().clone();
            let sink = progress_channel(&handle, ctx, app.screens.backup.progress.clone());
            app.screens.backup.last_error = None;
            app.screens.backup.started = Some(Instant::now());
            app.screens.backup.last_elapsed = None;
            app.screens.backup.create_job.start(&handle, ctx, async move {
                svc::create_backup(&state, &connection_id, options, sink).await
            });
        }

        Action::AskRestore(index) => {
            app.screens.backup.drop_existing = false;
            app.screens.backup.confirm_restore = app.screens.backup.entries.get(index).cloned();
        }
        Action::CancelRestore => {
            app.screens.backup.confirm_restore = None;
            app.screens.backup.drop_existing = false;
        }
        Action::ConfirmRestore => {
            let Some(entry) = app.screens.backup.confirm_restore.take() else {
                return;
            };
            let Some((connection_id, _, _)) = target.cloned() else {
                app.toasts.info("Connect to a database first");
                return;
            };
            let options = RestoreOptions {
                file_path: entry.file_path.clone(),
                drop_existing: app.screens.backup.drop_existing,
            };
            let file_name = entry.file_name.clone();
            let state = app.state.clone();
            let handle = app.rt.handle().clone();
            let sink = progress_channel(&handle, ctx, app.screens.backup.progress.clone());
            app.screens.backup.last_error = None;
            app.screens.backup.started = Some(Instant::now());
            app.screens.backup.last_elapsed = None;
            app.screens.backup.restore_job.start(&handle, ctx, async move {
                let r = svc::restore_backup(&state, &connection_id, options, sink).await;
                (file_name, r)
            });
        }

        Action::AskDelete(index) => {
            app.screens.backup.confirm_delete = app.screens.backup.entries.get(index).cloned();
        }
        Action::CancelDelete => app.screens.backup.confirm_delete = None,
        Action::ConfirmDelete => {
            let Some(entry) = app.screens.backup.confirm_delete.take() else {
                return;
            };
            let path = entry.file_path.clone();
            let file_name = entry.file_name.clone();
            let handle = app.rt.handle().clone();
            app.screens.backup.last_error = None;
            app.screens.backup.started = Some(Instant::now());
            app.screens.backup.delete_job.start(&handle, ctx, async move {
                let r = tokio::task::spawn_blocking(move || svc::delete_backup(&path))
                    .await
                    .unwrap_or_else(|e| {
                        Err(DbError::InternalError(format!("Delete task failed: {}", e)))
                    });
                (file_name, r)
            });
            // The listing is refreshed once the delete lands, not now: the
            // file is still on disk until then.
        }
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

fn header(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette, actions: &mut Vec<Action>) {
    let b = &app.screens.backup;
    let listing = b.list_job.is_running();
    let directory = b.directory.clone();

    ui.horizontal(|ui| {
        ui.label(text::heading(palette, "Backup Manager"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!listing, egui::Button::new("Refresh"))
                .clicked()
            {
                actions.push(Action::Refresh);
            }
            if ui
                .add_enabled(!directory.is_empty(), egui::Button::new("Open Folder"))
                .on_hover_text("Reveal this directory in the file manager")
                .clicked()
            {
                actions.push(Action::OpenFolder);
            }
            if listing {
                ui.add(egui::Spinner::new().size(14.0));
            }
        });
    });

    ui.label(text::muted(
        palette,
        if directory.is_empty() {
            "Loading backup directory…".to_string()
        } else {
            format!("Backups stored in: {}", directory)
        },
    ));

    if let Some(err) = &app.screens.backup.last_error {
        ui.colored_label(palette.destructive, err);
    }
}

fn create_panel(
    app: &mut DbHiveApp,
    ui: &mut egui::Ui,
    palette: &Palette,
    target: Option<&(String, String, DbDriver)>,
    actions: &mut Vec<Action>,
) {
    // Captured before the screen state is borrowed, so the picker can
    // spawn on the runtime without re-borrowing `app`.
    let rt = app.rt.handle().clone();
    let busy = app.screens.backup.busy();
    let creating = app.screens.backup.create_job.is_running();

    card(ui, palette, "New Backup", |ui| {
        match target {
            Some((_, name, driver)) => {
                ui.label(text::muted(
                    palette,
                    format!("{} ({:?}): via {}", name, driver, svc::backup_tool(driver)),
                ));
            }
            None => {
                ui.label(text::muted(
                    palette,
                    "Connect to a database to create a backup.",
                ));
            }
        }
        ui.add_space(6.0);

        let enabled = target.is_some() && !busy;
        let b = &mut app.screens.backup;

        ui.add_enabled_ui(enabled, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut b.form.include_schema, "Include Schema");
                ui.add_space(16.0);
                ui.checkbox(&mut b.form.include_data, "Include Data");
            });

            ui.horizontal(|ui| {
                ui.label("Note");
                ui.add(
                    egui::TextEdit::singleline(&mut b.form.note)
                        .hint_text("e.g. before risky migration")
                        .desired_width(320.0),
                );
            });

            let advanced = b.show_advanced;
            if ui
                .small_button(if advanced {
                    "Hide advanced options"
                } else {
                    "Advanced options"
                })
                .clicked()
            {
                b.show_advanced = !advanced;
            }

            if b.show_advanced {
                ui.add_space(4.0);
                ui.label(text::muted(
                    palette,
                    "Tables (comma separated; blank backs up everything)",
                ));
                ui.add(
                    egui::TextEdit::multiline(&mut b.form.tables)
                        .hint_text("orders, customers")
                        .desired_rows(2)
                        .desired_width(f32::INFINITY),
                );
                // No file dialog crate is available in this workspace, so
                // the destination is typed and validated instead of picked.
                ui.label(text::muted(
                    palette,
                    "Output directory (absolute path; blank uses the default above)",
                ));
                crate::widgets::file_dialog::path_field(
                    ui,
                    &rt,
                    &mut b.dir_picker,
                    &mut b.form.output_dir,
                    "/Users/me/db-backups",
                    || crate::widgets::file_dialog::Pick::Folder {
                        title: "Backup directory".into(),
                    },
                );
            }
        });

        // Validation runs through the same function the service uses, so the
        // inline hint and the eventual error cannot disagree.
        let options = app.screens.backup.options();
        let invalid = target
            .map(|(_, _, driver)| svc::validate_backup_options(driver, &options).err())
            .unwrap_or(None);

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let can_create = target.is_some() && !busy && invalid.is_none();
            let label = if creating {
                "Creating backup…"
            } else {
                "Create Backup"
            };
            if ui
                .add_enabled(can_create, egui::Button::new(label))
                .clicked()
            {
                actions.push(Action::Create);
            }
            if creating {
                ui.add(egui::Spinner::new().size(14.0));
            }
            if let Some(err) = &invalid {
                ui.colored_label(palette.warning, err.to_string());
            }
        });

        progress_line(app, ui, palette);
    });
}

/// The stage of the running operation, or how long the last one took.
fn progress_line(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let busy = app.screens.backup.busy();
    let current = app.screens.backup.progress.lock().unwrap().clone();

    if busy {
        let elapsed = app
            .screens
            .backup
            .started
            .map(|t| format_duration(t.elapsed()))
            .unwrap_or_default();
        let stage = current
            .as_ref()
            .map(|p| p.message.clone())
            .unwrap_or_else(|| "Starting…".to_string());
        ui.horizontal(|ui| {
            // Indeterminate: the dump tools report no percentage, so the bar
            // animates rather than lying about how far along it is. Animating
            // it also keeps the frame loop running while the elapsed time
            // ticks up.
            ui.add(
                egui::ProgressBar::new(0.0)
                    .animate(true)
                    .desired_width(160.0),
            );
            ui.label(text::muted(palette, format!("{} ({})", stage, elapsed)));
        });
        return;
    }

    if let Some(p) = current.as_ref().filter(|p| p.done) {
        let elapsed = app
            .screens
            .backup
            .last_elapsed
            .map(|d| format!(" in {}", format_duration(d)))
            .unwrap_or_default();
        match &p.error {
            Some(err) => {
                ui.colored_label(palette.destructive, format!("{}{}", err, elapsed));
            }
            None => {
                ui.colored_label(palette.success, format!("{}{}", p.message, elapsed));
            }
        }
    }
}

fn list_panel(
    app: &mut DbHiveApp,
    ui: &mut egui::Ui,
    palette: &Palette,
    connected: bool,
    actions: &mut Vec<Action>,
) {
    let listing = app.screens.backup.list_job.is_running();
    let busy = app.screens.backup.busy();

    if app.screens.backup.entries.is_empty() {
        ui.label(text::muted(
            palette,
            if listing {
                "Loading backups…"
            } else {
                "No backups yet"
            },
        ));
        return;
    }

    // The grid returns the index of the row in the original slice, so it maps
    // straight back to `entries` regardless of sorting or filtering.
    let result = entries_to_result(&app.screens.backup.entries);
    let b = &mut app.screens.backup;
    // The grid scrolls internally and would otherwise take every remaining
    // pixel, pushing the action row out of the dialog. Reserve its height.
    let grid_height = (ui.available_height() - 44.0).max(120.0);
    ui.allocate_ui(egui::vec2(ui.available_width(), grid_height), |ui| {
        results_grid::show(ui, palette, &result, &mut b.grid);
    });

    let selected = b.grid.selected_row;
    let selected_entry = selected.and_then(|i| b.entries.get(i)).cloned();

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        match &selected_entry {
            Some(entry) => {
                let failed = matches!(entry.status, BackupStatus::Failed(_));
                let can_restore = connected && !failed && !busy;

                let restore = ui.add_enabled(can_restore, egui::Button::new("Restore…"));
                let restore = if !connected {
                    restore.on_disabled_hover_text("Connect to a database to restore")
                } else if failed {
                    restore.on_disabled_hover_text("Cannot restore a failed backup")
                } else {
                    restore
                };
                if restore.clicked() {
                    if let Some(i) = selected {
                        actions.push(Action::AskRestore(i));
                    }
                }

                if ui
                    .add_enabled(
                        !busy,
                        egui::Button::new(
                            egui::RichText::new("Delete…").color(palette.destructive),
                        ),
                    )
                    .clicked()
                {
                    if let Some(i) = selected {
                        actions.push(Action::AskDelete(i));
                    }
                }

                ui.label(text::muted(palette, format!("Selected: {}", entry.file_name)));
            }
            None => {
                ui.label(text::muted(
                    palette,
                    "Select a backup to restore or delete it.",
                ));
            }
        }
    });
}

fn restore_dialog(
    app: &mut DbHiveApp,
    ctx: &egui::Context,
    palette: &Palette,
    target: Option<&(String, String, DbDriver)>,
    actions: &mut Vec<Action>,
) {
    let Some(entry) = app.screens.backup.confirm_restore.clone() else {
        return;
    };
    let restoring = app.screens.backup.restore_job.is_running();
    let driver = target.map(|(_, _, d)| d.clone());
    let db_label = target
        .map(|(_, name, _)| name.clone())
        .unwrap_or_else(|| "the connected database".to_string());

    let mut open = true;
    egui::Window::new("Restore backup?")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            // Name the file and the target: a confirmation that says only
            // "are you sure?" is not a confirmation.
            ui.label(text::strong(palette, &entry.file_name));
            ui.label(text::muted(
                palette,
                format!(
                    "will be restored into {}. This overwrites data in that database and \
                     cannot be undone.",
                    db_label
                ),
            ));
            ui.add_space(8.0);

            // SQLite replaces the whole file, so the toggle has no meaning.
            let drop_applies = drop_existing_applies(driver.as_ref());
            ui.add_enabled_ui(!restoring && drop_applies, |ui| {
                ui.checkbox(
                    &mut app.screens.backup.drop_existing,
                    "Drop & recreate database first",
                );
            });
            ui.horizontal(|ui| {
                ui.add_space(22.0);
                ui.label(text::muted(
                    palette,
                    if drop_applies {
                        "Destroys every existing object in the target database before \
                         restoring (Postgres / MySQL)."
                    } else {
                        "Not applicable: restoring SQLite replaces the database file."
                    },
                ));
            });

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!restoring, egui::Button::new("Cancel"))
                    .clicked()
                {
                    actions.push(Action::CancelRestore);
                }
                let label = restore_button_label(app.screens.backup.drop_existing, &db_label);
                if ui
                    .add_enabled(
                        !restoring,
                        egui::Button::new(egui::RichText::new(label).color(palette.destructive)),
                    )
                    .clicked()
                {
                    actions.push(Action::ConfirmRestore);
                }
                if restoring {
                    ui.add(egui::Spinner::new().size(14.0));
                }
            });
        });

    if !open && !restoring {
        actions.push(Action::CancelRestore);
    }
}

fn delete_dialog(
    app: &mut DbHiveApp,
    ctx: &egui::Context,
    palette: &Palette,
    actions: &mut Vec<Action>,
) {
    let Some(entry) = app.screens.backup.confirm_delete.clone() else {
        return;
    };
    let deleting = app.screens.backup.delete_job.is_running();

    let mut open = true;
    egui::Window::new("Delete backup?")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label(text::strong(palette, &entry.file_name));
            ui.label(text::muted(
                palette,
                format!(
                    "{} will be permanently deleted from disk. This cannot be undone.",
                    entry.file_path
                ),
            ));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!deleting, egui::Button::new("Cancel"))
                    .clicked()
                {
                    actions.push(Action::CancelDelete);
                }
                if ui
                    .add_enabled(
                        !deleting,
                        egui::Button::new(
                            egui::RichText::new(format!("Delete {}", entry.file_name))
                                .color(palette.destructive),
                        ),
                    )
                    .clicked()
                {
                    actions.push(Action::ConfirmDelete);
                }
                if deleting {
                    ui.add(egui::Spinner::new().size(14.0));
                }
            });
        });

    if !open && !deleting {
        actions.push(Action::CancelDelete);
    }
}

/// One titled card, matching the grouping the web shell used.
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


// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_screen_is_closed_and_backs_up_everything() {
        let b = Backup::default();
        assert!(!b.open);
        assert!(!b.busy());
        assert!(b.form.include_schema && b.form.include_data);
        assert!(b.confirm_restore.is_none() && b.confirm_delete.is_none());
    }

    #[test]
    fn the_screen_maps_its_form_onto_backup_options() {
        let mut b = Backup::default();
        b.form.tables = "orders,customers".into();
        b.form.note = " n ".into();
        let (o, f) = (b.options(), b.form.options());
        assert_eq!(o.tables, vec!["orders", "customers"]);
        assert_eq!(o.tables, f.tables);
        assert_eq!(o.note.as_deref(), Some("n"));
        assert_eq!(o.note, f.note);
        assert_eq!(o.output_dir, f.output_dir);
        assert_eq!((o.include_schema, o.include_data), (f.include_schema, f.include_data));
    }
}
