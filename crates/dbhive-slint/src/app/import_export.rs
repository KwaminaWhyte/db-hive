//! Controller glue for the Import / Export panel.
//!
//! A child module of `app` so it can use the controller's private pieces
//! (`Ctx`, `Session`, `with_ctx`, `load_tables`) without widening their
//! visibility. The logic lives in `dbhive_ui_model::import_wizard` and
//! `data_forms` and in `dbhive_core::service_import_export`; this file moves
//! values between them, the stores in `AppState`, and the `ImportExportState`
//! global in the window.
//!
//! Long jobs (reading a file, running an import, writing a dump) go through
//! `runtime::spawn`. Nothing here holds the `AppState` lock across an await:
//! the services take the `Mutex` and lock it only to clone a driver.

use std::rc::Rc;
use std::sync::atomic::Ordering;

use dbhive_core::models::{DbDriver, DbError};
use dbhive_core::service_import_export as svc;
use dbhive_ui_model::data_forms as forms;
use dbhive_ui_model::import_export::{
    import_summary, sql_import_outcome, sql_import_summary, Outcome, Step,
};
use dbhive_ui_model::import_wizard::{file_problem, ImportWizard};
use dbhive_ui_model::workspace_tabs::TabKind;
use slint::ComponentHandle;
use svc::SqlImportResult;

use super::{with_ctx, Ctx};
use crate::import_export_view::{self as view, DataSession};
use crate::{runtime, ImportExportState, Tone};

/// The connection the panel acts on, read from the workspace.
struct Target {
    id: String,
    name: String,
    driver: DbDriver,
    /// The schema (Postgres), database (MySQL) or file (SQLite) the workspace
    /// is on, as `load_tables` recorded it.
    raw_schema: String,
}

impl Target {
    /// What table names are qualified with; blank on SQLite.
    fn schema(&self) -> String {
        svc::effective_schema(&self.driver, &self.raw_schema)
    }
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn suggest_dir() -> std::path::PathBuf {
    svc::suggested_directory()
}

impl Ctx {
    fn data_target(&self) -> Option<Target> {
        let s = self.session.borrow();
        let ws = &s.workspace;
        if ws.connection_id.is_empty() {
            return None;
        }
        let driver = ws.driver.clone()?;
        // Until the tables have loaded the workspace has no schema; on
        // Postgres its database name is not one, so leave it blank (public).
        let raw_schema = match (&ws.schema, &ws.database) {
            (Some(schema), _) => schema.clone(),
            (None, _) if driver.is_postgres_compatible() => String::new(),
            (None, Some(database)) => database.clone(),
            (None, None) => String::new(),
        };
        Some(Target {
            id: ws.connection_id.clone(),
            name: ws.connection_name.clone(),
            driver,
            raw_schema,
        })
    }

    // -- Projection ---------------------------------------------------------

    /// Project the panel's state into the `ImportExportState` global. The
    /// text fields the user types into are not touched here: they are set
    /// only when a session starts or a suggestion is applied.
    pub(super) fn refresh_data_panel(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let s = self.session.borrow();
        let ws = &s.workspace;
        let place = ws
            .schema
            .clone()
            .or_else(|| ws.database.clone())
            .unwrap_or_default();
        view::project(
            &w.global::<ImportExportState>(),
            &s.data,
            &view::location_label(&ws.connection_name, &place),
            ws.driver.as_ref(),
        );
    }

    /// Put a file preview in the grid, or empty it.
    fn set_preview_models(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let s = self.session.borrow();
        view::set_preview(&w.global::<ImportExportState>(), &s.data.wizard);
    }

    // -- Open and close ------------------------------------------------------

    /// Open the panel on a connection. `table` preselects a table (the table
    /// tab that is open, for instance).
    pub(super) fn open_import_export(self: &Rc<Self>, tab: Option<i32>) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let Some(target) = self.data_target() else {
            self.toast("Open a connection to import or export data.", Tone::Neutral);
            return;
        };
        self.hide_history_panel();
        self.hide_backup_panel();

        // The table tab in front, if any: the natural target of an export.
        let context_table = {
            let s = self.session.borrow();
            match s.workspace.tabs.active().map(|t| &t.kind) {
                Some(TabKind::Table(name)) => Some(name.clone()),
                _ => None,
            }
        };

        let fresh = self.session.borrow().data.connection_id != target.id;
        let g = w.global::<ImportExportState>();
        {
            let mut s = self.session.borrow_mut();
            if fresh {
                s.data = DataSession {
                    connection_id: target.id.clone(),
                    ..DataSession::default()
                };
                s.data.wizard = ImportWizard::new(&target.schema());
                s.data.dump.path =
                    s.data
                        .dump
                        .suggested_path(&suggest_dir(), &target.raw_schema_name(), &today());
                g.set_import_path("".into());
                g.set_new_table_text("".into());
                g.set_skip_rows("".into());
                g.set_batch_size("".into());
                g.set_confirm_text("".into());
                g.set_sql_path("".into());
                g.set_export_path("".into());
                g.set_export_limit("".into());
                g.set_dump_tables("".into());
                g.set_dump_path(s.data.dump.path.clone().into());
            }
            let d = &mut s.data;
            // The schema may have been discovered since the last opening.
            d.wizard.schema = target.schema();
            d.wizard.options.schema =
                (!d.wizard.schema.is_empty()).then(|| d.wizard.schema.clone());
            if let Some(t) = tab {
                d.tab = t.clamp(0, forms::TABS.len() as i32 - 1);
            }
            if let Some(name) = &context_table {
                d.export.table = name.clone();
                if fresh || d.wizard.options.table_name.is_empty() {
                    d.wizard.options.table_name = name.clone();
                }
            }
        }
        if fresh || context_table.is_some() {
            self.suggest_export_path(false);
        }
        g.set_type_options(view::type_options());
        g.set_visible(true);
        self.refresh_data_panel();
        self.load_data_tables();
    }

    /// The schema tree's "Import Data…": the import tab, aimed at `table`
    /// when there is one. The caller has already made its schema current.
    pub(super) fn open_import_into(self: &Rc<Self>, table: Option<&str>) {
        self.open_import_export(Some(0));
        let Some(name) = table else { return };
        let load = {
            let mut s = self.session.borrow_mut();
            let w = &mut s.data.wizard;
            w.set_target_table(name);
            w.preview.is_some() && w.target_columns.is_empty()
        };
        self.refresh_data_panel();
        if load {
            self.load_target_columns();
        }
    }

    pub(super) fn hide_data_panel(&self) {
        if let Some(w) = self.window.upgrade() {
            w.global::<ImportExportState>().set_visible(false);
        }
    }

    fn close_data_panel(&self) {
        // A running import keeps running; the panel can be reopened to see
        // how it ended. Closing never cancels it silently.
        self.hide_data_panel();
    }

    fn data_tab_selected(&self, index: i32) {
        self.session.borrow_mut().data.tab = index;
        self.refresh_data_panel();
    }

    // -- Tables --------------------------------------------------------------

    fn load_data_tables(self: &Rc<Self>) {
        let Some(target) = self.data_target() else {
            return;
        };
        let ticket = {
            let mut s = self.session.borrow_mut();
            s.data.tables_loading = true;
            s.data.tables_error.clear();
            s.data.tables_latest.begin()
        };
        self.refresh_data_panel();
        let state = self.state.clone();
        let schema = target.raw_schema.clone();
        let id = target.id.clone();
        runtime::spawn(
            self.ui_id,
            async move {
                let schema = (!schema.is_empty()).then_some(schema);
                svc::get_tables_for_import(&state, &id, schema.as_deref()).await
            },
            move |result| {
                with_ctx(|ctx| {
                    if !ticket.is_current() {
                        return;
                    }
                    ctx.tables_loaded(result);
                })
            },
        );
    }

    fn tables_loaded(self: &Rc<Self>, result: Result<Vec<String>, DbError>) {
        let mut load_columns = false;
        {
            let mut s = self.session.borrow_mut();
            let d = &mut s.data;
            d.tables_loading = false;
            match result {
                Ok(mut tables) => {
                    tables.sort();
                    d.wizard.set_tables(tables.clone());
                    // Export defaults to the first table when none is chosen
                    // or the chosen one is gone.
                    if !tables.contains(&d.export.table) {
                        d.export.table = tables.first().cloned().unwrap_or_default();
                    }
                    // A table chosen before the list arrived (from a table
                    // tab) needs its columns.
                    load_columns = d.wizard.preview.is_some()
                        && !d.wizard.options.table_name.is_empty()
                        && tables.contains(&d.wizard.options.table_name)
                        && d.wizard.target_columns.is_empty();
                }
                Err(e) => d.tables_error = format!("Could not list tables: {e}"),
            }
        }
        self.suggest_export_path(false);
        self.refresh_data_panel();
        if load_columns {
            self.load_target_columns();
        }
    }

    // -- Import wizard ---------------------------------------------------------

    fn import_path_edited(&self, text: &str) {
        {
            let mut s = self.session.borrow_mut();
            s.data.wizard.set_file_path(text);
            s.data.import_path_error.clear();
            s.data.wizard.error.clear();
        }
        self.set_preview_models();
        self.refresh_data_panel();
    }

    fn import_read_preview(self: &Rc<Self>) {
        let (path, opts, ticket) = {
            let mut s = self.session.borrow_mut();
            let d = &mut s.data;
            if let Some(p) = file_problem(&d.wizard.file_path) {
                d.import_path_error = p;
                drop(s);
                self.refresh_data_panel();
                return;
            }
            d.import_path_error.clear();
            d.wizard.error.clear();
            d.busy = "Reading the file…".to_string();
            (
                forms::expand_home(d.wizard.file_path.trim()),
                d.wizard.preview_options(),
                d.preview_latest.begin(),
            )
        };
        self.refresh_data_panel();
        runtime::spawn(
            self.ui_id,
            async move {
                // Parsing a large file is CPU and IO bound; keep it off the
                // runtime threads the drivers use.
                tokio::task::spawn_blocking(move || svc::preview_import_file(&path, &opts))
                    .await
                    .unwrap_or_else(|e| {
                        Err(DbError::InternalError(format!("Preview task failed: {e}")))
                    })
            },
            move |result| {
                with_ctx(|ctx| {
                    if !ticket.is_current() {
                        return;
                    }
                    ctx.preview_done(result);
                })
            },
        );
    }

    fn preview_done(self: &Rc<Self>, result: Result<svc::ImportPreview, DbError>) {
        let mut load_columns = false;
        {
            let mut s = self.session.borrow_mut();
            let d = &mut s.data;
            d.busy.clear();
            match result {
                Ok(preview) => {
                    d.wizard.apply_preview(preview);
                    let name = d.wizard.options.table_name.clone();
                    if !name.is_empty() {
                        if d.wizard.tables.contains(&name) {
                            load_columns = true;
                        } else {
                            d.wizard.set_typed_table(&name);
                        }
                    }
                }
                Err(e) => {
                    d.wizard.error = format!("Could not read the file: {e}");
                }
            }
        }
        self.set_preview_models();
        self.refresh_data_panel();
        if load_columns {
            self.load_target_columns();
        }
    }

    fn load_target_columns(self: &Rc<Self>) {
        let Some(target) = self.data_target() else {
            return;
        };
        let (table, ticket) = {
            let mut s = self.session.borrow_mut();
            let d = &mut s.data;
            if d.wizard.options.table_name.is_empty() {
                return;
            }
            d.columns_loading = true;
            (
                d.wizard.options.table_name.clone(),
                d.columns_latest.begin(),
            )
        };
        self.refresh_data_panel();
        let state = self.state.clone();
        let schema = target.raw_schema.clone();
        let id = target.id.clone();
        let asked = table.clone();
        runtime::spawn(
            self.ui_id,
            async move {
                let schema = (!schema.is_empty()).then_some(schema);
                svc::get_table_columns_for_import(&state, &id, &asked, schema.as_deref()).await
            },
            move |result| {
                with_ctx(|ctx| {
                    if !ticket.is_current() {
                        return;
                    }
                    {
                        let mut s = ctx.session.borrow_mut();
                        let d = &mut s.data;
                        d.columns_loading = false;
                        match result {
                            Ok(cols) if d.wizard.options.table_name == table => {
                                d.wizard.apply_target_columns(cols)
                            }
                            Ok(_) => {}
                            Err(e) => {
                                d.wizard.error = format!("Could not read the table's columns: {e}")
                            }
                        }
                    }
                    ctx.refresh_data_panel();
                })
            },
        );
    }

    fn import_sheet_picked(self: &Rc<Self>, index: i32) {
        let changed = self.session.borrow_mut().data.wizard.set_sheet_index(index);
        if changed {
            self.import_read_preview();
        }
    }

    fn import_step_picked(&self, index: i32) {
        if let Some(step) = usize::try_from(index)
            .ok()
            .and_then(|i| Step::ORDER.get(i))
            .copied()
        {
            self.session.borrow_mut().data.wizard.go_to(step);
        }
        self.refresh_data_panel();
    }

    fn import_table_picked(self: &Rc<Self>, index: i32) {
        let picked = self
            .session
            .borrow_mut()
            .data
            .wizard
            .set_target_table_by_index(index);
        if picked.is_some() {
            if let Some(w) = self.window.upgrade() {
                w.global::<ImportExportState>()
                    .set_new_table_text("".into());
            }
            self.refresh_data_panel();
            self.load_target_columns();
        }
    }

    fn import_new_table_edited(self: &Rc<Self>, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let existing = self.session.borrow_mut().data.wizard.set_typed_table(text);
        self.refresh_data_panel();
        if existing {
            self.load_target_columns();
        }
    }

    /// Plan the import: read the whole file and build the statements that
    /// would run. Nothing is written until the user has seen them.
    fn import_review(self: &Rc<Self>) {
        let Some(target) = self.data_target() else {
            return;
        };
        let (path, options, cached, ticket) = {
            let mut s = self.session.borrow_mut();
            let d = &mut s.data;
            if let Err(e) = d.wizard.commit_options() {
                d.wizard.error = e;
                drop(s);
                self.refresh_data_panel();
                return;
            }
            d.wizard.error.clear();
            d.busy = "Reading the whole file…".to_string();
            (
                forms::expand_home(d.wizard.file_path.trim()),
                d.wizard.options.clone(),
                d.wizard.source.clone(),
                d.plan_latest.begin(),
            )
        };
        self.refresh_data_panel();
        let kind = target.driver.clone();
        runtime::spawn(
            self.ui_id,
            async move {
                tokio::task::spawn_blocking(move || {
                    let source = match cached {
                        Some(s) => (*s).clone(),
                        None => svc::read_import_file(&path, &options)?,
                    };
                    let plan = svc::build_import_plan(&kind, &source, &options)?;
                    Ok((source, plan))
                })
                .await
                .unwrap_or_else(|e| Err(DbError::InternalError(format!("Read task failed: {e}"))))
            },
            move |result: Result<(svc::ImportSource, svc::ImportPlan), DbError>| {
                with_ctx(|ctx| {
                    if !ticket.is_current() {
                        return;
                    }
                    {
                        let mut s = ctx.session.borrow_mut();
                        let d = &mut s.data;
                        d.busy.clear();
                        match result {
                            Ok((source, plan)) => d.wizard.apply_plan(source, plan),
                            Err(e) => d.wizard.plan_failed(e.to_string()),
                        }
                    }
                    ctx.refresh_data_panel();
                })
            },
        );
    }

    fn import_run(self: &Rc<Self>) {
        let Some(target) = self.data_target() else {
            return;
        };
        let (source, options, cancel) = {
            let mut s = self.session.borrow_mut();
            let d = &mut s.data;
            if !d.wizard.can_import() {
                return;
            }
            let Some(source) = d.wizard.source.clone() else {
                return;
            };
            d.wizard.import_started();
            d.import_cancel.store(false, Ordering::Relaxed);
            (source, d.wizard.options.clone(), d.import_cancel.clone())
        };
        self.refresh_data_panel();
        let state = self.state.clone();
        let id = target.id.clone();
        runtime::spawn(
            self.ui_id,
            async move { svc::run_import(&state, &id, source, options, cancel).await },
            |result| with_ctx(|ctx| ctx.import_done(result)),
        );
    }

    fn import_done(self: &Rc<Self>, result: Result<svc::ImportResult, DbError>) {
        let mut reload = false;
        let toast = {
            let mut s = self.session.borrow_mut();
            let d = &mut s.data;
            match result {
                Ok(r) => {
                    let summary = import_summary(&r);
                    let tone = if r.success {
                        Tone::Success
                    } else {
                        Tone::Warning
                    };
                    reload = r.rows_imported > 0;
                    d.wizard.import_finished(r);
                    (summary, tone)
                }
                Err(e) => {
                    let msg = format!("Import failed: {e}");
                    d.wizard.import_failed(msg.clone());
                    (msg, Tone::Danger)
                }
            }
        };
        self.toast(toast.0, toast.1);
        self.refresh_data_panel();
        if reload {
            // New rows, and maybe a new table.
            self.load_tables();
            self.load_data_tables();
        }
    }

    fn import_again(self: &Rc<Self>) {
        self.session.borrow_mut().data.wizard.restart();
        if let Some(w) = self.window.upgrade() {
            let g = w.global::<ImportExportState>();
            g.set_import_path("".into());
            g.set_new_table_text("".into());
            g.set_skip_rows("".into());
            g.set_batch_size("".into());
            g.set_confirm_text("".into());
        }
        self.set_preview_models();
        self.refresh_data_panel();
    }

    // -- SQL file --------------------------------------------------------------

    fn sql_run(self: &Rc<Self>) {
        let Some(target) = self.data_target() else {
            return;
        };
        let (path, options, cancel) = {
            let mut s = self.session.borrow_mut();
            let d = &mut s.data;
            if d.sql_running {
                return;
            }
            d.sql_error.clear();
            d.sql_path_error.clear();
            let options = match d.sql.options() {
                Ok(o) => o,
                Err(e) => {
                    d.sql_path_error = e;
                    drop(s);
                    self.refresh_data_panel();
                    return;
                }
            };
            d.sql_result = None;
            d.sql_running = true;
            d.sql_cancel.store(false, Ordering::Relaxed);
            (
                forms::expand_home(d.sql.path.trim()),
                options,
                d.sql_cancel.clone(),
            )
        };
        self.refresh_data_panel();
        let state = self.state.clone();
        let id = target.id.clone();
        runtime::spawn(
            self.ui_id,
            async move { svc::import_from_sql(&state, &id, &path, options, cancel).await },
            |result| with_ctx(|ctx| ctx.sql_done(result)),
        );
    }

    fn sql_done(self: &Rc<Self>, result: Result<SqlImportResult, DbError>) {
        let mut reload = false;
        let toast = {
            let mut s = self.session.borrow_mut();
            let d = &mut s.data;
            d.sql_running = false;
            match result {
                Ok(r) => {
                    let summary = sql_import_summary(&r);
                    let tone = match sql_import_outcome(&r) {
                        Outcome::Success => Tone::Success,
                        _ => Tone::Warning,
                    };
                    reload = r.executed > 0;
                    d.sql_result = Some(r);
                    (summary, tone)
                }
                Err(e) => {
                    d.sql_error = format!("The SQL file could not be run: {e}");
                    (d.sql_error.clone(), Tone::Danger)
                }
            }
        };
        self.toast(toast.0, toast.1);
        self.refresh_data_panel();
        if reload {
            self.load_tables();
            self.load_data_tables();
        }
    }

    // -- Export a table ---------------------------------------------------------

    /// Fill the export path with a date-stamped name in the Downloads folder.
    /// Unless `force`, a path the user typed is left alone.
    fn suggest_export_path(&self, force: bool) {
        let path = {
            let mut s = self.session.borrow_mut();
            let d = &mut s.data;
            if d.export_path_touched && !force {
                return;
            }
            if force {
                d.export_path_touched = false;
            }
            d.export.path = d.export.suggested_path(&suggest_dir(), &today());
            d.export.path.clone()
        };
        if let Some(w) = self.window.upgrade() {
            w.global::<ImportExportState>().set_export_path(path.into());
        }
    }

    fn export_table_picked(&self, index: i32) {
        {
            let mut s = self.session.borrow_mut();
            let d = &mut s.data;
            if let Some(t) = usize::try_from(index)
                .ok()
                .and_then(|i| d.wizard.tables.get(i))
            {
                d.export.table = t.clone();
            }
            d.export_result.clear();
            d.export_error.clear();
        }
        self.suggest_export_path(false);
        self.refresh_data_panel();
    }

    fn export_format_picked(&self, index: i32) {
        let path = {
            let mut s = self.session.borrow_mut();
            let d = &mut s.data;
            d.export.format = index.max(0) as usize;
            d.export.retarget_extension();
            d.export_result.clear();
            d.export.path.clone()
        };
        if let Some(w) = self.window.upgrade() {
            w.global::<ImportExportState>().set_export_path(path.into());
        }
        self.refresh_data_panel();
    }

    fn export_run(self: &Rc<Self>) {
        let Some(target) = self.data_target() else {
            return;
        };
        let (table, opts) = {
            let mut s = self.session.borrow_mut();
            let d = &mut s.data;
            if d.export_running {
                return;
            }
            d.export_result.clear();
            match d.export.options(&target.schema()) {
                Ok(o) => {
                    d.export_error.clear();
                    d.export_running = true;
                    (d.export.table.clone(), o)
                }
                Err(e) => {
                    d.export_error = e;
                    drop(s);
                    self.refresh_data_panel();
                    return;
                }
            }
        };
        self.refresh_data_panel();
        let state = self.state.clone();
        let id = target.id.clone();
        let schema = target.schema();
        let path = opts.path.clone();
        runtime::spawn(
            self.ui_id,
            async move { svc::export_table_to_file(&state, &id, &schema, &table, opts).await },
            move |result| {
                with_ctx(|ctx| {
                    let message = {
                        let mut s = ctx.session.borrow_mut();
                        let d = &mut s.data;
                        d.export_running = false;
                        match result {
                            Ok(rows) => {
                                d.export_result = format!("Exported {rows} rows to {path}");
                                (d.export_result.clone(), Tone::Success)
                            }
                            Err(e) => {
                                d.export_error = format!("Export failed: {e}");
                                (d.export_error.clone(), Tone::Danger)
                            }
                        }
                    };
                    ctx.toast(message.0, message.1);
                    ctx.refresh_data_panel();
                })
            },
        );
    }

    // -- SQL dump ----------------------------------------------------------------

    fn suggest_dump_path(&self) {
        let Some(target) = self.data_target() else {
            return;
        };
        let path = {
            let mut s = self.session.borrow_mut();
            let d = &mut s.data;
            d.dump_path_touched = false;
            d.dump.path =
                d.dump
                    .suggested_path(&suggest_dir(), &target.raw_schema_name(), &today());
            d.dump.path.clone()
        };
        if let Some(w) = self.window.upgrade() {
            w.global::<ImportExportState>().set_dump_path(path.into());
        }
    }

    fn dump_run(self: &Rc<Self>) {
        let Some(target) = self.data_target() else {
            return;
        };
        let (opts, path) = {
            let mut s = self.session.borrow_mut();
            let d = &mut s.data;
            if d.dump_running {
                return;
            }
            d.dump_result.clear();
            match d.dump.options(&target.raw_schema) {
                Ok(o) => {
                    d.dump_error.clear();
                    d.dump_running = true;
                    (o, forms::expand_home(d.dump.path.trim()))
                }
                Err(e) => {
                    d.dump_error = e;
                    drop(s);
                    self.refresh_data_panel();
                    return;
                }
            }
        };
        self.refresh_data_panel();
        let state = self.state.clone();
        let id = target.id.clone();
        let shown = path.clone();
        runtime::spawn(
            self.ui_id,
            async move { svc::export_to_sql(&state, &id, &path, opts).await },
            move |result| {
                with_ctx(|ctx| {
                    let message = {
                        let mut s = ctx.session.borrow_mut();
                        let d = &mut s.data;
                        d.dump_running = false;
                        match result {
                            Ok(n) => {
                                d.dump_result = format!(
                                    "Dumped {n} {} to {shown}",
                                    if n == 1 { "table" } else { "tables" }
                                );
                                (d.dump_result.clone(), Tone::Success)
                            }
                            Err(e) => {
                                d.dump_error = format!("Dump failed: {e}");
                                (d.dump_error.clone(), Tone::Danger)
                            }
                        }
                    };
                    ctx.toast(message.0, message.1);
                    ctx.refresh_data_panel();
                })
            },
        );
    }
}

impl Target {
    /// A name for the dump file: the database or schema, not a full path.
    fn raw_schema_name(&self) -> String {
        let name = std::path::Path::new(&self.raw_schema)
            .file_stem()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.is_empty() {
            self.name.clone()
        } else {
            name
        }
    }
}

/// Connect every `ImportExportState` callback to the controller.
pub(super) fn wire(window: &crate::AppWindow, ctx: &Rc<Ctx>) {
    let g = window.global::<ImportExportState>();
    g.set_type_options(view::type_options());

    macro_rules! on {
        ($setter:ident, |$c:ident $(, $a:ident)*| $body:expr) => {{
            let $c = ctx.clone();
            g.$setter(move |$($a),*| {
                let $c = &$c;
                $body
            });
        }};
    }

    on!(on_open_requested, |c| c.open_import_export(None));
    on!(on_close, |c| c.close_data_panel());
    on!(on_tab_selected, |c, i| c.data_tab_selected(i));
    on!(on_retry_tables, |c| c.load_data_tables());

    on!(on_step_picked, |c, i| c.import_step_picked(i));
    on!(on_import_path_edited, |c, t| c.import_path_edited(&t));
    on!(on_import_header_toggled, |c| {
        {
            let mut s = c.session.borrow_mut();
            let on = !s.data.wizard.options.first_row_is_header;
            s.data.wizard.set_first_row_is_header(on);
        }
        c.refresh_data_panel();
    });
    on!(on_delimiter_picked, |c, i| {
        c.session.borrow_mut().data.wizard.set_delimiter_index(i);
        c.refresh_data_panel();
    });
    on!(on_sheet_picked, |c, i| c.import_sheet_picked(i));
    on!(on_import_read_preview, |c| c.import_read_preview());
    on!(on_import_back, |c| {
        c.session.borrow_mut().data.wizard.back();
        c.refresh_data_panel();
    });
    on!(on_import_next, |c| {
        c.session.borrow_mut().data.wizard.next();
        c.refresh_data_panel();
    });
    on!(on_import_review, |c| c.import_review());
    on!(on_import_run, |c| c.import_run());
    on!(on_import_stop, |c| {
        c.session
            .borrow()
            .data
            .import_cancel
            .store(true, Ordering::Relaxed);
    });
    on!(on_import_again, |c| c.import_again());
    on!(on_table_picked, |c, i| c.import_table_picked(i));
    on!(on_new_table_edited, |c, t| c.import_new_table_edited(&t));
    on!(on_auto_map, |c| {
        c.session.borrow_mut().data.wizard.auto_map();
        c.refresh_data_panel();
    });
    on!(on_mapping_target, |c, row, choice| {
        c.session
            .borrow_mut()
            .data
            .wizard
            .set_mapping_target(row.max(0) as usize, choice.max(0) as usize);
        c.refresh_data_panel();
    });
    on!(on_mapping_type, |c, row, ty| {
        c.session
            .borrow_mut()
            .data
            .wizard
            .set_mapping_type(row.max(0) as usize, ty.max(0) as usize);
        c.refresh_data_panel();
    });
    on!(on_mapping_default, |c, row, t| {
        // No refresh: rebuilding the rows would take focus from the field
        // being typed in.
        c.session
            .borrow_mut()
            .data
            .wizard
            .set_mapping_default(row.max(0) as usize, &t);
    });
    on!(on_mapping_skip, |c, row| {
        c.session
            .borrow_mut()
            .data
            .wizard
            .toggle_skip(row.max(0) as usize);
        c.refresh_data_panel();
    });
    on!(on_truncate_toggled, |c| {
        {
            let mut s = c.session.borrow_mut();
            let on = !s.data.wizard.options.truncate_before;
            s.data.wizard.set_truncate(on);
        }
        c.refresh_data_panel();
    });
    on!(on_skip_rows_edited, |c, t| c
        .session
        .borrow_mut()
        .data
        .wizard
        .set_skip_rows_text(&t));
    on!(on_batch_size_edited, |c, t| c
        .session
        .borrow_mut()
        .data
        .wizard
        .set_batch_size_text(&t));
    on!(on_confirm_edited, |c, t| {
        c.session.borrow_mut().data.wizard.confirm_input = t.to_string();
        c.refresh_data_panel();
    });

    on!(on_sql_path_edited, |c, t| {
        {
            let mut s = c.session.borrow_mut();
            s.data.sql.path = t.to_string();
            s.data.sql_path_error.clear();
        }
        c.refresh_data_panel();
    });
    on!(on_sql_transaction_toggled, |c| {
        {
            let mut s = c.session.borrow_mut();
            s.data.sql.use_transaction = !s.data.sql.use_transaction;
        }
        c.refresh_data_panel();
    });
    on!(on_sql_continue_toggled, |c| {
        {
            let mut s = c.session.borrow_mut();
            s.data.sql.continue_on_error = !s.data.sql.continue_on_error;
        }
        c.refresh_data_panel();
    });
    on!(on_sql_run, |c| c.sql_run());
    on!(on_sql_stop, |c| {
        c.session
            .borrow()
            .data
            .sql_cancel
            .store(true, Ordering::Relaxed);
    });
    on!(on_sql_again, |c| {
        {
            let mut s = c.session.borrow_mut();
            s.data.sql_result = None;
            s.data.sql_error.clear();
            s.data.sql.path.clear();
        }
        if let Some(w) = c.window.upgrade() {
            w.global::<ImportExportState>().set_sql_path("".into());
        }
        c.refresh_data_panel();
    });

    on!(on_export_table_picked, |c, i| c.export_table_picked(i));
    on!(on_export_format_picked, |c, i| c.export_format_picked(i));
    on!(on_export_path_edited, |c, t| {
        {
            let mut s = c.session.borrow_mut();
            s.data.export.path = t.to_string();
            s.data.export_path_touched = true;
            s.data.export_error.clear();
        }
        c.refresh_data_panel();
    });
    on!(on_export_suggest, |c| {
        c.suggest_export_path(true);
        c.refresh_data_panel();
    });
    on!(on_export_delimiter_picked, |c, i| {
        c.session.borrow_mut().data.export.delimiter = i.max(0) as usize;
        c.refresh_data_panel();
    });
    on!(on_export_headers_toggled, |c| {
        {
            let mut s = c.session.borrow_mut();
            s.data.export.include_headers = !s.data.export.include_headers;
        }
        c.refresh_data_panel();
    });
    on!(on_export_limit_edited, |c, t| c
        .session
        .borrow_mut()
        .data
        .export
        .limit_text =
        t.to_string());
    on!(on_export_run, |c| c.export_run());

    on!(on_dump_drop_toggled, |c| {
        {
            let mut s = c.session.borrow_mut();
            s.data.dump.include_drop = !s.data.dump.include_drop;
        }
        c.refresh_data_panel();
    });
    on!(on_dump_create_toggled, |c| {
        {
            let mut s = c.session.borrow_mut();
            s.data.dump.include_create = !s.data.dump.include_create;
        }
        c.refresh_data_panel();
    });
    on!(on_dump_data_toggled, |c| {
        {
            let mut s = c.session.borrow_mut();
            s.data.dump.include_data = !s.data.dump.include_data;
        }
        c.refresh_data_panel();
    });
    on!(on_dump_tables_edited, |c, t| {
        c.session.borrow_mut().data.dump.tables_text = t.to_string();
        c.refresh_data_panel();
    });
    on!(on_dump_path_edited, |c, t| {
        {
            let mut s = c.session.borrow_mut();
            s.data.dump.path = t.to_string();
            s.data.dump_path_touched = true;
            s.data.dump_error.clear();
        }
        c.refresh_data_panel();
    });
    on!(on_dump_suggest, |c| {
        c.suggest_dump_path();
        c.refresh_data_panel();
    });
    on!(on_dump_run, |c| c.dump_run());
}
