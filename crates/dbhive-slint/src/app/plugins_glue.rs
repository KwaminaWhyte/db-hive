//! Controller glue for the plugin manager.
//!
//! A child module of `app` so it can use the controller's private pieces
//! (`Ctx`, `Session`, `with_ctx`, `show_confirm`) without widening their
//! visibility. What the panel shows and every decision about it live in
//! `dbhive_ui_model::plugins_panel`; the engine, the plugin directory and the
//! marketplace catalogue are `dbhive_core::plugins`. This file moves values
//! between those and the `PluginsState` global in the window.
//!
//! # What a plugin can do from here
//!
//! It can describe an interface as data, receive events for controls it
//! declared, and ask the host to show a toast; each only if its manifest
//! declares the matching permission, which the runtime checks. It cannot
//! reach a connection, a credential or a file outside its own data directory,
//! and it cannot name a function the interface on screen did not declare:
//! `PluginManager::dispatch_ui_event` resolves every event against that
//! descriptor before any plugin code runs.
//!
//! # Where plugins come from
//!
//! Installing a marketplace entry copies the plugin shipped with the
//! application (`default_bundled_roots`); there is no marketplace server and
//! this shell does not invent one. `DBHIVE_PLUGIN_SOURCES` (a `PATH`-style
//! list of directories) adds search roots in front, a development aid like
//! the other `DBHIVE_*` variables, and what the controller test uses to
//! install a plugin from a temp directory.
//!
//! # Threads
//!
//! Plugin JavaScript is bounded by loop count rather than wall clock, so it can
//! legitimately run for seconds. Every call goes through `runtime::spawn`
//! (the manager runs the interpreter on a blocking thread); nothing here
//! waits on a plugin from the UI thread.

use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use dbhive_core::plugins::manager::default_bundled_roots;
use dbhive_core::plugins::{HostCall, PluginManager, PluginStore, PluginUi, UiEvent};
use dbhive_ui_model::plugins::Tab;
use dbhive_ui_model::plugins_panel::{
    absorb, install_error_message, Absorbed, Busy, NoteTone, PluginsPanel, Scope,
};
use slint::ComponentHandle;

use super::{with_ctx, Ctx, PendingAction};
use crate::plugins_view::{self as view, Items};
use crate::{PluginsState, Tone};

/// Environment variable naming extra directories to search for bundled plugins.
pub const SOURCES_ENV: &str = "DBHIVE_PLUGIN_SOURCES";

/// A confirmation the panel is waiting on.
pub(super) enum Pending {
    Uninstall(String),
}

pub(super) struct PluginSession {
    panel: PluginsPanel,
    /// `None` when the data directory could not be resolved; the panel then
    /// shows that error instead of an empty list.
    manager: Result<Arc<PluginManager>, String>,
    scan: crate::runtime::Latest,
    /// Newest interface-producing call, so a slow answer cannot overwrite a
    /// newer one. Toasts a stale answer raised are still shown.
    interface_ticket: crate::runtime::Latest,
    /// Plugins already started this session, so a rescan does not run their
    /// `onLoad` again.
    started: HashSet<String>,
    form_error: String,
}

impl Default for PluginSession {
    fn default() -> Self {
        Self {
            panel: PluginsPanel::new(),
            manager: Err("The plugin manager has not started".to_string()),
            scan: crate::runtime::Latest::default(),
            interface_ticket: crate::runtime::Latest::default(),
            started: HashSet::new(),
            form_error: String::new(),
        }
    }
}

fn build_manager() -> Result<Arc<PluginManager>, String> {
    let dir = dbhive_core::storage::app_data_dir().map_err(|e| e.to_string())?;
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(extra) = std::env::var_os(SOURCES_ENV) {
        roots.extend(std::env::split_paths(&extra).filter(|p| !p.as_os_str().is_empty()));
    }
    roots.extend(default_bundled_roots());
    Ok(Arc::new(
        PluginManager::new(PluginStore::under_app_dir(&dir), env!("CARGO_PKG_VERSION"))
            .with_bundled_roots(roots),
    ))
}

/// What a plugin call produced, for the UI thread. `Send` data only.
struct Activation {
    /// What `onLoad` asked the host to do.
    calls: Vec<HostCall>,
    /// `renderUi`'s answer, or why there is none.
    interface: Result<(Option<PluginUi>, Vec<HostCall>), String>,
    /// `onLoad` itself failed (a plugin that is not loaded has no interface).
    load_error: Option<String>,
}

/// Load a plugin (`onLoad`) and ask it to describe its interface.
async fn activate(manager: &PluginManager, id: &str) -> Activation {
    let mut calls = Vec::new();
    if !manager.is_loaded(id).await {
        match manager.load(id).await {
            Ok(out) => calls = out.outbox,
            Err(e) => {
                return Activation {
                    calls,
                    interface: Err(e.to_string()),
                    load_error: Some(e.to_string()),
                }
            }
        }
    }
    let interface = manager
        .render_ui(id)
        .await
        .map(|out| (Some(out.value), out.outbox))
        .map_err(|e| e.to_string());
    Activation {
        calls,
        interface,
        load_error: None,
    }
}

fn tone(t: NoteTone) -> Tone {
    match t {
        NoteTone::Neutral => Tone::Neutral,
        NoteTone::Success => Tone::Success,
        NoteTone::Warning => Tone::Warning,
        NoteTone::Danger => Tone::Danger,
    }
}

fn scope_of(n: i32) -> Scope {
    if n == 1 {
        Scope::Form
    } else {
        Scope::View
    }
}

/// `renderUi` is optional. A plugin without it simply has no interface.
fn interface_error_message(plugin_name: &str, engine_error: &str) -> String {
    if engine_error.contains("'renderUi' not found") {
        format!("{} does not provide an interface to open.", plugin_name)
    } else {
        engine_error.to_string()
    }
}

impl Ctx {
    // -- Small helpers --------------------------------------------------------

    fn plugin_name(&self, id: &str) -> String {
        let s = self.session.borrow();
        s.plugins
            .panel
            .find(id)
            .map(|p| p.manifest.name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    fn plugin_manager(&self) -> Result<Arc<PluginManager>, String> {
        self.session.borrow().plugins.manager.clone()
    }

    /// Project the panel into the `PluginsState` global.
    pub(super) fn refresh_plugins(&self, items: Items) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let s = self.session.borrow();
        let g = w.global::<PluginsState>();
        view::apply(&g, &s.plugins.panel, items);
        g.set_form_error(s.plugins.form_error.as_str().into());
        g.set_form_busy(
            s.plugins
                .panel
                .form
                .as_ref()
                .is_some_and(|f| s.plugins.panel.busy.get(&f.plugin_id) == Some(&Busy::Saving)),
        );
    }

    fn show_plugin_toasts(self: &Rc<Self>, absorbed: &Absorbed) {
        for t in &absorbed.toasts {
            self.toast(t.text.clone(), tone(t.tone));
        }
    }

    // -- Open, close, scan -----------------------------------------------------------

    /// Start the manager and read what is installed, so enabled plugins can
    /// register their toolbar buttons before the panel is ever opened.
    pub(super) fn plugins_start(self: &Rc<Self>) {
        self.session.borrow_mut().plugins.manager = build_manager();
        self.plugins_rescan(false);
    }

    pub(super) fn open_plugins(self: &Rc<Self>) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        w.global::<PluginsState>().set_visible(true);
        // A panel that already has a list shows it while it re-reads.
        let loaded = matches!(
            self.session.borrow().plugins.panel.load,
            dbhive_ui_model::plugins_panel::Load::Ready
        );
        self.plugins_rescan(!loaded);
        self.refresh_plugins(Items::Rebuild);
    }

    fn hide_plugins(&self) {
        if let Some(w) = self.window.upgrade() {
            w.global::<PluginsState>().set_visible(false);
        }
    }

    pub(super) fn plugins_open_now(&self) -> bool {
        self.window
            .upgrade()
            .is_some_and(|w| w.global::<PluginsState>().get_visible())
    }

    fn plugins_rescan(self: &Rc<Self>, show_loading: bool) {
        let manager = match self.plugin_manager() {
            Ok(m) => m,
            Err(e) => {
                self.session.borrow_mut().plugins.panel.apply_scan(Err(e));
                self.refresh_plugins(Items::Rebuild);
                return;
            }
        };
        let ticket = {
            let mut s = self.session.borrow_mut();
            if show_loading {
                s.plugins.panel.begin_load();
            }
            s.plugins.scan.begin()
        };
        self.refresh_plugins(Items::Rebuild);
        crate::runtime::spawn(
            async move {
                manager
                    .initialize()
                    .await
                    .map(|scan| (scan.plugins, scan.problems))
                    .map_err(|e| e.to_string())
            },
            move |result| {
                with_ctx(|ctx| {
                    if !ticket.is_current() {
                        return;
                    }
                    ctx.session.borrow_mut().plugins.panel.apply_scan(result);
                    ctx.refresh_plugins(Items::Rebuild);
                    ctx.plugins_start_enabled();
                })
            },
        );
    }

    /// Run `onLoad` and `renderUi` for every enabled plugin not started yet, so
    /// their toolbar buttons exist. A plugin with no interface is fine; one
    /// that fails to load is reported once.
    fn plugins_start_enabled(self: &Rc<Self>) {
        let todo: Vec<String> = {
            let s = self.session.borrow();
            s.plugins
                .panel
                .installed
                .iter()
                .filter(|p| p.enabled)
                .map(|p| p.manifest.id.clone())
                .filter(|id| !s.plugins.started.contains(id) && !s.plugins.panel.is_busy(id))
                .collect()
        };
        for id in todo {
            self.plugins_activate(id, false);
        }
    }

    /// Load a plugin and take its interface. `explicit` is the user asking to
    /// open it: failures are then shown beside the controls.
    fn plugins_activate(self: &Rc<Self>, id: String, explicit: bool) {
        let Ok(manager) = self.plugin_manager() else {
            return;
        };
        {
            let mut s = self.session.borrow_mut();
            s.plugins.started.insert(id.clone());
            s.plugins.panel.busy.insert(
                id.clone(),
                if explicit {
                    Busy::Opening
                } else {
                    Busy::Starting
                },
            );
        }
        self.refresh_plugins(Items::Patch);
        let ticket = self.session.borrow().plugins.interface_ticket.begin();
        let task_id = id.clone();
        crate::runtime::spawn(
            async move { activate(&manager, &task_id).await },
            move |activation| {
                with_ctx(|ctx| {
                    ctx.session.borrow_mut().plugins.panel.busy.remove(&id);
                    ctx.plugins_activated(&id, explicit, activation, ticket.is_current());
                })
            },
        );
    }

    fn plugins_activated(self: &Rc<Self>, id: &str, explicit: bool, a: Activation, current: bool) {
        let name = self.plugin_name(id);
        let load_calls = absorb(&name, None, a.calls);
        self.show_plugin_toasts(&load_calls);

        if let Some(e) = a.load_error {
            // A plugin that will not start is worth telling the user about,
            // once, whoever asked.
            self.toast(format!("Could not start {}: {}", name, e), Tone::Danger);
            if explicit {
                self.session
                    .borrow_mut()
                    .plugins
                    .panel
                    .fail_interface(id, e);
            }
            self.refresh_plugins(Items::Rebuild);
            return;
        }

        match a.interface {
            Ok((returned, calls)) => {
                let absorbed = absorb(&name, returned, calls);
                self.show_plugin_toasts(&absorbed);
                if let (Some(next), true) = (absorbed.descriptor, current) {
                    let mut s = self.session.borrow_mut();
                    // Still enabled and still installed? The user may have
                    // moved on while the plugin was thinking.
                    if s.plugins.panel.find(id).is_some_and(|p| p.enabled) {
                        s.plugins.panel.set_interface(id, next);
                    }
                }
            }
            Err(e) if explicit => {
                let msg = interface_error_message(&name, &e);
                self.session
                    .borrow_mut()
                    .plugins
                    .panel
                    .fail_interface(id, msg);
            }
            // The first look at a plugin that has no interface says nothing.
            Err(_) => {}
        }
        self.refresh_plugins(Items::Rebuild);
    }

    // -- Panel intents --------------------------------------------------------------

    fn plugins_set_tab(self: &Rc<Self>, tab: Tab) {
        self.session.borrow_mut().plugins.panel.tab = tab;
        self.refresh_plugins(Items::Rebuild);
    }

    fn plugins_select(self: &Rc<Self>, id: &str) {
        {
            let mut s = self.session.borrow_mut();
            s.plugins.panel.select(id);
            s.plugins.form_error.clear();
        }
        self.refresh_plugins(Items::Rebuild);
    }

    fn selected_plugin(&self) -> Option<String> {
        self.session.borrow().plugins.panel.selected.clone()
    }

    /// Enable or disable. `id` is blank for the selected plugin.
    fn plugins_toggle(self: &Rc<Self>, id: &str) {
        let Some(id) = (if id.is_empty() {
            self.selected_plugin()
        } else {
            Some(id.to_string())
        }) else {
            return;
        };
        let Ok(manager) = self.plugin_manager() else {
            return;
        };
        let (enabling, name) = {
            let mut s = self.session.borrow_mut();
            let Some(p) = s.plugins.panel.find(&id) else {
                return;
            };
            if s.plugins.panel.is_busy(&id) {
                return;
            }
            let (enabling, name) = (!p.enabled, p.manifest.name.clone());
            s.plugins.panel.busy.insert(id.clone(), Busy::Toggling);
            (enabling, name)
        };
        self.refresh_plugins(Items::Rebuild);
        let ticket = self.session.borrow().plugins.interface_ticket.begin();
        let task_id = id.clone();
        crate::runtime::spawn(
            async move {
                let plugin = manager
                    .set_enabled(&task_id, enabling)
                    .await
                    .map_err(|e| e.to_string())?;
                // Enabling loads the plugin and reads its interface; disabling
                // has already unloaded it (the manager does that first).
                let activation = if enabling {
                    Some(activate(&manager, &task_id).await)
                } else {
                    None
                };
                Ok::<_, String>((plugin, activation))
            },
            move |result| {
                with_ctx(|ctx| {
                    ctx.session.borrow_mut().plugins.panel.busy.remove(&id);
                    match result {
                        Ok((plugin, activation)) => {
                            {
                                let mut s = ctx.session.borrow_mut();
                                if enabling {
                                    s.plugins.started.insert(id.clone());
                                } else {
                                    s.plugins.started.remove(&id);
                                }
                                s.plugins.panel.upsert(plugin);
                            }
                            ctx.toast(
                                format!(
                                    "{} {}",
                                    name,
                                    if enabling { "enabled" } else { "disabled" }
                                ),
                                Tone::Neutral,
                            );
                            match activation {
                                Some(a) => {
                                    ctx.plugins_activated(&id, false, a, ticket.is_current())
                                }
                                None => ctx.refresh_plugins(Items::Rebuild),
                            }
                        }
                        Err(e) => {
                            ctx.toast(format!("Could not update {}: {}", name, e), Tone::Danger);
                            ctx.refresh_plugins(Items::Rebuild);
                        }
                    }
                })
            },
        );
    }

    fn plugins_open_interface(self: &Rc<Self>) {
        let Some(id) = self.selected_plugin() else {
            return;
        };
        {
            let mut s = self.session.borrow_mut();
            let usable = s.plugins.panel.find(&id).is_some_and(|p| p.enabled)
                && !s.plugins.panel.is_busy(&id);
            if !usable {
                return;
            }
            s.plugins.panel.open_view = Some(id.clone());
            s.plugins.panel.form = None;
        }
        self.plugins_activate(id, true);
        self.refresh_plugins(Items::Rebuild);
    }

    fn plugins_close_interface(self: &Rc<Self>) {
        self.session.borrow_mut().plugins.panel.open_view = None;
        self.refresh_plugins(Items::Rebuild);
    }

    fn plugins_open_settings(self: &Rc<Self>) {
        let Some(id) = self.selected_plugin() else {
            return;
        };
        {
            let mut s = self.session.borrow_mut();
            s.plugins.form_error.clear();
            if !s.plugins.panel.open_settings(&id) {
                return;
            }
        }
        self.refresh_plugins(Items::Rebuild);
    }

    fn plugins_close_settings(self: &Rc<Self>) {
        {
            let mut s = self.session.borrow_mut();
            s.plugins.panel.close_settings();
            s.plugins.form_error.clear();
        }
        self.refresh_plugins(Items::Rebuild);
    }

    fn plugins_save_settings(self: &Rc<Self>) {
        let Ok(manager) = self.plugin_manager() else {
            return;
        };
        let saved = self.session.borrow().plugins.panel.settings_to_save();
        let (id, config) = match saved {
            Ok(v) => v,
            Err(e) => {
                self.session.borrow_mut().plugins.form_error = e;
                self.refresh_plugins(Items::Patch);
                return;
            }
        };
        {
            let mut s = self.session.borrow_mut();
            s.plugins.form_error.clear();
            s.plugins.panel.busy.insert(id.clone(), Busy::Saving);
        }
        self.refresh_plugins(Items::Patch);
        let name = self.plugin_name(&id);
        let task_id = id.clone();
        crate::runtime::spawn(
            async move {
                manager
                    .update_config(&task_id, config)
                    .await
                    .map_err(|e| e.to_string())
            },
            move |result| {
                with_ctx(|ctx| {
                    ctx.session.borrow_mut().plugins.panel.busy.remove(&id);
                    match result {
                        Ok(plugin) => {
                            {
                                let mut s = ctx.session.borrow_mut();
                                s.plugins.panel.upsert(plugin);
                                s.plugins.panel.close_settings();
                            }
                            ctx.toast(format!("{} settings saved", name), Tone::Success);
                        }
                        // Keep the form open with what the user typed.
                        Err(e) => {
                            ctx.session.borrow_mut().plugins.form_error =
                                format!("Could not save the settings: {}", e);
                        }
                    }
                    ctx.refresh_plugins(Items::Patch);
                })
            },
        );
    }

    fn plugins_uninstall_asked(self: &Rc<Self>) {
        let Some(id) = self.selected_plugin() else {
            return;
        };
        if self.session.borrow().plugins.panel.is_busy(&id) {
            return;
        }
        let name = self.plugin_name(&id);
        self.session.borrow_mut().pending_action =
            Some(PendingAction::Plugins(Pending::Uninstall(id)));
        self.show_confirm(
            "Uninstall plugin?",
            &format!(
                "Uninstall {}? This removes the plugin and all of its stored data, and cannot be undone.",
                name
            ),
            "Uninstall",
        );
    }

    pub(super) fn plugins_confirmed(self: &Rc<Self>, pending: Pending) {
        match pending {
            Pending::Uninstall(id) => self.plugins_uninstall(id),
        }
    }

    fn plugins_uninstall(self: &Rc<Self>, id: String) {
        let Ok(manager) = self.plugin_manager() else {
            return;
        };
        let name = self.plugin_name(&id);
        {
            let mut s = self.session.borrow_mut();
            if s.plugins.panel.find(&id).is_none() || s.plugins.panel.is_busy(&id) {
                return;
            }
            s.plugins.panel.busy.insert(id.clone(), Busy::Uninstalling);
        }
        self.refresh_plugins(Items::Rebuild);
        let task_id = id.clone();
        crate::runtime::spawn(
            async move {
                // onUnload for the plugin's benefit; its failure does not stop
                // the removal.
                let _ = manager.unload(&task_id).await;
                manager.uninstall(&task_id).await.map_err(|e| e.to_string())
            },
            move |result| {
                with_ctx(|ctx| {
                    ctx.session.borrow_mut().plugins.panel.busy.remove(&id);
                    match result {
                        Ok(()) => {
                            {
                                let mut s = ctx.session.borrow_mut();
                                s.plugins.started.remove(&id);
                                s.plugins.panel.remove(&id);
                            }
                            ctx.toast(format!("{} uninstalled", name), Tone::Neutral);
                        }
                        Err(e) => {
                            ctx.toast(format!("Could not uninstall {}: {}", name, e), Tone::Danger)
                        }
                    }
                    ctx.refresh_plugins(Items::Rebuild);
                })
            },
        );
    }

    fn plugins_install(self: &Rc<Self>, id: &str) {
        let Ok(manager) = self.plugin_manager() else {
            return;
        };
        let entry = {
            let mut s = self.session.borrow_mut();
            let Some(entry) = s.plugins.panel.market_entry(id).cloned() else {
                return;
            };
            if s.plugins.panel.is_installed(id) || s.plugins.panel.is_busy(id) {
                return;
            }
            s.plugins.panel.install_errors.remove(id);
            s.plugins
                .panel
                .busy
                .insert(id.to_string(), Busy::Installing);
            entry
        };
        self.refresh_plugins(Items::Rebuild);
        let id = id.to_string();
        let name = entry.manifest.name.clone();
        let ticket = self.session.borrow().plugins.interface_ticket.begin();
        let task_id = id.clone();
        crate::runtime::spawn(
            async move {
                let plugin = manager.install(&entry).await.map_err(|e| e.to_string())?;
                // A freshly installed plugin is enabled: installing it is the
                // consent. Load it and take its toolbar.
                let activation = activate(&manager, &task_id).await;
                Ok::<_, String>((plugin, activation))
            },
            move |result| {
                with_ctx(|ctx| {
                    ctx.session.borrow_mut().plugins.panel.busy.remove(&id);
                    match result {
                        Ok((plugin, activation)) => {
                            // Name what is on disk now, not what the catalogue
                            // promised.
                            let installed_name = plugin.manifest.name.clone();
                            {
                                let mut s = ctx.session.borrow_mut();
                                s.plugins.started.insert(id.clone());
                                s.plugins.panel.upsert(plugin);
                            }
                            ctx.toast(format!("{} installed", installed_name), Tone::Success);
                            ctx.plugins_activated(&id, false, activation, ticket.is_current());
                        }
                        Err(e) => {
                            let msg = install_error_message(&name, &e);
                            ctx.session
                                .borrow_mut()
                                .plugins
                                .panel
                                .install_errors
                                .insert(id.clone(), msg.clone());
                            ctx.toast(msg, Tone::Danger);
                            ctx.refresh_plugins(Items::Rebuild);
                        }
                    }
                })
            },
        );
    }

    // -- Events into a plugin ---------------------------------------------------

    /// Deliver `event` to a plugin. `current` is the interface the user is
    /// looking at; core resolves the event against it before any plugin code
    /// runs. `track` marks a call that blocks others for that plugin (a
    /// button), as opposed to a field edit.
    fn plugins_dispatch(
        self: &Rc<Self>,
        plugin_id: &str,
        current: PluginUi,
        event: UiEvent,
        track: bool,
    ) {
        let Ok(manager) = self.plugin_manager() else {
            return;
        };
        {
            let mut s = self.session.borrow_mut();
            if track {
                if s.plugins.panel.is_busy(plugin_id) {
                    return;
                }
                s.plugins
                    .panel
                    .busy
                    .insert(plugin_id.to_string(), Busy::Running);
            }
        }
        if track {
            self.refresh_plugins(Items::Patch);
        }
        let id = plugin_id.to_string();
        let name = self.plugin_name(plugin_id);
        let ticket = self.session.borrow().plugins.interface_ticket.begin();
        let task_id = id.clone();
        crate::runtime::spawn(
            async move {
                manager
                    .dispatch_ui_event(&task_id, &current, &event)
                    .await
                    .map_err(|e| e.to_string())
            },
            move |result| {
                with_ctx(|ctx| {
                    if track {
                        ctx.session.borrow_mut().plugins.panel.busy.remove(&id);
                    }
                    match result {
                        Ok(out) => {
                            let absorbed = absorb(&name, out.value, out.outbox);
                            ctx.show_plugin_toasts(&absorbed);
                            if let (Some(next), true) = (absorbed.descriptor, ticket.is_current()) {
                                ctx.session
                                    .borrow_mut()
                                    .plugins
                                    .panel
                                    .set_interface(&id, next);
                            }
                        }
                        Err(e) => {
                            ctx.session
                                .borrow_mut()
                                .plugins
                                .panel
                                .set_interface_error(&id, e.clone());
                            ctx.toast(format!("{}: {}", name, e), Tone::Danger);
                        }
                    }
                    ctx.refresh_plugins(Items::Rebuild);
                })
            },
        );
    }

    fn plugins_button(self: &Rc<Self>, scope: Scope, id: &str) {
        if scope != Scope::View {
            return;
        }
        let (plugin, event) = {
            let s = self.session.borrow();
            let Some(plugin) = s.plugins.panel.open_view.clone() else {
                return;
            };
            let Some(event) = s.plugins.panel.button_event(Scope::View, id) else {
                return;
            };
            (plugin, event)
        };
        self.plugins_dispatch(&plugin, event.0, event.1, true);
    }

    fn plugins_toolbar(self: &Rc<Self>, plugin_id: &str, item_id: &str) {
        let event = self
            .session
            .borrow()
            .plugins
            .panel
            .toolbar_event(plugin_id, item_id);
        if let Some((current, event)) = event {
            self.plugins_dispatch(plugin_id, current, event, true);
        }
    }

    /// The user finished editing a field: tell the plugin its new value.
    fn plugins_value_changed(self: &Rc<Self>, scope: Scope, id: &str) {
        if scope != Scope::View {
            return;
        }
        let (plugin, event) = {
            let s = self.session.borrow();
            let Some(plugin) = s.plugins.panel.open_view.clone() else {
                return;
            };
            let Some(event) = s.plugins.panel.value_event(id) else {
                return;
            };
            (plugin, event)
        };
        self.plugins_dispatch(&plugin, event.0, event.1, false);
    }
}

/// Connect every `PluginsState` callback to the controller, and start the
/// manager.
pub(super) fn wire(window: &crate::AppWindow, ctx: &Rc<Ctx>) {
    let p = window.global::<PluginsState>();

    macro_rules! on {
        ($setter:ident, |$c:ident $(, $arg:ident)*| $body:expr) => {{
            let $c = ctx.clone();
            p.$setter(move |$($arg),*| {
                let $c = &$c;
                $body
            });
        }};
    }

    on!(on_open_requested, |c| c.open_plugins());
    on!(on_close, |c| c.hide_plugins());
    on!(on_retry, |c| {
        c.plugins_rescan(true);
    });
    on!(on_refresh, |c| {
        c.plugins_rescan(true);
    });
    on!(on_tab_selected, |c, i| c.plugins_set_tab(if i == 1 {
        Tab::Marketplace
    } else {
        Tab::Installed
    }));

    on!(on_select, |c, id| c.plugins_select(&id));
    on!(on_toggle_enabled, |c, id| c.plugins_toggle(&id));
    on!(on_open_interface, |c| c.plugins_open_interface());
    on!(on_close_interface, |c| c.plugins_close_interface());
    on!(on_open_settings, |c| c.plugins_open_settings());
    on!(on_settings_save, |c| c.plugins_save_settings());
    on!(on_settings_cancel, |c| c.plugins_close_settings());
    on!(on_uninstall, |c| c.plugins_uninstall_asked());

    on!(on_item_button, |c, scope, id| c
        .plugins_button(scope_of(scope), &id));
    on!(on_item_input, |c, scope, id, t| {
        c.session
            .borrow_mut()
            .plugins
            .panel
            .set_text(scope_of(scope), &id, &t);
    });
    on!(on_item_input_accepted, |c, scope, id, t| {
        c.session
            .borrow_mut()
            .plugins
            .panel
            .set_text(scope_of(scope), &id, &t);
        c.plugins_value_changed(scope_of(scope), &id);
    });
    on!(on_item_number, |c, scope, id, t| {
        c.session
            .borrow_mut()
            .plugins
            .panel
            .set_number(scope_of(scope), &id, &t);
        c.refresh_plugins(Items::Patch);
    });
    on!(on_item_number_accepted, |c, scope, id, t| {
        let changed = c
            .session
            .borrow_mut()
            .plugins
            .panel
            .set_number(scope_of(scope), &id, &t);
        c.refresh_plugins(Items::Patch);
        if changed.is_some() {
            c.plugins_value_changed(scope_of(scope), &id);
        }
    });
    on!(on_item_check, |c, scope, id| {
        let changed = c
            .session
            .borrow_mut()
            .plugins
            .panel
            .toggle_check(scope_of(scope), &id);
        c.refresh_plugins(Items::Patch);
        if changed.is_some() {
            c.plugins_value_changed(scope_of(scope), &id);
        }
    });
    on!(on_item_select, |c, scope, id, i| {
        let changed = c.session.borrow_mut().plugins.panel.pick_option(
            scope_of(scope),
            &id,
            usize::try_from(i).unwrap_or(usize::MAX),
        );
        c.refresh_plugins(Items::Patch);
        if changed.is_some() {
            c.plugins_value_changed(scope_of(scope), &id);
        }
    });
    on!(on_item_panel, |c, scope, i| {
        c.session
            .borrow_mut()
            .plugins
            .panel
            .toggle_panel(scope_of(scope), usize::try_from(i).unwrap_or(usize::MAX));
        c.refresh_plugins(Items::Patch);
    });

    on!(on_search_edited, |c, t| {
        c.session.borrow_mut().plugins.panel.filters.search = t.to_string();
        c.refresh_plugins(Items::Patch);
    });
    on!(on_category_picked, |c, i| {
        c.session
            .borrow_mut()
            .plugins
            .panel
            .set_category_index(usize::try_from(i).unwrap_or(0));
        c.refresh_plugins(Items::Patch);
    });
    on!(on_sort_picked, |c, i| {
        c.session
            .borrow_mut()
            .plugins
            .panel
            .set_sort_index(usize::try_from(i).unwrap_or(0));
        c.refresh_plugins(Items::Patch);
    });
    on!(on_verified_toggled, |c| {
        {
            let mut s = c.session.borrow_mut();
            let f = &mut s.plugins.panel.filters;
            f.verified_only = !f.verified_only;
        }
        c.refresh_plugins(Items::Patch);
    });
    on!(on_clear_filters, |c| {
        c.session.borrow_mut().plugins.panel.clear_filters();
        if let Some(w) = c.window.upgrade() {
            w.global::<PluginsState>().set_search("".into());
        }
        c.refresh_plugins(Items::Patch);
    });
    on!(on_install, |c, id| c.plugins_install(&id));
    on!(on_toolbar_pressed, |c, plugin, item| c
        .plugins_toolbar(&plugin, &item));

    ctx.plugins_start();
}
