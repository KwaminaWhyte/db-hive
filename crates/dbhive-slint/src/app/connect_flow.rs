//! The connection side of the controller: the password prompt, folder
//! grouping in the connections list, the editor's folder / local port /
//! "saved in keychain" extras, Browse buttons, and the Connection Lost
//! overlay.
//!
//! The decisions are `dbhive_ui_model::{connect_flow, connection_folders,
//! file_pick}`; this file runs them and projects the result into the
//! `PasswordPromptState`, `ConnectionLostState`, `ConnectionsState`,
//! `ConnectionEditorState` and `FileBrowse` globals.
//!
//! Secrets. The typed password lives in `PasswordPrompt::password` until it
//! is handed to `service::connect`; it is never in a toast, a log line or a
//! message. Keychain lookups run off the UI thread through a [`SecretStore`],
//! which a test replaces with [`use_secret_store`] so it never touches the
//! developer's keychain.
//!
//! A child module of `app` so it can reach the controller's private state.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use dbhive_core::credentials::CredentialManager;
use dbhive_core::models::ConnectionProfile;
use dbhive_core::storage::JsonStore;
use dbhive_ui_model::connect_flow::{
    is_connection_lost, needs_password, ConnectionLost, PasswordPrompt,
};
use dbhive_ui_model::connection_editor::password_label;
use dbhive_ui_model::connection_folders::{self as folders, Entry};
use dbhive_ui_model::file_pick::{self, PickRequest};
use slint::{ComponentHandle, ModelRc, VecModel};

use super::{with_ctx, Ctx};
use crate::connection_list::{self, EnvTone};
use crate::file_dialog::{self, FilePicker};
use crate::{
    runtime, AppWindow, BackupState, BrowseTarget, ConnectionEditorState, ConnectionFormData,
    ConnectionItem, ConnectionLostState, ConnectionsState, FileBrowse, ImportExportState,
    PasswordPromptState, ResultsState, Tone,
};

/// Where the collapsed folders are remembered.
const FOLDER_STORE: &str = "connection_folders.json";
const COLLAPSED_KEY: &str = "collapsed";
/// Folder suggestion chips in the editor.
const MAX_FOLDER_CHIPS: usize = 6;

/// Whether the keychain holds a profile's password, and forgetting it.
pub trait SecretStore: Send + Sync {
    fn has_password(&self, profile_id: &str) -> bool;
    fn forget_password(&self, profile_id: &str);
}

/// The OS keychain, through `CredentialManager`.
pub struct KeyringSecrets;

impl SecretStore for KeyringSecrets {
    fn has_password(&self, profile_id: &str) -> bool {
        CredentialManager::has_password(profile_id).unwrap_or(false)
    }
    fn forget_password(&self, profile_id: &str) {
        if let Err(e) = CredentialManager::delete_password(profile_id) {
            // The error names the profile id, never the secret.
            eprintln!("Warning: could not remove a password from the keychain: {e}");
        }
    }
}

/// An in-memory store for tests: which ids "have" a password, and which
/// were asked to forget theirs.
#[derive(Default)]
pub struct MemorySecrets {
    pub stored: std::sync::Mutex<HashSet<String>>,
    pub forgotten: std::sync::Mutex<Vec<String>>,
}

impl SecretStore for MemorySecrets {
    fn has_password(&self, profile_id: &str) -> bool {
        self.stored.lock().unwrap().contains(profile_id)
    }
    fn forget_password(&self, profile_id: &str) {
        self.stored.lock().unwrap().remove(profile_id);
        self.forgotten.lock().unwrap().push(profile_id.to_string());
    }
}

thread_local! {
    static SECRETS_OVERRIDE: RefCell<Option<Arc<dyn SecretStore>>> = const { RefCell::new(None) };
}

/// Use `store` instead of the OS keychain for windows built on this thread.
/// A test calls this before `app::build`.
pub fn use_secret_store(store: Arc<dyn SecretStore>) {
    SECRETS_OVERRIDE.with(|s| *s.borrow_mut() = Some(store));
}

pub(super) struct FlowSession {
    secrets: Arc<dyn SecretStore>,
    picker: Arc<dyn FilePicker>,
    prompt: Option<PasswordPrompt>,
    /// The username the prompt names, for its message.
    prompt_user: String,
    /// A keychain lookup before connecting is in flight for this id.
    checking: Option<String>,
    /// Profiles whose password must leave the keychain once connected
    /// ("Remember" was unticked; `service::connect` saves it regardless).
    forget_after_connect: HashSet<String>,
    collapsed: HashSet<String>,
    collapsed_loaded: bool,
    lost: Option<ConnectionLost>,
    /// `(profile id, stored)` from the editor's last keychain lookup.
    editor_stored: Option<(String, bool)>,
    browsing: bool,
}

impl Default for FlowSession {
    fn default() -> Self {
        Self {
            secrets: SECRETS_OVERRIDE
                .with(|s| s.borrow().clone())
                .unwrap_or_else(|| Arc::new(KeyringSecrets)),
            picker: file_dialog::current(),
            prompt: None,
            prompt_user: String::new(),
            checking: None,
            forget_after_connect: HashSet::new(),
            collapsed: HashSet::new(),
            collapsed_loaded: false,
            lost: None,
            editor_stored: None,
            browsing: false,
        }
    }
}

fn read_collapsed() -> HashSet<String> {
    let Ok(store) = JsonStore::open(FOLDER_STORE) else {
        return HashSet::new();
    };
    store
        .get(COLLAPSED_KEY)
        .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
        .unwrap_or_default()
        .into_iter()
        .collect()
}

fn write_collapsed(collapsed: Vec<String>) -> Result<(), String> {
    let mut store = JsonStore::open(FOLDER_STORE).map_err(|e| e.to_string())?;
    store.set(COLLAPSED_KEY, serde_json::json!(collapsed));
    store.save().map_err(|e| e.to_string())
}

impl Ctx {
    // -- Connecting --------------------------------------------------------

    /// A saved connection was clicked. Connect straight away when a password
    /// is not needed or is known; otherwise ask for it first.
    pub(super) fn connect_begin(self: &Rc<Self>, id: String) {
        {
            let s = self.session.borrow();
            if s.connecting.is_some() || s.flow.checking.is_some() || s.flow.prompt.is_some() {
                return;
            }
        }
        let Some(profile) = self.find_profile(&id) else {
            return self.connect_with_password(id, String::new());
        };
        let cached = self
            .state
            .lock()
            .unwrap()
            .connection_passwords
            .contains_key(&id);
        if cached || !needs_password(&profile) {
            return self.connect_with_password(id, String::new());
        }

        let secrets = {
            let mut s = self.session.borrow_mut();
            s.flow.checking = Some(id.clone());
            s.flow.secrets.clone()
        };
        self.refresh_connections();
        let lookup_id = id.clone();
        runtime::spawn(
            self.ui_id,
            async move {
                tokio::task::spawn_blocking(move || secrets.has_password(&lookup_id))
                    .await
                    .unwrap_or(false)
            },
            move |stored| {
                with_ctx(|ctx| {
                    ctx.session.borrow_mut().flow.checking = None;
                    if stored {
                        ctx.connect_with_password(id, String::new());
                    } else {
                        ctx.refresh_connections();
                        ctx.prompt_open(&profile);
                    }
                })
            },
        );
    }

    /// A connect attempt finished. Applies an unticked "Remember".
    pub(super) fn connect_flow_finished(&self, id: &str, ok: bool) {
        let forget = self
            .session
            .borrow_mut()
            .flow
            .forget_after_connect
            .remove(id);
        if ok && forget {
            let secrets = self.session.borrow().flow.secrets.clone();
            let id = id.to_string();
            runtime::spawn(
                self.ui_id,
                async move {
                    let _ = tokio::task::spawn_blocking(move || secrets.forget_password(&id)).await;
                },
                |_| {},
            );
        }
    }

    // -- Password prompt -----------------------------------------------------

    fn prompt_open(&self, profile: &ConnectionProfile) {
        {
            let mut s = self.session.borrow_mut();
            s.flow.prompt = Some(PasswordPrompt::new(profile));
            s.flow.prompt_user = profile.username.clone();
        }
        if let Some(w) = self.window.upgrade() {
            let g = w.global::<PasswordPromptState>();
            g.set_password("".into());
            g.set_field_label(password_label(&profile.driver).into());
        }
        self.prompt_project();
    }

    fn prompt_project(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let g = w.global::<PasswordPromptState>();
        let s = self.session.borrow();
        match &s.flow.prompt {
            Some(p) => {
                g.set_title(p.title().into());
                g.set_message(p.message(&s.flow.prompt_user).into());
                g.set_show_text(p.show_text);
                g.set_remember(p.remember);
                g.set_visible(true);
            }
            None => {
                g.set_visible(false);
                // Nothing typed stays in the window once the dialog is gone.
                g.set_password("".into());
            }
        }
    }

    fn prompt_edit(&self, f: impl FnOnce(&mut PasswordPrompt)) {
        if let Some(p) = self.session.borrow_mut().flow.prompt.as_mut() {
            f(p);
        }
        self.prompt_project();
    }

    fn prompt_cancel(&self) {
        self.session.borrow_mut().flow.prompt = None;
        self.prompt_project();
    }

    fn prompt_submit(self: &Rc<Self>) {
        let Some(p) = self.session.borrow_mut().flow.prompt.take() else {
            return;
        };
        if !p.remember {
            self.session
                .borrow_mut()
                .flow
                .forget_after_connect
                .insert(p.profile_id.clone());
        }
        self.prompt_project();
        self.connect_with_password(p.profile_id, p.password);
    }

    // -- Connections list ------------------------------------------------------

    /// The list's rows, grouped by folder, and the header's summary line.
    pub(super) fn connection_items(&self, profiles: &[ConnectionProfile]) -> Vec<ConnectionItem> {
        if !self.session.borrow().flow.collapsed_loaded {
            let loaded = read_collapsed();
            let mut s = self.session.borrow_mut();
            s.flow.collapsed = loaded;
            s.flow.collapsed_loaded = true;
        }
        let s = self.session.borrow();
        let visible = connection_list::visible(profiles, &s.search);
        let searching = !s.search.trim().is_empty();
        let entries = folders::group(&visible, profiles, &s.flow.collapsed, searching);

        let items = entries
            .into_iter()
            .map(|e| match e {
                Entry::Folder {
                    key,
                    label,
                    count,
                    collapsed,
                } => ConnectionItem {
                    id: "".into(),
                    name: label.into(),
                    host: "".into(),
                    initials: "".into(),
                    tint: slint::Color::default(),
                    env_label: "".into(),
                    env_tone: Tone::Neutral,
                    favorite: false,
                    connected: false,
                    connecting: false,
                    is_folder: true,
                    folder: key.into(),
                    count: count as i32,
                    collapsed,
                },
                Entry::Connection(p) => {
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
                        connecting: s.connecting.as_deref() == Some(p.id.as_str())
                            || s.flow.checking.as_deref() == Some(p.id.as_str()),
                        is_folder: false,
                        folder: "".into(),
                        count: 0,
                        collapsed: false,
                    }
                }
            })
            .collect();

        if let Some(w) = self.window.upgrade() {
            let summary = folders::summary(profiles.len(), folders::folder_names(profiles).len());
            w.global::<ConnectionsState>().set_summary(summary.into());
        }
        items
    }

    fn toggle_folder(&self, key: &str) {
        let snapshot = {
            let mut s = self.session.borrow_mut();
            folders::toggle(&mut s.flow.collapsed, key);
            let mut v: Vec<String> = s.flow.collapsed.iter().cloned().collect();
            v.sort();
            v
        };
        self.refresh_connections();
        runtime::spawn(
            self.ui_id,
            async move { write_collapsed(snapshot) },
            |saved| {
                if let Err(e) = saved {
                    eprintln!("Warning: could not remember collapsed folders: {e}");
                }
            },
        );
    }

    // -- Connection editor extras ----------------------------------------------

    /// Fill the parts of the editor's data that need more than the form.
    pub(super) fn editor_extras(&self, data: &mut ConnectionFormData) {
        let names = folders::folder_names(&self.profiles());
        data.folder_options = ModelRc::new(VecModel::from(
            crate::connection_editor::folder_options(&names, MAX_FOLDER_CHIPS),
        ));
        let s = self.session.borrow();
        let id = s.editor.form.id.as_deref();
        data.password_stored = matches!(
            (&s.flow.editor_stored, id),
            (Some((stored_id, true)), Some(id)) if stored_id == id
        );
    }

    /// The editor opened on a saved profile: look up whether its password is
    /// in the keychain, for the "Saved in keychain" badge.
    pub(super) fn editor_check_stored(&self, id: &str) {
        let secrets = {
            let mut s = self.session.borrow_mut();
            s.flow.editor_stored = None;
            s.flow.secrets.clone()
        };
        let id = id.to_string();
        runtime::spawn(
            self.ui_id,
            async move {
                let lookup = id.clone();
                let stored = tokio::task::spawn_blocking(move || secrets.has_password(&lookup))
                    .await
                    .unwrap_or(false);
                (id, stored)
            },
            |(id, stored)| {
                with_ctx(|ctx| {
                    ctx.session.borrow_mut().flow.editor_stored = Some((id, stored));
                    ctx.refresh_editor();
                })
            },
        );
    }

    // -- Browse buttons ----------------------------------------------------------

    fn browse_request(&self, w: &AppWindow, target: BrowseTarget) -> PickRequest {
        let form = self.session.borrow().editor.form.clone();
        let ie = w.global::<ImportExportState>();
        match target {
            BrowseTarget::SqliteFile => file_pick::sqlite_file(&form.database),
            BrowseTarget::SshKey => file_pick::ssh_key(&form.ssh_private_key_path),
            BrowseTarget::ImportFile => file_pick::data_file(&ie.get_import_path()),
            BrowseTarget::SqlFile => file_pick::sql_file(&ie.get_sql_path()),
            BrowseTarget::ExportFile => file_pick::save_as("Export table", &ie.get_export_path()),
            BrowseTarget::DumpFile => file_pick::save_as("Save SQL dump", &ie.get_dump_path()),
            BrowseTarget::ResultsExport => file_pick::save_as(
                "Export results",
                &w.global::<ResultsState>().get_export_path(),
            ),
            BrowseTarget::BackupDir => {
                let b = w.global::<BackupState>();
                let typed = b.get_output_dir();
                let start = if typed.trim().is_empty() {
                    b.get_directory()
                } else {
                    typed
                };
                file_pick::folder("Choose a backup folder", &start)
            }
        }
    }

    fn browse(self: &Rc<Self>, target: BrowseTarget) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        if self.session.borrow().flow.browsing {
            return;
        }
        let request = self.browse_request(&w, target);
        let picker = {
            let mut s = self.session.borrow_mut();
            s.flow.browsing = true;
            s.flow.picker.clone()
        };
        w.global::<FileBrowse>().set_busy(true);
        runtime::spawn(self.ui_id, picker.pick(request), move |chosen| {
            with_ctx(|ctx| {
                ctx.session.borrow_mut().flow.browsing = false;
                if let Some(w) = ctx.window.upgrade() {
                    w.global::<FileBrowse>().set_busy(false);
                }
                if let Some(path) = chosen {
                    ctx.browse_apply(target, path.to_string_lossy().into_owned());
                }
            })
        });
    }

    /// Put a chosen path into its field, through the field's own edited
    /// handler so the owning panel sees it exactly as if it were typed.
    fn browse_apply(&self, target: BrowseTarget, path: String) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let p: slint::SharedString = path.as_str().into();
        match target {
            BrowseTarget::SqliteFile => self.edit_form(|f| f.database = path),
            BrowseTarget::SshKey => self.edit_form(|f| f.ssh_private_key_path = path),
            BrowseTarget::ImportFile => {
                let g = w.global::<ImportExportState>();
                g.set_import_path(p.clone());
                g.invoke_import_path_edited(p);
            }
            BrowseTarget::SqlFile => {
                let g = w.global::<ImportExportState>();
                g.set_sql_path(p.clone());
                g.invoke_sql_path_edited(p);
            }
            BrowseTarget::ExportFile => {
                let g = w.global::<ImportExportState>();
                g.set_export_path(p.clone());
                g.invoke_export_path_edited(p);
            }
            BrowseTarget::DumpFile => {
                let g = w.global::<ImportExportState>();
                g.set_dump_path(p.clone());
                g.invoke_dump_path_edited(p);
            }
            BrowseTarget::ResultsExport => {
                let g = w.global::<ResultsState>();
                g.set_export_path(p.clone());
                g.invoke_export_path_edited(p);
            }
            BrowseTarget::BackupDir => {
                let g = w.global::<BackupState>();
                g.set_output_dir(p.clone());
                g.invoke_output_dir_edited(p);
            }
        }
    }

    // -- Connection lost ---------------------------------------------------------

    /// A run or page load on `connection_id` failed with `error`. When the
    /// error means the connection itself is gone, and the workspace is still
    /// on that connection, show the Connection Lost overlay.
    pub(super) fn connection_lost_check(&self, connection_id: &str, error: &str) {
        if !is_connection_lost(error) {
            return;
        }
        {
            let mut s = self.session.borrow_mut();
            if s.workspace.connection_id != connection_id || s.flow.lost.is_some() {
                return;
            }
            let name = s.workspace.connection_name.clone();
            s.flow.lost = Some(ConnectionLost::new(connection_id, &name, error));
        }
        self.lost_project();
    }

    pub(super) fn connect_flow_modal_open(&self) -> bool {
        let s = self.session.borrow();
        s.flow.lost.is_some() || s.flow.prompt.is_some()
    }

    fn lost_project(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let g = w.global::<ConnectionLostState>();
        let s = self.session.borrow();
        match &s.flow.lost {
            Some(l) => {
                g.set_title(l.title().into());
                g.set_error(l.shown_error().into());
                g.set_reconnecting(l.reconnecting);
                g.set_visible(true);
            }
            None => g.set_visible(false),
        }
    }

    /// Drop the dead connection and open a new one with the same profile.
    /// The workspace and its tabs stay; the new driver takes the same id.
    fn lost_reconnect(self: &Rc<Self>) {
        let id = {
            let mut s = self.session.borrow_mut();
            let Some(l) = s.flow.lost.as_mut() else {
                return;
            };
            if !l.begin_reconnect() {
                return;
            }
            l.connection_id.clone()
        };
        self.lost_project();
        let state = self.state.clone();
        let worker_id = id.clone();
        runtime::spawn(
            self.ui_id,
            async move {
                // Closing a dead socket can itself fail; that is expected.
                let _ = dbhive_core::service::disconnect(&state, &worker_id).await;
                dbhive_core::service::connect(&state, worker_id, String::new(), None).await
            },
            move |result| {
                with_ctx(|ctx| match result {
                    Ok(_) => {
                        ctx.session.borrow_mut().flow.lost = None;
                        ctx.lost_project();
                        let name = ctx.session.borrow().workspace.connection_name.clone();
                        ctx.toast(format!("Reconnected to {name}"), Tone::Success);
                        ctx.load_tables();
                    }
                    Err(e) => {
                        if let Some(l) = ctx.session.borrow_mut().flow.lost.as_mut() {
                            l.reconnect_failed(&e.to_string());
                        }
                        ctx.lost_project();
                    }
                })
            },
        );
    }

    /// Give up on the connection and go back to the list.
    fn lost_go_connections(self: &Rc<Self>) {
        let Some(lost) = self.session.borrow_mut().flow.lost.take() else {
            return;
        };
        self.lost_project();
        self.session
            .borrow_mut()
            .connected
            .remove(&lost.connection_id);
        self.leave_workspace();
        self.refresh_connections();
        let state = self.state.clone();
        runtime::spawn(
            self.ui_id,
            async move { dbhive_core::service::disconnect(&state, &lost.connection_id).await },
            |_| {},
        );
    }
}

pub(super) fn wire(window: &AppWindow, ctx: &Rc<Ctx>) {
    macro_rules! on {
        ($global:ty, $setter:ident, |$c:ident $(, $a:ident)*| $body:expr) => {{
            let $c = ctx.clone();
            window.global::<$global>().$setter(move |$($a),*| {
                let $c = &$c;
                $body
            });
        }};
    }

    on!(PasswordPromptState, on_password_edited, |c, t| c
        .prompt_edit(|p| p.password = t.to_string()));
    on!(PasswordPromptState, on_toggle_show, |c| c
        .prompt_edit(|p| p.show_text = !p.show_text));
    on!(PasswordPromptState, on_toggle_remember, |c| c
        .prompt_edit(|p| p.remember = !p.remember));
    on!(PasswordPromptState, on_submit, |c| c.prompt_submit());
    on!(PasswordPromptState, on_cancel, |c| c.prompt_cancel());

    on!(ConnectionsState, on_toggle_folder, |c, key| c
        .toggle_folder(&key));

    on!(ConnectionEditorState, on_folder_changed, |c, t| c
        .edit_form(|f| f.folder = t.to_string()));
    on!(ConnectionEditorState, on_ssh_local_port_changed, |c, t| c
        .edit_form(|f| f.ssh_local_port = t.to_string()));

    on!(FileBrowse, on_browse, |c, target| c.browse(target));

    on!(ConnectionLostState, on_reconnect, |c| c.lost_reconnect());
    on!(ConnectionLostState, on_go_connections, |c| c
        .lost_go_connections());
}
