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

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dbhive_core::drivers::QueryResult;
use dbhive_core::models::backup::{
    BackupEntry, BackupOptions, BackupProgress, BackupStatus, RestoreOptions,
};
use dbhive_core::models::{DbDriver, DbError};
use dbhive_core::service_backup as svc;

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::theme::{text, Palette};
use crate::widgets::results_grid;

// ---------------------------------------------------------------------------
// Pure logic: formatting, form parsing, grid projection
// ---------------------------------------------------------------------------

/// Human-readable byte count, matching the web shell's `formatBytes`:
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
        return "—".to_string();
    }
    match chrono::DateTime::from_timestamp(unix_secs, 0) {
        Some(dt) => dt
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        None => "—".to_string(),
    }
}

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
fn optional(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// Status as displayed, plus the failure reason if there is one.
pub fn status_label(status: &BackupStatus) -> (&'static str, Option<&str>) {
    match status {
        BackupStatus::Completed => ("Completed", None),
        BackupStatus::Failed(reason) => ("Failed", Some(reason.as_str())),
    }
}

/// Fill in the connection and driver of listed backups from the saved
/// profiles.
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
        let mut matches = profiles.iter().filter(|(_, name, _)| svc::safe_name(name) == key);
        if let (Some((id, name, driver)), None) = (matches.next(), matches.next()) {
            e.connection_id = id.clone();
            e.connection_name = name.clone();
            e.driver = driver.clone();
        }
    }
}

/// Project the backup list into the shape `widgets::results_grid` draws.
///
/// The grid is the only table in this app, so the list goes through it
/// rather than growing a second one; it also brings sorting and filtering
/// for free, which the web shell's table had.
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

    // Create form
    include_schema: bool,
    include_data: bool,
    note: String,
    /// Comma or newline separated; empty means every table.
    tables: String,
    /// Typed absolute path; empty means the default backup directory. There
    /// is no file picker crate in this workspace.
    output_dir: String,
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
            // Both on: the default backup is a complete one.
            include_schema: true,
            include_data: true,
            note: String::new(),
            tables: String::new(),
            output_dir: String::new(),
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
        BackupOptions {
            include_data: self.include_data,
            include_schema: self.include_schema,
            tables: parse_tables(&self.tables),
            note: optional(&self.note),
            output_dir: optional(&self.output_dir),
        }
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
    let dir = optional(&app.screens.backup.output_dir);
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
                app.screens.backup.note.clear();
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
    let busy = app.screens.backup.busy();
    let creating = app.screens.backup.create_job.is_running();

    card(ui, palette, "New Backup", |ui| {
        match target {
            Some((_, name, driver)) => {
                ui.label(text::muted(
                    palette,
                    format!("{} ({:?}) — via {}", name, driver, svc::backup_tool(driver)),
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
                ui.checkbox(&mut b.include_schema, "Include Schema");
                ui.add_space(16.0);
                ui.checkbox(&mut b.include_data, "Include Data");
            });

            ui.horizontal(|ui| {
                ui.label("Note");
                ui.add(
                    egui::TextEdit::singleline(&mut b.note)
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
                    egui::TextEdit::multiline(&mut b.tables)
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
                ui.add(
                    egui::TextEdit::singleline(&mut b.output_dir)
                        .hint_text("/Users/me/db-backups")
                        .desired_width(f32::INFINITY),
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
            let drop_applies = !matches!(driver, Some(DbDriver::Sqlite));
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
                let label = if app.screens.backup.drop_existing {
                    format!("Drop {} and restore", db_label)
                } else {
                    "Restore".to_string()
                };
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
        .stroke(egui::Stroke::new(1.0, palette.border))
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
        assert_eq!(format_timestamp(0), "—");
        assert_eq!(format_timestamp(-1), "—");
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
        assert_eq!(parse_tables("orders\ncustomers"), vec!["orders", "customers"]);
    }

    #[test]
    fn blank_text_fields_mean_unset_not_empty() {
        assert_eq!(optional("  "), None);
        assert_eq!(optional(""), None);
        assert_eq!(optional("  note "), Some("note".to_string()));
    }

    #[test]
    fn form_maps_onto_backup_options() {
        let mut b = Backup::default();
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
    fn failure_reason_travels_with_the_status() {
        assert_eq!(status_label(&BackupStatus::Completed), ("Completed", None));
        let failed = BackupStatus::Failed("pg_dump: connection refused".into());
        let (label, reason) = status_label(&failed);
        assert_eq!(label, "Failed");
        assert_eq!(reason, Some("pg_dump: connection refused"));
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
            ("a".to_string(), "prod orders".to_string(), "MySql".to_string()),
            ("b".to_string(), "prod/orders".to_string(), "Postgres".to_string()),
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

    #[test]
    fn a_fresh_screen_is_closed_and_backs_up_everything() {
        let b = Backup::default();
        assert!(!b.open);
        assert!(!b.busy());
        assert!(b.include_schema && b.include_data);
        assert!(b.confirm_restore.is_none() && b.confirm_delete.is_none());
    }
}
