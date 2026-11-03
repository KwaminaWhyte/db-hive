//! Controller glue for the Backup / Restore panel.
//!
//! A child module of `app`, like the history and import/export glue. The
//! logic lives in `dbhive_ui_model::backup_manager` and `backup`, and the
//! dump and restore themselves in `dbhive_core::service_backup`; this file
//! moves values between them and the `BackupState` global in the window.
//!
//! A backup or restore drives the database's own dump tool to completion on a
//! blocking thread. The service reports its stages down a channel; a task on
//! the runtime forwards each one to the UI thread, so the progress readout
//! moves as stages arrive instead of waiting for the end. The tools cannot be
//! interrupted safely, so there is no cancel: the panel says so.

use std::rc::Rc;
use std::time::Instant;

use dbhive_core::models::backup::{BackupEntry, BackupProgress, RestoreOptions};
use dbhive_core::models::{ConnectionProfile, DbDriver, DbError};
use dbhive_core::service_backup as svc;
use dbhive_ui_model::backup::optional;
use dbhive_ui_model::backup_manager::{
    delete_message, restore_blocker, BackupManager, OpKind, View,
};
use slint::ComponentHandle;

use super::{with_ctx, Ctx, PendingAction};
use crate::backup_view as view;
use crate::{runtime, BackupState, Tone};

/// A confirmation the panel is waiting on.
pub(super) enum Pending {
    DeleteBackup(String),
}

/// The panel's own state; the view state is `BackupManager`.
#[derive(Default)]
pub(super) struct BackupSession {
    mgr: BackupManager,
    list_latest: runtime::Latest,
    started: Option<Instant>,
}

/// The connection a backup is taken from: the connected workspace.
struct Source {
    profile: ConnectionProfile,
}

impl Source {
    fn driver_label(&self) -> String {
        format!("{:?}", self.profile.driver)
    }

    /// The thing a restore overwrites, named for the confirmation.
    fn database_label(&self) -> String {
        match self.profile.driver {
            DbDriver::Sqlite => std::path::Path::new(&svc::sqlite_file(&self.profile))
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| self.profile.name.clone()),
            _ => {
                let db = svc::target_database(&self.profile);
                if db.is_empty() {
                    self.profile.name.clone()
                } else {
                    db
                }
            }
        }
    }

    fn blocker(&self) -> String {
        if svc::supports_backup(&self.profile.driver) {
            String::new()
        } else {
            format!(
                "Backups are not supported for {:?} connections.",
                self.profile.driver
            )
        }
    }
}

impl Ctx {
    fn backup_source(&self) -> Option<Source> {
        let id = self.session.borrow().workspace.connection_id.clone();
        if id.is_empty() {
            return None;
        }
        let guard = self.state.lock().unwrap();
        guard
            .get_profile(&id)
            .cloned()
            .map(|profile| Source { profile })
    }

    // -- Projection -------------------------------------------------------------

    pub(super) fn refresh_backup_panel(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let source = self.backup_source();
        let s = self.session.borrow();
        let context = match &source {
            Some(src) => view::Context {
                location: format!("{} · {}", src.profile.name, src.driver_label()),
                driver: Some(src.profile.driver.clone()),
                target_name: src.profile.name.clone(),
                database_label: src.database_label(),
                blocker: src.blocker(),
            },
            None => view::Context::default(),
        };
        view::project(&w.global::<BackupState>(), &s.backups.mgr, &context);
    }

    // -- Open and close -------------------------------------------------------------

    pub(super) fn open_backup_panel(self: &Rc<Self>) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        if self.backup_source().is_none() {
            self.toast(
                "Open a connection to back it up or restore into it.",
                Tone::Neutral,
            );
            return;
        }
        self.hide_history_panel();
        self.hide_data_panel();
        {
            let mut s = self.session.borrow_mut();
            let m = &mut s.backups.mgr;
            // Reopening while an operation is running shows it; otherwise
            // start from the list with a clean slate.
            if !m.busy() {
                m.view = View::List;
                m.op = None;
                m.restore_target = None;
                m.form_error.clear();
            }
        }
        w.global::<BackupState>().set_visible(true);
        self.refresh_backup_panel();
        self.load_backups();
    }

    pub(super) fn hide_backup_panel(&self) {
        if let Some(w) = self.window.upgrade() {
            w.global::<BackupState>().set_visible(false);
        }
    }

    // -- The list ----------------------------------------------------------------------

    fn load_backups(self: &Rc<Self>) {
        let profiles: Vec<(String, String, String)> = {
            let guard = self.state.lock().unwrap();
            guard
                .list_profiles()
                .into_iter()
                .map(|p| (p.id.clone(), p.name.clone(), format!("{:?}", p.driver)))
                .collect()
        };
        let (dir, ticket) = {
            let mut s = self.session.borrow_mut();
            s.backups.mgr.loading = true;
            s.backups.mgr.list_error.clear();
            (
                optional(&s.backups.mgr.form.output_dir),
                s.backups.list_latest.begin(),
            )
        };
        self.refresh_backup_panel();
        runtime::spawn(
            async move {
                // The listing stats every file: a blocking thread, not a
                // runtime worker.
                tokio::task::spawn_blocking(move || {
                    let resolved = match dir {
                        Some(d) => d,
                        None => svc::get_backup_directory()?,
                    };
                    let mut entries = svc::list_backups(Some(&resolved))?;
                    dbhive_ui_model::backup::annotate_entries(&mut entries, &profiles);
                    Ok((resolved, entries))
                })
                .await
                .unwrap_or_else(|e| Err(DbError::InternalError(format!("Listing failed: {e}"))))
            },
            move |result: Result<(String, Vec<BackupEntry>), DbError>| {
                with_ctx(|ctx| {
                    if !ticket.is_current() {
                        return;
                    }
                    {
                        let mut s = ctx.session.borrow_mut();
                        let m = &mut s.backups.mgr;
                        m.loading = false;
                        match result {
                            Ok((dir, entries)) => {
                                m.directory = dir;
                                m.entries = entries;
                            }
                            Err(e) => {
                                m.list_error = format!("Could not read the backup folder: {e}")
                            }
                        }
                    }
                    ctx.refresh_backup_panel();
                })
            },
        );
    }

    fn open_backup_folder(self: &Rc<Self>) {
        let dir = {
            let s = self.session.borrow();
            let d = &s.backups.mgr.directory;
            (!d.is_empty()).then(|| d.clone())
        };
        let result = match dir {
            Some(d) => svc::open_directory(&d),
            None => svc::open_backup_directory(),
        };
        if let Err(e) = result {
            self.toast(format!("Could not open the folder: {e}"), Tone::Danger);
        }
    }

    // -- Create ---------------------------------------------------------------------------

    fn backup_new(&self) {
        {
            let mut s = self.session.borrow_mut();
            s.backups.mgr.form_error.clear();
            s.backups.mgr.view = View::Create;
        }
        self.refresh_backup_panel();
    }

    fn backup_create(self: &Rc<Self>) {
        let Some(source) = self.backup_source() else {
            return;
        };
        let options = {
            let mut s = self.session.borrow_mut();
            let m = &mut s.backups.mgr;
            let options = m.form.options();
            if let Err(e) = svc::validate_backup_options(&source.profile.driver, &options) {
                m.form_error = e.to_string();
                drop(s);
                self.refresh_backup_panel();
                return;
            }
            m.form_error.clear();
            m.begin(OpKind::Backup);
            s.backups.started = Some(Instant::now());
            options
        };
        self.refresh_backup_panel();

        let state = self.state.clone();
        let id = source.profile.id.clone();
        let progress = forward_progress();
        runtime::spawn(
            async move { svc::create_backup(&state, &id, options, progress).await },
            |result| with_ctx(|ctx| ctx.backup_created(result)),
        );
    }

    fn backup_created(self: &Rc<Self>, result: Result<BackupEntry, DbError>) {
        let toast = {
            let mut s = self.session.borrow_mut();
            let elapsed = s.backups.started.take().map(|t| t.elapsed());
            let m = &mut s.backups.mgr;
            let Some(op) = m.op.as_mut() else { return };
            op.elapsed = elapsed;
            op.done = true;
            match result {
                Ok(entry) => {
                    op.error = None;
                    op.message = format!("Backup written to {}", entry.file_path);
                    m.form.note.clear();
                    (
                        format!("Backup created: {}", entry.file_path),
                        Tone::Success,
                    )
                }
                Err(e) => {
                    op.error = Some(e.to_string());
                    (format!("Backup failed: {e}"), Tone::Danger)
                }
            }
        };
        self.toast(toast.0, toast.1);
        self.refresh_backup_panel();
        self.load_backups();
    }

    /// A stage arrived from the running service.
    fn backup_progress(&self, p: BackupProgress) {
        {
            let mut s = self.session.borrow_mut();
            let elapsed = s.backups.started.map(|t| t.elapsed());
            let Some(op) = s.backups.mgr.op.as_mut() else {
                return;
            };
            // The completion handler is authoritative once it has run.
            if op.done {
                return;
            }
            op.apply(&p);
            // "done" from the sink only means the tool finished; the result
            // (entry or error) is applied by the completion handler.
            if p.done {
                op.done = false;
                op.stage = "writing".to_string();
            }
            op.elapsed = elapsed;
        }
        self.refresh_backup_panel();
    }

    // -- Restore ------------------------------------------------------------------------------

    fn backup_restore_asked(&self, path: &str) {
        {
            let mut s = self.session.borrow_mut();
            let Some(entry) = s.backups.mgr.find(path).cloned() else {
                return;
            };
            s.backups.mgr.open_restore(entry);
        }
        self.refresh_backup_panel();
    }

    fn backup_restore(self: &Rc<Self>) {
        let Some(source) = self.backup_source() else {
            return;
        };
        let (options, file) = {
            let mut s = self.session.borrow_mut();
            let m = &mut s.backups.mgr;
            let Some(entry) = m.restore_target.clone() else {
                return;
            };
            if restore_blocker(&entry, Some(&source.profile.driver)).is_some() {
                return;
            }
            let options = RestoreOptions {
                file_path: entry.file_path.clone(),
                drop_existing: m.drop_existing,
            };
            m.begin(OpKind::Restore);
            s.backups.started = Some(Instant::now());
            (options, entry.file_name)
        };
        self.refresh_backup_panel();

        let state = self.state.clone();
        let id = source.profile.id.clone();
        let progress = forward_progress();
        runtime::spawn(
            async move { svc::restore_backup(&state, &id, options, progress).await },
            move |result| with_ctx(|ctx| ctx.backup_restored(file, result)),
        );
    }

    fn backup_restored(self: &Rc<Self>, file: String, result: Result<(), DbError>) {
        let mut reload = false;
        let toast = {
            let mut s = self.session.borrow_mut();
            let elapsed = s.backups.started.take().map(|t| t.elapsed());
            let m = &mut s.backups.mgr;
            let Some(op) = m.op.as_mut() else { return };
            op.elapsed = elapsed;
            op.done = true;
            match result {
                Ok(()) => {
                    op.error = None;
                    op.message = format!("Restored from {file}");
                    reload = true;
                    (format!("Restored from {file}"), Tone::Success)
                }
                Err(e) => {
                    op.error = Some(e.to_string());
                    (format!("Restore failed: {e}"), Tone::Danger)
                }
            }
        };
        self.toast(toast.0, toast.1);
        self.refresh_backup_panel();
        if reload {
            // The tables behind the sidebar may all have changed.
            self.load_tables();
        }
    }

    // -- Delete ---------------------------------------------------------------------------------

    fn backup_delete_asked(&self, path: &str) {
        let Some(entry) = self.session.borrow().backups.mgr.find(path).cloned() else {
            return;
        };
        self.session.borrow_mut().pending_action = Some(PendingAction::Backup(
            Pending::DeleteBackup(entry.file_path.clone()),
        ));
        self.show_confirm("Delete this backup?", &delete_message(&entry), "Delete");
    }

    pub(super) fn backup_confirmed(self: &Rc<Self>, pending: Pending) {
        match pending {
            Pending::DeleteBackup(path) => {
                let name = std::path::Path::new(&path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.clone());
                match svc::delete_backup(&path) {
                    Ok(()) => self.toast(format!("Deleted {name}"), Tone::Success),
                    Err(e) => self.toast(format!("Delete failed: {e}"), Tone::Danger),
                }
                self.load_backups();
            }
        }
    }
}

/// A progress sink whose stages are forwarded to the UI thread as they
/// arrive.
fn forward_progress() -> svc::ProgressSink {
    let (sink, mut rx) = svc::ProgressSink::channel();
    runtime::handle().spawn(async move {
        while let Some(p) = rx.recv().await {
            let _ = slint::invoke_from_event_loop(move || with_ctx(|ctx| ctx.backup_progress(p)));
        }
    });
    sink
}

/// Connect every `BackupState` callback to the controller.
pub(super) fn wire(window: &crate::AppWindow, ctx: &Rc<Ctx>) {
    let g = window.global::<BackupState>();

    macro_rules! on {
        ($setter:ident, |$c:ident $(, $a:ident)*| $body:expr) => {{
            let $c = ctx.clone();
            g.$setter(move |$($a),*| {
                let $c = &$c;
                $body
            });
        }};
    }

    on!(on_open_requested, |c| c.open_backup_panel());
    on!(on_close, |c| c.hide_backup_panel());
    on!(on_refresh, |c| c.load_backups());
    on!(on_open_folder, |c| c.open_backup_folder());
    on!(on_filter_edited, |c, t| {
        c.session.borrow_mut().backups.mgr.filter = t.to_string();
        c.refresh_backup_panel();
    });
    on!(on_new_backup, |c| c.backup_new());
    on!(on_restore_asked, |c, id| c.backup_restore_asked(&id));
    on!(on_delete_asked, |c, id| c.backup_delete_asked(&id));

    on!(on_create_submit, |c| c.backup_create());
    on!(on_create_cancel, |c| {
        c.session.borrow_mut().backups.mgr.view = View::List;
        c.refresh_backup_panel();
    });
    on!(on_schema_toggled, |c| {
        {
            let mut s = c.session.borrow_mut();
            let f = &mut s.backups.mgr.form;
            f.include_schema = !f.include_schema;
        }
        c.refresh_backup_panel();
    });
    on!(on_data_toggled, |c| {
        {
            let mut s = c.session.borrow_mut();
            let f = &mut s.backups.mgr.form;
            f.include_data = !f.include_data;
        }
        c.refresh_backup_panel();
    });
    on!(on_note_edited, |c, t| c
        .session
        .borrow_mut()
        .backups
        .mgr
        .form
        .note = t.to_string());
    on!(on_tables_edited, |c, t| c
        .session
        .borrow_mut()
        .backups
        .mgr
        .form
        .tables = t.to_string());
    on!(on_output_dir_edited, |c, t| {
        let mut s = c.session.borrow_mut();
        s.backups.mgr.form.output_dir = t.to_string();
        s.backups.mgr.form_error.clear();
    });

    on!(on_restore_submit, |c| c.backup_restore());
    on!(on_restore_cancel, |c| {
        {
            let mut s = c.session.borrow_mut();
            s.backups.mgr.view = View::List;
            s.backups.mgr.restore_target = None;
        }
        c.refresh_backup_panel();
    });
    on!(on_drop_toggled, |c| {
        let driver = c.backup_source().map(|s| s.profile.driver);
        {
            let mut s = c.session.borrow_mut();
            let on = !s.backups.mgr.drop_existing;
            s.backups.mgr.set_drop_existing(on, driver.as_ref());
        }
        c.refresh_backup_panel();
    });

    on!(on_progress_dismiss, |c| {
        c.session.borrow_mut().backups.mgr.dismiss_progress();
        c.refresh_backup_panel();
    });
}
