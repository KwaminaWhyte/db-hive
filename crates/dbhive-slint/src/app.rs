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

use dbhive_core::models::ConnectionProfile;
use dbhive_core::state::AppState;
use slint::{ComponentHandle, ModelRc, VecModel};

use dbhive_ui_model::connection_editor::{
    build_profile, build_profile_with, form_from_profile, save_profile, test_profile, validate,
    ConnectionForm,
};

use crate::connection_editor as editor_view;
use crate::connection_list::{self, EnvTone};
use crate::runtime;
use crate::{AppWindow, ConnectionItem, Screen, Theme, ToastItem, Tone};

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
                    host: connection_list::address(p).into(),
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
                        let name = ctx
                            .profiles()
                            .into_iter()
                            .find(|p| p.id == conn_id)
                            .map(|p| p.name)
                            .unwrap_or(conn_id);
                        ctx.toast(format!("Connected to {}", name), Tone::Success);
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
                if let Err(e) = result {
                    ctx.toast(format!("Disconnect failed: {}", e), Tone::Danger);
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

        let state = self.state.clone();
        runtime::spawn(
            async move { test_profile(state, profile, password, None).await },
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

        let state = self.state.clone();
        runtime::spawn(
            async move { save_profile(state, profile, id, password, None).await },
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
        let weak = window.as_weak();
        window.on_toggle_theme(move || {
            if let Some(w) = weak.upgrade() {
                let theme = w.global::<Theme>();
                theme.set_dark(!theme.get_dark());
            }
        });
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
        window.on_confirm_accepted(move || ctx.delete_pending_connection());
    }
    {
        let ctx = ctx.clone();
        window.on_confirm_cancelled(move || ctx.cancel_pending_delete());
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

    ctx.refresh_connections();
    CTX.with(|c| *c.borrow_mut() = Some(ctx));
    let result = window.run();
    CTX.with(|c| c.borrow_mut().take());
    result
}
