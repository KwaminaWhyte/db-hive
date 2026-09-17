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

use crate::connection_list::{self, EnvTone};
use crate::runtime;
use crate::{AppWindow, ConnectionItem, Screen, Theme, ToastItem, Tone};

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

    ctx.refresh_connections();
    CTX.with(|c| *c.borrow_mut() = Some(ctx));
    let result = window.run();
    CTX.with(|c| c.borrow_mut().take());
    result
}
