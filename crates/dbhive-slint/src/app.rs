//! The application controller: owns state, feeds the window, handles intents.
//!
//! Slint is retained mode, so this is event driven. A callback changes
//! session state and then calls the one function that projects that state
//! into the window's properties. Nothing polls.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dbhive_core::drivers::DatabaseDriver;
use dbhive_core::models::metadata::TableInfo;
use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use slint::{ComponentHandle, ModelRc, VecModel};

use dbhive_ui_model::connection_editor::{
    build_profile, build_profile_with, form_from_profile, paste_connection_string, save_profile,
    test_profile, validate, ConnectionForm,
};
use dbhive_ui_model::results_grid::visible_rows;
use dbhive_ui_model::settings_editor::{
    destructive_warnings, query_timeout, timeout_message, trim_history,
};
use dbhive_ui_model::workspace_tabs::{result_summary, TabKind, Tabs};

mod settings;

use crate::connection_editor as editor_view;
use crate::connection_list::{self, EnvTone};
use crate::grid_model::{layout_columns, LazyRows};
use crate::runtime;
use crate::workspace_model;
use crate::sql_highlight;
use crate::{
    AppWindow, ConnectionItem, GridRow, Screen, TabInfo, TableItem, Theme, ToastItem, Tone,
};

/// How many rows a table preview pulls. Not user-configurable yet; the
/// egui table editor's own default (`workspace.rs:516`) is 100, this is a
/// little more generous since the grid virtualises past what is on screen.
const PREVIEW_ROW_LIMIT: u32 = 200;

/// The connected workspace: which connection, which table, and its rows.
#[derive(Default)]
struct Workspace {
    connection_id: String,
    connection_name: String,
    driver: Option<DbDriver>,
    database: Option<String>,
    schema: Option<String>,
    tables: Vec<TableInfo>,
    loading_tables: bool,
    tables_error: String,
    tables_latest: runtime::Latest,
    /// Query and table-preview tabs. Each tab keeps its own full result;
    /// filtering and sorting read it and project a subset every refresh
    /// rather than mutating it, so "reset the filter" never needs to
    /// remember the original order.
    tabs: Tabs,
}

/// What a confirmation dialog, once accepted, should do. Deleting a
/// connection has its own slot (`Session::pending_delete`).
enum PendingAction {
    /// Close a query tab that still has SQL in it.
    CloseTab(u64),
    /// Run a statement the destructive-query guard flagged.
    RunAnyway(u64),
    /// Reset every setting to its default (Settings > Reset to defaults).
    ResetSettings,
    /// Close Settings and throw away its unsaved edits.
    DiscardSettings,
}

/// The new/edit connection dialog's state.
///
/// The password lives here, never in `ConnectionForm`: that struct becomes a
/// `ConnectionProfile` that gets written to JSON, and a profile never carries
/// a plaintext secret. See `dbhive_ui_model::connection_editor::save_profile`.
struct Editor {
    is_new: bool,
    form: ConnectionForm,
    password: String,
    show_password: bool,
    /// Text in the "paste a connection string" box, and why the last attempt
    /// to apply it failed. Neither is part of the saved profile.
    connection_string: String,
    connection_string_error: String,
    /// Like `password`: held here so the secret never enters the profile.
    ssh_password: String,
    show_ssh_password: bool,
    testing: bool,
    test_message: String,
    test_ok: bool,
    saving: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            is_new: true,
            form: ConnectionForm::default(),
            password: String::new(),
            show_password: false,
            connection_string: String::new(),
            connection_string_error: String::new(),
            ssh_password: String::new(),
            show_ssh_password: false,
            testing: false,
            test_message: String::new(),
            test_ok: false,
            saving: false,
        }
    }
}

/// UI-thread session state. Shared with callbacks through `Rc<RefCell<_>>`,
/// which is fine because Slint callbacks all run on the UI thread.
#[derive(Default)]
struct Session {
    search: String,
    connected: HashSet<String>,
    connecting: Option<String>,
    error: String,
    toasts: Vec<ToastItem>,
    next_toast: i32,
    editor: Editor,
    /// The connection a delete confirmation is pending for: `(id, name)`.
    pending_delete: Option<(String, String)>,
    pending_action: Option<PendingAction>,
    workspace: Workspace,
    settings: settings::SettingsState,
}

struct Ctx {
    window: slint::Weak<AppWindow>,
    state: Arc<Mutex<AppState>>,
    session: RefCell<Session>,
}

impl Ctx {
    fn profiles(&self) -> Vec<ConnectionProfile> {
        // Copy out, drop the lock, then build models: never hold the state
        // lock while touching the UI.
        let guard = self.state.lock().unwrap();
        guard.list_profiles().into_iter().cloned().collect()
    }

    fn refresh_connections(&self) {
        let Some(w) = self.window.upgrade() else { return };
        let profiles = self.profiles();
        let s = self.session.borrow();

        let items: Vec<ConnectionItem> = connection_list::visible(&profiles, &s.search)
            .into_iter()
            .map(|p| {
                let (initials, (r, g, b)) = connection_list::driver_chip(&p.driver);
                let (env_label, env_tone) = match connection_list::environment(p) {
                    Some((l, EnvTone::Neutral)) => (l, Tone::Neutral),
                    Some((l, EnvTone::Warning)) => (l, Tone::Warning),
                    Some((l, EnvTone::Danger)) => (l, Tone::Danger),
                    None => ("", Tone::Neutral),
                };
                ConnectionItem {
                    id: p.id.as_str().into(),
                    name: p.name.as_str().into(),
                    host: connection_list::subtitle(p).into(),
                    initials: initials.into(),
                    tint: slint::Color::from_rgb_u8(r, g, b),
                    env_label: env_label.into(),
                    env_tone,
                    favorite: p.is_favorite,
                    connected: s.connected.contains(&p.id),
                    connecting: s.connecting.as_deref() == Some(p.id.as_str()),
                }
            })
            .collect();

        w.set_connection_total(profiles.len() as i32);
        w.set_connections(ModelRc::new(VecModel::from(items)));
        w.set_connection_error(s.error.as_str().into());
    }

    fn toast(self: &Rc<Self>, text: impl Into<String>, tone: Tone) {
        let id = {
            let mut s = self.session.borrow_mut();
            s.next_toast += 1;
            let id = s.next_toast;
            s.toasts.push(ToastItem {
                id,
                text: text.into().into(),
                tone,
            });
            id
        };
        self.refresh_toasts();

        // Errors stay long enough to read; confirmations get out of the way.
        let ttl = if tone == Tone::Danger { 10 } else { 4 };
        let ctx = Rc::downgrade(self);
        slint::Timer::single_shot(Duration::from_secs(ttl), move || {
            if let Some(ctx) = ctx.upgrade() {
                ctx.dismiss_toast(id);
            }
        });
    }

    fn dismiss_toast(&self, id: i32) {
        self.session.borrow_mut().toasts.retain(|t| t.id != id);
        self.refresh_toasts();
    }

    fn refresh_toasts(&self) {
        if let Some(w) = self.window.upgrade() {
            let toasts = self.session.borrow().toasts.clone();
            w.set_toasts(ModelRc::new(VecModel::from(toasts)));
        }
    }

    fn connect(self: &Rc<Self>, id: String) {
        {
            let mut s = self.session.borrow_mut();
            if s.connecting.is_some() {
                return;
            }
            s.connecting = Some(id.clone());
            s.error.clear();
        }
        self.refresh_connections();

        let state = self.state.clone();
        let worker_id = id.clone();
        runtime::spawn(
            async move { dbhive_core::service::connect(&state, worker_id, String::new(), None).await },
            move |result| with_ctx(|ctx| {
                ctx.session.borrow_mut().connecting = None;
                match result {
                    Ok(conn_id) => {
                        ctx.session.borrow_mut().connected.insert(conn_id.clone());
                        let profile = ctx.find_profile(&conn_id);
                        let name = profile
                            .as_ref()
                            .map(|p| p.name.clone())
                            .unwrap_or_else(|| conn_id.clone());
                        ctx.toast(format!("Connected to {}", name), Tone::Success);
                        if let Some(profile) = profile {
                            ctx.open_workspace(profile);
                        }
                    }
                    Err(e) => {
                        ctx.session.borrow_mut().error = format!("Connection failed: {}", e);
                        ctx.toast("Connection failed", Tone::Danger);
                    }
                }
                ctx.refresh_connections();
            }),
        );
    }

    fn disconnect(self: &Rc<Self>, id: String) {
        let state = self.state.clone();
        let worker_id = id.clone();
        runtime::spawn(
            async move { dbhive_core::service::disconnect(&state, &worker_id).await },
            move |result| with_ctx(|ctx| {
                ctx.session.borrow_mut().connected.remove(&id);
                match result {
                    Ok(()) => {
                        // The workspace screen has nothing to show once its
                        // connection is gone.
                        let showing_it = ctx.session.borrow().workspace.connection_id == id;
                        if showing_it {
                            ctx.leave_workspace();
                        }
                    }
                    Err(e) => ctx.toast(format!("Disconnect failed: {}", e), Tone::Danger),
                }
                ctx.refresh_connections();
            }),
        );
    }

    fn find_profile(&self, id: &str) -> Option<ConnectionProfile> {
        self.state.lock().unwrap().get_profile(id).cloned()
    }

    // -- Connection editor -------------------------------------------------

    fn refresh_editor(&self) {
        let Some(w) = self.window.upgrade() else { return };
        let s = self.session.borrow();
        let ed = &s.editor;
        let mut data = editor_view::form_data(&ed.form);
        data.password = ed.password.as_str().into();
        data.show_password_text = ed.show_password;
        data.connection_string = ed.connection_string.as_str().into();
        data.connection_string_error = ed.connection_string_error.as_str().into();
        data.ssh_password = ed.ssh_password.as_str().into();
        data.show_ssh_password_text = ed.show_ssh_password;
        w.set_editor_is_new(ed.is_new);
        w.set_editor(data);
        let errors: Vec<slint::SharedString> =
            validate(&ed.form).messages().into_iter().map(Into::into).collect();
        w.set_editor_errors(ModelRc::new(VecModel::from(errors)));
        w.set_editor_testing(ed.testing);
        w.set_editor_test_message(ed.test_message.as_str().into());
        w.set_editor_test_ok(ed.test_ok);
        w.set_editor_saving(ed.saving);
    }

    fn open_new_connection(&self) {
        self.session.borrow_mut().editor = Editor::default();
        self.refresh_editor();
        if let Some(w) = self.window.upgrade() {
            w.set_show_connection_editor(true);
        }
    }

    fn open_edit_connection(&self, id: &str) {
        let Some(profile) = self.find_profile(id) else { return };
        self.session.borrow_mut().editor = Editor {
            is_new: false,
            form: form_from_profile(&profile),
            ..Editor::default()
        };
        self.refresh_editor();
        if let Some(w) = self.window.upgrade() {
            w.set_show_connection_editor(true);
        }
    }

    fn open_duplicate_connection(&self, id: &str) {
        let Some(profile) = self.find_profile(id) else { return };
        let mut form = form_from_profile(&profile);
        form.id = None;
        form.name = format!("{} copy", form.name);
        // A fresh profile has no history of its own yet.
        form.created_at = None;
        form.last_connected_at = None;
        form.connection_count = 0;
        self.session.borrow_mut().editor = Editor {
            is_new: true,
            form,
            ..Editor::default()
        };
        self.refresh_editor();
        if let Some(w) = self.window.upgrade() {
            w.set_show_connection_editor(true);
        }
    }

    fn close_editor(&self) {
        if let Some(w) = self.window.upgrade() {
            w.set_show_connection_editor(false);
        }
    }

    /// Runs `f` on the editor's form, then re-projects it to the window.
    /// Every field-changed callback is this one line.
    fn edit_form(&self, f: impl FnOnce(&mut ConnectionForm)) {
        f(&mut self.session.borrow_mut().editor.form);
        self.refresh_editor();
    }

    fn toggle_editor_password_visibility(&self) {
        let mut s = self.session.borrow_mut();
        s.editor.show_password = !s.editor.show_password;
        drop(s);
        self.refresh_editor();
    }

    fn set_editor_password(&self, password: String) {
        self.session.borrow_mut().editor.password = password;
        self.refresh_editor();
    }

    fn toggle_editor_favorite(&self) {
        self.edit_form(|f| f.is_favorite = !f.is_favorite);
    }

    fn set_editor_connection_string(&self, text: String) {
        {
            let mut s = self.session.borrow_mut();
            s.editor.connection_string = text;
            s.editor.connection_string_error.clear();
        }
        self.refresh_editor();
    }

    /// Fill the form from the pasted string. On success the box is cleared and
    /// the password (if the string had one) goes to the password field; on
    /// failure the text stays so it can be corrected.
    fn apply_editor_connection_string(&self) {
        {
            let mut s = self.session.borrow_mut();
            let raw = s.editor.connection_string.clone();
            match paste_connection_string(&mut s.editor.form, &raw) {
                Ok(pasted) => {
                    if let Some(p) = pasted.password {
                        s.editor.password = p;
                    }
                    s.editor.connection_string.clear();
                    s.editor.connection_string_error.clear();
                }
                Err(message) => s.editor.connection_string_error = message,
            }
        }
        self.refresh_editor();
    }

    fn set_editor_ssh_password(&self, password: String) {
        self.session.borrow_mut().editor.ssh_password = password;
        self.refresh_editor();
    }

    fn toggle_editor_ssh_password_visibility(&self) {
        let mut s = self.session.borrow_mut();
        s.editor.show_ssh_password = !s.editor.show_ssh_password;
        drop(s);
        self.refresh_editor();
    }

    /// The SSH password to hand the tunnel: only when tunnelling is on with
    /// password auth and one was typed.
    fn editor_ssh_password(&self) -> Option<String> {
        let s = self.session.borrow();
        let f = &s.editor.form;
        let wanted = f.ssh_enabled
            && f.ssh_auth_method == dbhive_core::models::connection::SshAuthMethod::Password
            && !s.editor.ssh_password.is_empty();
        wanted.then(|| s.editor.ssh_password.clone())
    }

    fn test_editor_connection(self: &Rc<Self>) {
        let (form, password) = {
            let s = self.session.borrow();
            (s.editor.form.clone(), s.editor.password.clone())
        };
        let profile = build_profile_with(
            &form,
            form.id.clone().unwrap_or_else(|| "test".to_string()),
            now_secs(),
        );
        {
            let mut s = self.session.borrow_mut();
            s.editor.testing = true;
            s.editor.test_message.clear();
        }
        self.refresh_editor();

        let ssh_password = self.editor_ssh_password();
        let state = self.state.clone();
        runtime::spawn(
            async move { test_profile(state, profile, password, ssh_password).await },
            move |result| with_ctx(|ctx| {
                let mut s = ctx.session.borrow_mut();
                s.editor.testing = false;
                match result {
                    Ok(()) => {
                        s.editor.test_ok = true;
                        s.editor.test_message = "Connected successfully.".to_string();
                    }
                    Err(e) => {
                        s.editor.test_ok = false;
                        s.editor.test_message = format!("Connection failed: {}", e);
                    }
                }
                drop(s);
                ctx.refresh_editor();
            }),
        );
    }

    fn save_editor(self: &Rc<Self>) {
        let (form, password, is_new) = {
            let s = self.session.borrow();
            (s.editor.form.clone(), s.editor.password.clone(), s.editor.is_new)
        };
        if !validate(&form).is_valid() {
            // The messages are already on screen; nothing to save yet.
            return;
        }
        let profile = build_profile(&form);
        let id = profile.id.clone();
        let name = profile.name.clone();

        self.session.borrow_mut().editor.saving = true;
        self.refresh_editor();

        let ssh_password = self.editor_ssh_password();
        let state = self.state.clone();
        runtime::spawn(
            async move { save_profile(state, profile, id, password, ssh_password).await },
            move |result| with_ctx(|ctx| {
                ctx.session.borrow_mut().editor.saving = false;
                match result {
                    Ok((_, warning)) => {
                        ctx.close_editor();
                        let verb = if is_new { "Added" } else { "Saved" };
                        ctx.toast(format!("{} {}", verb, name), Tone::Success);
                        if let Some(w) = warning {
                            ctx.toast(w, Tone::Warning);
                        }
                        ctx.refresh_connections();
                    }
                    Err(e) => {
                        ctx.refresh_editor();
                        ctx.toast(format!("Could not save: {}", e), Tone::Danger);
                    }
                }
            }),
        );
    }

    // -- Delete confirmation -------------------------------------------------

    fn confirm_delete_connection(&self, id: &str) {
        let Some(profile) = self.find_profile(id) else { return };
        self.session.borrow_mut().pending_delete = Some((id.to_string(), profile.name.clone()));
        let Some(w) = self.window.upgrade() else { return };
        w.set_confirm_title("Delete connection?".into());
        w.set_confirm_message(
            format!("\"{}\" will be removed. This cannot be undone.", profile.name).into(),
        );
        w.set_confirm_label("Delete".into());
        w.set_show_confirm(true);
    }

    fn delete_pending_connection(self: &Rc<Self>) {
        let Some((id, name)) = self.session.borrow_mut().pending_delete.take() else { return };
        let result = {
            let mut guard = self.state.lock().unwrap();
            guard.remove_profile(&id);
            guard.save_profiles_to_store()
        };
        match result {
            Ok(()) => self.toast(format!("Deleted {}", name), Tone::Neutral),
            Err(e) => self.toast(format!("Could not delete: {}", e), Tone::Danger),
        }
        self.refresh_connections();
    }

    fn cancel_pending_delete(&self) {
        self.session.borrow_mut().pending_delete = None;
    }

    // -- Workspace -----------------------------------------------------------

    /// A cloned handle to the driver behind an open connection, if it is
    /// still open. Cloning the `Arc` and dropping the lock immediately means
    /// nothing holds `AppState`'s mutex across an `.await`.
    fn open_driver(&self, connection_id: &str) -> Option<Arc<dyn DatabaseDriver>> {
        self.state.lock().unwrap().get_connection(connection_id).cloned()
    }

    fn refresh_workspace(&self) {
        let Some(w) = self.window.upgrade() else { return };
        let s = self.session.borrow();
        let ws = &s.workspace;

        w.set_workspace_connection_name(ws.connection_name.as_str().into());
        let location = match (&ws.database, &ws.schema) {
            (Some(db), Some(schema)) if schema != db => format!("{} · {}", db, schema),
            (Some(db), _) => db.clone(),
            _ => String::new(),
        };
        w.set_workspace_location_label(location.into());

        let mut names: Vec<&TableInfo> = ws.tables.iter().collect();
        names.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        let items: Vec<TableItem> = names
            .into_iter()
            .map(|t| TableItem {
                name: t.name.as_str().into(),
                row_count: t
                    .row_count
                    .map(|n| format!("~{}", workspace_model::group_thousands(n)))
                    .unwrap_or_default()
                    .into(),
            })
            .collect();
        w.set_workspace_tables(ModelRc::new(VecModel::from(items)));
        w.set_workspace_selected_table(ws.tabs.selected_table().unwrap_or("").into());
        w.set_workspace_loading_tables(ws.loading_tables);
        w.set_workspace_tables_error(ws.tables_error.as_str().into());

        let infos: Vec<TabInfo> = ws
            .tabs
            .tabs()
            .iter()
            .map(|t| TabInfo {
                title: t.title.as_str().into(),
                query: t.is_query(),
                busy: t.running,
            })
            .collect();
        w.set_workspace_tabs(ModelRc::new(VecModel::from(infos)));
        w.set_workspace_active_tab(ws.tabs.active_index().map_or(-1, |i| i as i32));

        let empty_grid = || {
            w.set_workspace_columns(ModelRc::new(VecModel::from(Vec::<crate::GridColumn>::new())));
            w.set_workspace_rows(ModelRc::new(VecModel::from(Vec::<GridRow>::new())));
            w.set_workspace_grid_width(0.0);
            w.set_workspace_row_count(0);
            w.set_workspace_visible_row_count(0);
            w.set_workspace_has_result(false);
            w.set_workspace_result_summary("".into());
            w.set_workspace_result_message("".into());
        };

        let Some(tab) = ws.tabs.active() else {
            w.set_workspace_active_is_query(false);
            if !w.get_workspace_editor_text().is_empty() {
                w.set_workspace_editor_text("".into());
            }
            w.set_workspace_editor_lines(ModelRc::new(VecModel::from(Vec::<crate::HighlightLine>::new())));
            w.set_workspace_running(false);
            w.set_workspace_rows_error("".into());
            w.set_workspace_result_title("".into());
            empty_grid();
            return;
        };

        w.set_workspace_active_is_query(tab.is_query());
        // Only write the text when it differs: the editor's own typing already
        // updated the property, and rewriting it would move the caret.
        if w.get_workspace_editor_text() != tab.sql.as_str() {
            w.set_workspace_editor_text(tab.sql.as_str().into());
        }
        let dark = w.global::<Theme>().get_dark();
        w.set_workspace_editor_lines(sql_highlight::to_model_for(&tab.sql, dark));
        w.set_workspace_running(tab.running);
        w.set_workspace_rows_error(tab.error.as_deref().unwrap_or("").into());
        w.set_workspace_result_title(
            match &tab.kind {
                TabKind::Query => "Results",
                TabKind::Table(name) => name.as_str(),
            }
            .into(),
        );

        // The grid only has something to show once a query has actually
        // landed.
        let Some(result) = &tab.result else {
            empty_grid();
            return;
        };

        let summary = result_summary(result, tab.elapsed_ms);
        w.set_workspace_has_result(true);
        w.set_workspace_result_summary(summary.as_str().into());
        // A statement that returned no columns changed data (or ran DDL):
        // say so, instead of showing an empty grid.
        let message = if result.columns.is_empty() {
            match result.rows_affected {
                Some(_) => summary.clone(),
                None => "Statement executed successfully.".to_string(),
            }
        } else {
            String::new()
        };
        w.set_workspace_result_message(message.as_str().into());

        let widths = vec![workspace_model::DEFAULT_COLUMN_WIDTH; result.columns.len()];
        let (columns, total_width) = layout_columns(&result.columns, &widths);
        w.set_workspace_columns(columns);
        w.set_workspace_grid_width(total_width);

        let order = visible_rows(result, &tab.grid);
        let visible = dbhive_core::drivers::QueryResult {
            columns: result.columns.clone(),
            rows: order.iter().map(|&i| result.rows[i].clone()).collect(),
            rows_affected: None,
        };
        let ordered = workspace_model::to_grid_rows(&visible);
        w.set_workspace_rows(ModelRc::new(LazyRows::new(std::rc::Rc::new(ordered))));
        w.set_workspace_row_count(result.rows.len() as i32);
        w.set_workspace_visible_row_count(order.len() as i32);
    }

    fn open_workspace(self: &Rc<Self>, profile: ConnectionProfile) {
        {
            let mut s = self.session.borrow_mut();
            s.workspace = Workspace {
                connection_id: profile.id.clone(),
                connection_name: profile.name.clone(),
                driver: Some(profile.driver.clone()),
                database: workspace_model::connected_database(&profile),
                loading_tables: true,
                ..Workspace::default()
            };
            // Land on an empty query, ready to type into.
            s.workspace.tabs.open_query("");
        }
        if let Some(w) = self.window.upgrade() {
            w.set_screen(Screen::Workspace);
        }
        self.refresh_workspace();
        self.load_tables();
    }

    fn leave_workspace(&self) {
        self.session.borrow_mut().workspace = Workspace::default();
        if let Some(w) = self.window.upgrade() {
            w.set_screen(Screen::Connections);
        }
        self.refresh_workspace();
    }

    fn load_tables(self: &Rc<Self>) {
        let (connection_id, driver, database) = {
            let s = self.session.borrow();
            let ws = &s.workspace;
            (ws.connection_id.clone(), ws.driver.clone(), ws.database.clone())
        };
        let Some(driver_kind) = driver else { return };
        let Some(d) = self.open_driver(&connection_id) else {
            self.session.borrow_mut().workspace.tables_error =
                "This connection is no longer open.".to_string();
            self.refresh_workspace();
            return;
        };

        let ticket = self.session.borrow_mut().workspace.tables_latest.begin();
        self.session.borrow_mut().workspace.loading_tables = true;
        self.session.borrow_mut().workspace.tables_error.clear();
        self.refresh_workspace();

        runtime::spawn(
            async move {
                let databases = d.get_databases().await.unwrap_or_default();
                let database = database
                    .or_else(|| databases.first().map(|db| db.name.clone()))
                    .unwrap_or_default();
                let schemas = if driver_kind.is_postgres_compatible() {
                    d.get_schemas(&database).await.unwrap_or_default()
                } else {
                    Vec::new()
                };
                let schema_arg =
                    workspace_model::tables_schema_arg(&driver_kind, &database, None, &schemas);
                let tables = d.get_tables(&schema_arg).await;
                (database, schema_arg, tables)
            },
            move |(database, schema, tables)| with_ctx(|ctx| {
                if !ticket.is_current() {
                    return;
                }
                let mut s = ctx.session.borrow_mut();
                let ws = &mut s.workspace;
                ws.loading_tables = false;
                ws.database = Some(database);
                ws.schema = Some(schema);
                match tables {
                    Ok(t) => ws.tables = t,
                    Err(e) => ws.tables_error = format!("Could not list tables: {}", e),
                }
                drop(s);
                ctx.refresh_workspace();
            }),
        );
    }

    /// Open (or focus) the preview tab for a table, running its query the
    /// first time.
    fn select_table(self: &Rc<Self>, name: &str) {
        let connection_id = self.session.borrow().workspace.connection_id.clone();
        let Some(d) = self.open_driver(&connection_id) else {
            self.toast("This connection is no longer open.", Tone::Danger);
            return;
        };
        let sql = format!(
            "SELECT * FROM {} LIMIT {}",
            d.quote_identifier(name),
            PREVIEW_ROW_LIMIT
        );
        let (id, created) = self.session.borrow_mut().workspace.tabs.open_table(name, sql);
        self.refresh_workspace();
        if created {
            // A preview is a SELECT the app wrote, so it skips the guard.
            self.run_tab(id, false);
        }
    }

    fn new_query_tab(&self) {
        self.session.borrow_mut().workspace.tabs.open_query("");
        self.refresh_workspace();
    }

    fn select_tab(&self, index: usize) {
        {
            let mut s = self.session.borrow_mut();
            let tabs = &mut s.workspace.tabs;
            if let Some(id) = tabs.id_at(index) {
                tabs.select(id);
            }
        }
        self.refresh_workspace();
    }

    /// Close a tab, asking first when it is a query with SQL in it.
    fn close_tab(&self, index: usize) {
        let (id, ask, title) = {
            let s = self.session.borrow();
            let tabs = &s.workspace.tabs;
            let Some(id) = tabs.id_at(index) else { return };
            (
                id,
                tabs.needs_close_confirmation(id),
                tabs.get(id).map(|t| t.title.clone()).unwrap_or_default(),
            )
        };
        if !ask {
            self.session.borrow_mut().workspace.tabs.close(id);
            self.refresh_workspace();
            return;
        }
        self.session.borrow_mut().pending_action = Some(PendingAction::CloseTab(id));
        self.show_confirm(
            "Close this tab?",
            &format!("\"{title}\" has SQL in it that is not saved anywhere."),
            "Close tab",
        );
    }

    fn edit_active_sql(&self, text: String) {
        if let Some(tab) = self.session.borrow_mut().workspace.tabs.active_mut() {
            tab.sql = text;
        }
        self.refresh_workspace();
    }

    fn run_active_tab(self: &Rc<Self>) {
        let id = self.session.borrow().workspace.tabs.active().map(|t| t.id);
        if let Some(id) = id {
            self.run_tab(id, true);
        }
    }

    /// Run a tab's SQL. With `guard`, a statement that can destroy data asks
    /// for confirmation first.
    fn run_tab(self: &Rc<Self>, id: u64, guard: bool) {
        let (connection_id, connection_name, database, sql) = {
            let s = self.session.borrow();
            let ws = &s.workspace;
            let Some(tab) = ws.tabs.get(id) else { return };
            (
                ws.connection_id.clone(),
                ws.connection_name.clone(),
                ws.database.clone().unwrap_or_default(),
                tab.sql.clone(),
            )
        };
        if sql.trim().is_empty() {
            return;
        }

        if guard {
            // "Confirm destructive queries" in Settings turns the guard off.
            let warnings = destructive_warnings(&self.app_settings().query, &sql);
            if !warnings.is_empty() {
                let mut lines: Vec<String> =
                    warnings.iter().take(3).map(|w| format!("• {}", w.description)).collect();
                if warnings.len() > 3 {
                    lines.push(format!("• and {} more", warnings.len() - 3));
                }
                self.session.borrow_mut().pending_action = Some(PendingAction::RunAnyway(id));
                self.show_confirm("Run this statement?", &lines.join("\n"), "Run anyway");
                return;
            }
        }

        let Some(d) = self.open_driver(&connection_id) else {
            if let Some(tab) = self.session.borrow_mut().workspace.tabs.get_mut(id) {
                tab.error = Some("This connection is no longer open.".to_string());
            }
            self.refresh_workspace();
            return;
        };
        let Some(seq) = self.session.borrow_mut().workspace.tabs.begin_run(id) else {
            return;
        };

        // The activity log is separate from query history: the activity
        // monitor reads it.
        let log = dbhive_core::models::QueryLog::new(
            uuid::Uuid::new_v4().to_string(),
            connection_id.clone(),
            connection_name.clone(),
            (!database.is_empty()).then(|| database.clone()),
            sql.clone(),
        );
        let log_id = log.id.clone();
        dbhive_core::service_monitoring::record_query_start(&self.state, log);
        self.refresh_workspace();

        let run_sql = sql.clone();
        let query_settings = self.app_settings().query;
        runtime::spawn(
            async move {
                let started = std::time::Instant::now();
                // Settings > Query timeout. Dropping the future stops
                // waiting; the server may finish the statement on its own.
                let out = match query_timeout(&query_settings) {
                    Some(limit) => match tokio::time::timeout(limit, d.execute_query(&run_sql)).await {
                        Ok(result) => result,
                        Err(_) => Err(dbhive_core::models::DbError::QueryError(timeout_message(
                            &query_settings,
                        ))),
                    },
                    None => d.execute_query(&run_sql).await,
                };
                (out, started.elapsed().as_millis() as u64)
            },
            move |(out, elapsed)| with_ctx(|ctx| {
                let outcome = out.map_err(|e| e.to_string());
                ctx.record_run(&connection_id, &connection_name, &database, &sql, &log_id, &outcome, elapsed);
                {
                    let mut s = ctx.session.borrow_mut();
                    s.workspace.tabs.finish_run(id, seq, outcome, elapsed);
                }
                ctx.refresh_workspace();
            }),
        );
    }

    /// Write a finished run to the activity log and to query history. Done
    /// even if the tab was closed meanwhile: the query did run.
    #[allow(clippy::too_many_arguments)]
    fn record_run(
        &self,
        connection_id: &str,
        connection_name: &str,
        database: &str,
        sql: &str,
        log_id: &str,
        outcome: &Result<dbhive_core::drivers::QueryResult, String>,
        elapsed_ms: u64,
    ) {
        let entry = dbhive_core::models::QueryHistory::new(
            connection_id.to_string(),
            connection_name.to_string(),
            database.to_string(),
            sql.to_string(),
            chrono::Utc::now().to_rfc3339(),
        );
        let entry = match outcome {
            Ok(r) => {
                dbhive_core::service_monitoring::record_query_complete(
                    &self.state,
                    log_id,
                    elapsed_ms,
                    Some(r.rows.len() as u64),
                );
                entry.with_success(elapsed_ms, Some(r.rows.len() as u64))
            }
            Err(e) => {
                dbhive_core::service_monitoring::record_query_error(
                    &self.state,
                    log_id,
                    elapsed_ms,
                    e.clone(),
                );
                entry.with_error(e.clone(), Some(elapsed_ms))
            }
        };
        // Settings > Query Execution: history can be switched off, and is
        // trimmed to its size when written. The activity log above is not
        // history and is always kept.
        let query_settings = self.app_settings().query;
        if !query_settings.auto_save_history {
            return;
        }
        let mut snapshot = {
            let mut guard = self.state.lock().unwrap();
            guard.add_history(entry);
            guard.get_all_history()
        };
        trim_history(&mut snapshot, &query_settings);
        // Persisting history never delays showing the result, and a failed
        // write is not worth interrupting the user for.
        runtime::spawn(
            async move { AppState::save_history_to_store(&snapshot) },
            |saved| {
                if let Err(e) = saved {
                    eprintln!("Warning: failed to persist query history: {}", e);
                }
            },
        );
    }

    fn show_confirm(&self, title: &str, message: &str, label: &str) {
        let Some(w) = self.window.upgrade() else { return };
        w.set_confirm_title(title.into());
        w.set_confirm_message(message.into());
        w.set_confirm_label(label.into());
        w.set_show_confirm(true);
    }

    /// The confirmation dialog was accepted: do whichever action it was for.
    fn confirm_accepted(self: &Rc<Self>) {
        let action = self.session.borrow_mut().pending_action.take();
        match action {
            Some(PendingAction::CloseTab(id)) => {
                self.session.borrow_mut().workspace.tabs.close(id);
                self.refresh_workspace();
            }
            Some(PendingAction::RunAnyway(id)) => self.run_tab(id, false),
            Some(PendingAction::ResetSettings) => self.reset_settings(),
            Some(PendingAction::DiscardSettings) => self.discard_settings_and_close(),
            None => self.delete_pending_connection(),
        }
    }

    fn confirm_cancelled(&self) {
        self.session.borrow_mut().pending_action = None;
        self.cancel_pending_delete();
    }

    fn set_workspace_filter(&self, filter: String) {
        if let Some(tab) = self.session.borrow_mut().workspace.tabs.active_mut() {
            tab.grid.filter = filter;
        }
        self.refresh_workspace();
    }

    fn sort_workspace_by(&self, column: usize) {
        if let Some(tab) = self.session.borrow_mut().workspace.tabs.active_mut() {
            tab.grid.sort = match tab.grid.sort {
                Some((c, desc)) if c == column => Some((c, !desc)),
                _ => Some((column, false)),
            };
        }
        self.refresh_workspace();
    }

    fn toggle_favorite(self: &Rc<Self>, id: &str) {
        let result = {
            let mut guard = self.state.lock().unwrap();
            match guard.get_profile_mut(id) {
                Some(p) => {
                    p.is_favorite = !p.is_favorite;
                    guard.save_profiles_to_store()
                }
                None => Ok(()),
            }
        };
        if let Err(e) = result {
            self.toast(format!("Could not save: {}", e), Tone::Danger);
        }
        self.refresh_connections();
    }
}

thread_local! {
    /// The controller lives on the UI thread. Async completions reach it
    /// through here, because `invoke_from_event_loop` needs a `Send` closure
    /// and `Rc` is not.
    static CTX: RefCell<Option<Rc<Ctx>>> = const { RefCell::new(None) };
}

fn with_ctx(f: impl FnOnce(&Rc<Ctx>)) {
    if let Some(ctx) = CTX.with(|c| c.borrow().clone()) {
        f(&ctx);
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn run() -> Result<(), slint::PlatformError> {
    let window = build()?;
    let result = window.run();
    CTX.with(|c| c.borrow_mut().take());
    result
}

/// Build the window with every handler wired, without running it. `run`
/// blocks on it; tests drive the returned window directly, with the same
/// controller behind it.
pub fn build() -> Result<AppWindow, slint::PlatformError> {
    let window = AppWindow::new()?;
    let state = Arc::new(Mutex::new(AppState::new()));

    let ctx = Rc::new(Ctx {
        window: window.as_weak(),
        state: state.clone(),
        session: RefCell::new(Session::default()),
    });

    if let Err(e) = dbhive_core::service::load_persisted(&state) {
        ctx.session.borrow_mut().error = format!("Could not load saved data: {}", e);
    }

    window.global::<Theme>().set_dark(true);
    window.set_screen(Screen::Connections);
    window.set_app_version(env!("CARGO_PKG_VERSION").into());

    {
        let ctx = ctx.clone();
        window.on_connection_search_changed(move |text| {
            ctx.session.borrow_mut().search = text.to_string();
            ctx.refresh_connections();
        });
    }
    {
        let ctx = ctx.clone();
        window.on_connect(move |id| ctx.connect(id.to_string()));
    }
    {
        let ctx = ctx.clone();
        window.on_disconnect(move |id| ctx.disconnect(id.to_string()));
    }
    {
        let ctx = ctx.clone();
        window.on_toggle_favorite(move |id| ctx.toggle_favorite(&id));
    }
    {
        let ctx = ctx.clone();
        window.on_dismiss_connection_error(move || {
            ctx.session.borrow_mut().error.clear();
            ctx.refresh_connections();
        });
    }
    {
        let ctx = ctx.clone();
        window.on_dismiss_toast(move |id| ctx.dismiss_toast(id));
    }
    {
        let ctx = ctx.clone();
        // Flips light/dark, saves it as the theme mode, and recomputes the
        // editor's syntax colours (see settings.rs).
        window.on_toggle_theme(move || ctx.toggle_theme_mode());
    }

    // -- Connection editor and delete confirmation --------------------------
    {
        let ctx = ctx.clone();
        window.on_new_connection(move || ctx.open_new_connection());
    }
    {
        let ctx = ctx.clone();
        window.on_edit_connection(move |id| ctx.open_edit_connection(&id));
    }
    {
        let ctx = ctx.clone();
        window.on_duplicate_connection(move |id| ctx.open_duplicate_connection(&id));
    }
    {
        let ctx = ctx.clone();
        window.on_delete_connection(move |id| ctx.confirm_delete_connection(&id));
    }
    {
        let ctx = ctx.clone();
        window.on_confirm_accepted(move || ctx.confirm_accepted());
    }
    {
        let ctx = ctx.clone();
        window.on_confirm_cancelled(move || ctx.confirm_cancelled());
    }
    {
        let ctx = ctx.clone();
        window.on_editor_name_changed(move |t| ctx.edit_form(|f| f.name = t.to_string()));
    }
    {
        let ctx = ctx.clone();
        window.on_editor_driver_selected(move |i| {
            ctx.edit_form(|f| f.set_driver(editor_view::driver_at(i)))
        });
    }
    {
        let ctx = ctx.clone();
        window.on_editor_host_changed(move |t| ctx.edit_form(|f| f.host = t.to_string()));
    }
    {
        let ctx = ctx.clone();
        window.on_editor_port_changed(move |t| ctx.edit_form(|f| f.port = t.to_string()));
    }
    {
        let ctx = ctx.clone();
        window.on_editor_username_changed(move |t| ctx.edit_form(|f| f.username = t.to_string()));
    }
    {
        let ctx = ctx.clone();
        window.on_editor_password_changed(move |t| ctx.set_editor_password(t.to_string()));
    }
    {
        let ctx = ctx.clone();
        window.on_editor_toggle_password_visibility(move || {
            ctx.toggle_editor_password_visibility()
        });
    }
    {
        let ctx = ctx.clone();
        window.on_editor_database_changed(move |t| ctx.edit_form(|f| f.database = t.to_string()));
    }
    {
        let ctx = ctx.clone();
        window.on_editor_ssl_selected(move |i| {
            ctx.edit_form(|f| f.ssl_mode = editor_view::ssl_mode_at(i))
        });
    }
    {
        let ctx = ctx.clone();
        window.on_editor_environment_selected(move |i| {
            ctx.edit_form(|f| f.environment = editor_view::environment_at(i))
        });
    }
    {
        let ctx = ctx.clone();
        window.on_editor_description_changed(move |t| {
            ctx.edit_form(|f| f.description = t.to_string())
        });
    }
    {
        let ctx = ctx.clone();
        window.on_editor_toggle_favorite(move || ctx.toggle_editor_favorite());
    }
    {
        let ctx = ctx.clone();
        window.on_editor_connection_string_changed(move |t| {
            ctx.set_editor_connection_string(t.to_string())
        });
    }
    {
        let ctx = ctx.clone();
        window.on_editor_apply_connection_string(move || ctx.apply_editor_connection_string());
    }
    {
        let ctx = ctx.clone();
        window.on_editor_toggle_ssh(move || ctx.edit_form(|f| f.ssh_enabled = !f.ssh_enabled));
    }
    {
        let ctx = ctx.clone();
        window.on_editor_ssh_host_changed(move |t| ctx.edit_form(|f| f.ssh_host = t.to_string()));
    }
    {
        let ctx = ctx.clone();
        window.on_editor_ssh_port_changed(move |t| ctx.edit_form(|f| f.ssh_port = t.to_string()));
    }
    {
        let ctx = ctx.clone();
        window.on_editor_ssh_username_changed(move |t| {
            ctx.edit_form(|f| f.ssh_username = t.to_string())
        });
    }
    {
        let ctx = ctx.clone();
        window.on_editor_ssh_auth_selected(move |i| {
            ctx.edit_form(|f| f.ssh_auth_method = editor_view::ssh_auth_at(i))
        });
    }
    {
        let ctx = ctx.clone();
        window.on_editor_ssh_key_path_changed(move |t| {
            ctx.edit_form(|f| f.ssh_private_key_path = t.to_string())
        });
    }
    {
        let ctx = ctx.clone();
        window.on_editor_ssh_password_changed(move |t| ctx.set_editor_ssh_password(t.to_string()));
    }
    {
        let ctx = ctx.clone();
        window.on_editor_toggle_ssh_password_visibility(move || {
            ctx.toggle_editor_ssh_password_visibility()
        });
    }
    {
        let ctx = ctx.clone();
        window.on_editor_color_picked(move |i| {
            ctx.edit_form(|f| f.color = editor_view::color_at(i))
        });
    }
    {
        let ctx = ctx.clone();
        window.on_editor_test_connection(move || ctx.test_editor_connection());
    }
    {
        let ctx = ctx.clone();
        window.on_editor_save(move || ctx.save_editor());
    }
    {
        let ctx = ctx.clone();
        window.on_editor_cancel(move || ctx.close_editor());
    }

    // -- Workspace ------------------------------------------------------
    {
        let ctx = ctx.clone();
        window.on_workspace_back(move || ctx.leave_workspace());
    }
    {
        let ctx = ctx.clone();
        window.on_workspace_refresh_tables(move || ctx.load_tables());
    }
    {
        let ctx = ctx.clone();
        window.on_workspace_select_table(move |name| ctx.select_table(&name));
    }
    {
        let ctx = ctx.clone();
        window.on_workspace_filter_changed(move |t| ctx.set_workspace_filter(t.to_string()));
    }
    {
        let ctx = ctx.clone();
        window.on_workspace_header_clicked(move |i| ctx.sort_workspace_by(i as usize));
    }
    {
        let ctx = ctx.clone();
        window.on_workspace_tab_selected(move |i| ctx.select_tab(i as usize));
    }
    {
        let ctx = ctx.clone();
        window.on_workspace_tab_closed(move |i| ctx.close_tab(i as usize));
    }
    {
        let ctx = ctx.clone();
        window.on_workspace_new_query(move || ctx.new_query_tab());
    }
    {
        let ctx = ctx.clone();
        window.on_workspace_editor_edited(move |t| ctx.edit_active_sql(t.to_string()));
    }
    {
        let ctx = ctx.clone();
        window.on_workspace_run_query(move || ctx.run_active_tab());
    }
    {
        let ctx = ctx.clone();
        window.on_workspace_retry(move || ctx.run_active_tab());
    }

    settings::wire(&window, &ctx);
    // After the load, so the saved theme, accent and editor font are applied
    // before the first frame.
    ctx.init_settings();

    ctx.refresh_connections();
    CTX.with(|c| *c.borrow_mut() = Some(ctx));
    Ok(window)
}
