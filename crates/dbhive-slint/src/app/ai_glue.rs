//! Controller glue for the AI assistant panel.
//!
//! A child module of `app` so it can use the controller's private pieces
//! (`Ctx`, `Session`, `with_ctx`). The panel's logic lives in
//! `dbhive_ui_model::ai_panel` (and `ai_assistant`); prompts and providers live
//! in `dbhive_core::service_ai`. This file moves values between those, the
//! open workspace, and the `AiState` global.
//!
//! What it guarantees:
//!
//! * **Keys.** An API key is typed into `AiState.api_key`, handed to the key
//!   store on a worker thread and the field is cleared in the same breath. It
//!   is read back only inside `service_ai`, when a provider client is built.
//!   It is never in a view, a log line, a toast or the settings file. The
//!   store is the OS keyring; a test installs an in-memory one with
//!   [`use_ai_key_store`].
//! * **Network.** Every provider call, status check, model list and schema
//!   read runs through `runtime::spawn`. The UI thread never waits.
//! * **Nothing runs.** An answer's statement goes into the editor on request.
//!   Running it is the editor's Run button, with its destructive-statement
//!   guard.
//! * **Late results.** Each kind of request carries a [`runtime::Latest`]
//!   token, so a slow answer cannot overwrite a newer one, and switching
//!   provider abandons whatever was in flight.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use dbhive_core::ai::AiProviderType;
use dbhive_core::service_ai::{self, ApiKeyStore, KeyringKeyStore, ProviderStatus};
use dbhive_ui_model::ai_assistant::{EmptyState, Kind};
use dbhive_ui_model::ai_panel::{
    apply_text, context_database_label, error_text, AiPanel, Apply, Inputs,
};
use dbhive_ui_model::command_palette::CommandId;
use slint::ComponentHandle;

use super::{with_ctx, Ctx};
use crate::{ai_view, runtime, AiState, AppWindow, Screen, Tone};

thread_local! {
    /// A key store installed before the window is built, for tests.
    static KEY_STORE_OVERRIDE: RefCell<Option<Arc<dyn ApiKeyStore>>> = const { RefCell::new(None) };
}

/// Use `store` for this thread's next [`build`](super::build) instead of the OS
/// keyring. A test calls this before building the window so it never reads or
/// writes the developer's keychain.
pub fn use_ai_key_store(store: Arc<dyn ApiKeyStore>) {
    KEY_STORE_OVERRIDE.with(|s| *s.borrow_mut() = Some(store));
}

/// The panel's own state, with the tokens that keep late results honest.
pub(super) struct AiSession {
    panel: AiPanel,
    store: Arc<dyn ApiKeyStore>,
    status: runtime::Latest,
    models: runtime::Latest,
    request: runtime::Latest,
    context: runtime::Latest,
    saving_key: bool,
    /// Why the base URL could not be saved.
    settings_error: String,
    /// Rewrite the base URL field on the next projection.
    sync_base_url: bool,
    /// The editor text and error the panel last showed, so typing in the editor
    /// rebuilds the panel only when what it shows has changed.
    last_editor: (String, Option<String>),
}

impl Default for AiSession {
    fn default() -> Self {
        let store = KEY_STORE_OVERRIDE
            .with(|s| s.borrow().clone())
            .unwrap_or_else(|| Arc::new(KeyringKeyStore));
        Self {
            panel: AiPanel::default(),
            store,
            status: runtime::Latest::default(),
            models: runtime::Latest::default(),
            request: runtime::Latest::default(),
            context: runtime::Latest::default(),
            saving_key: false,
            settings_error: String::new(),
            sync_base_url: true,
            last_editor: (String::new(), None),
        }
    }
}

impl Ctx {
    // -- Projection ---------------------------------------------------------

    /// The active tab's text and last error, copied out.
    fn ai_editor(&self) -> (String, Option<String>) {
        let s = self.session.borrow();
        match s.workspace.tabs.active().filter(|t| t.is_query()) {
            Some(tab) => (tab.sql.clone(), tab.error.clone()),
            None => (String::new(), None),
        }
    }

    /// Project the panel into the `AiState` global. Cheap when closed.
    pub(super) fn ai_refresh(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        if !w.global::<AiState>().get_visible() {
            return;
        }
        let (sql, error) = self.ai_editor();
        let prompt = w.global::<AiState>().get_prompt().to_string();
        let chat = w.global::<AiState>().get_chat().to_string();
        let sync = {
            let mut s = self.session.borrow_mut();
            s.ai.last_editor = (sql.clone(), error.clone());
            std::mem::take(&mut s.ai.sync_base_url)
        };
        let s = self.session.borrow();
        let view = s.ai.panel.view(Inputs {
            current_sql: &sql,
            last_error: error.as_deref(),
            prompt: &prompt,
            chat: &chat,
        });
        ai_view::apply(
            &w,
            &view,
            &ai_view::Extras {
                settings_error: &s.ai.settings_error,
                saving_key: s.ai.saving_key,
                sync_base_url: sync,
            },
        );
    }

    /// The workspace changed: project the panel only if the active query tab's
    /// text or error (the two things it shows) differs from last time.
    pub(super) fn ai_refresh_for_editor(&self) {
        let (sql, error) = self.ai_editor();
        if self.session.borrow().ai.last_editor == (sql, error) {
            return;
        }
        self.ai_refresh();
    }

    // -- Open and close -----------------------------------------------------

    pub(super) fn ai_open(self: &Rc<Self>) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        if w.get_screen() != Screen::Workspace {
            self.toast("Open a connection to use the AI assistant.", Tone::Neutral);
            return;
        }
        w.global::<AiState>().set_visible(true);
        self.session.borrow_mut().ai.sync_base_url = true;

        let loaded = self.session.borrow().ai.panel.settings_loaded();
        if loaded {
            // Reopening: the status may have gone stale while it was closed.
            self.ai_check_status();
        } else {
            self.ai_load_settings();
        }
        self.ai_ensure_context(false);
        self.ai_refresh();
    }

    pub(super) fn ai_close(&self) {
        if let Some(w) = self.window.upgrade() {
            w.global::<AiState>().set_visible(false);
        }
    }

    /// Leaving the workspace: the schema and the conversation belong to the
    /// connection that is going away.
    pub(super) fn ai_leave_workspace(&self) {
        self.ai_close();
        let mut s = self.session.borrow_mut();
        s.ai.context.begin();
        s.ai.panel.clear();
        s.ai.panel.invalidate_context();
    }

    // -- Settings, status, models -------------------------------------------

    fn ai_load_settings(self: &Rc<Self>) {
        runtime::spawn(
            async move { service_ai::load_settings().map_err(|e| error_text(&e.to_string())) },
            |result| {
                with_ctx(|ctx| {
                    {
                        let mut s = ctx.session.borrow_mut();
                        s.ai.panel.settings_result(result);
                        s.ai.sync_base_url = true;
                    }
                    ctx.ai_check_status();
                    ctx.ai_refresh();
                })
            },
        );
    }

    fn ai_persist(self: &Rc<Self>, settings: service_ai::AiSettings) {
        runtime::spawn(
            async move { service_ai::save_settings(&settings).map_err(|e| error_text(&e.to_string())) },
            |result| {
                with_ctx(|ctx| {
                    if let Err(e) = result {
                        ctx.session
                            .borrow_mut()
                            .ai
                            .panel
                            .set_notice(format!("Could not save the AI settings: {e}"));
                        ctx.ai_refresh();
                    }
                })
            },
        );
    }

    /// Ask the provider whether it answers and whether it has what it needs.
    fn ai_check_status(self: &Rc<Self>) {
        let (ticket, provider, settings, store) = {
            let mut s = self.session.borrow_mut();
            s.ai.panel.begin_status_check();
            // A newer check supersedes the model list asked for by an older one.
            s.ai.models.begin();
            (
                s.ai.status.begin(),
                s.ai.panel.provider(),
                s.ai.panel.settings().clone(),
                s.ai.store.clone(),
            )
        };
        self.ai_refresh();
        runtime::spawn(
            async move {
                match service_ai::provider_status_with(store.as_ref(), &settings, provider).await {
                    Ok(status) => status,
                    Err(_) => ProviderStatus {
                        provider,
                        available: false,
                        configured: store.has(provider),
                    },
                }
            },
            move |status| {
                with_ctx(|ctx| {
                    if !ticket.is_current() {
                        return;
                    }
                    let wants = {
                        let mut s = ctx.session.borrow_mut();
                        s.ai.panel.status_result(provider, status);
                        s.ai.panel.wants_models()
                    };
                    if wants {
                        ctx.ai_load_models();
                    }
                    ctx.ai_refresh();
                })
            },
        );
    }

    fn ai_load_models(self: &Rc<Self>) {
        let (ticket, provider, settings, store) = {
            let mut s = self.session.borrow_mut();
            s.ai.panel.begin_models();
            (
                s.ai.models.begin(),
                s.ai.panel.provider(),
                s.ai.panel.settings().clone(),
                s.ai.store.clone(),
            )
        };
        runtime::spawn(
            async move {
                service_ai::list_models_with(store.as_ref(), &settings, provider)
                    .await
                    .map_err(|e| error_text(&e.to_string()))
            },
            move |result| {
                with_ctx(|ctx| {
                    if !ticket.is_current() {
                        return;
                    }
                    ctx.session
                        .borrow_mut()
                        .ai
                        .panel
                        .models_result(provider, result);
                    ctx.ai_refresh();
                })
            },
        );
    }

    fn ai_select_provider(self: &Rc<Self>, index: i32) {
        let Some(info) = usize::try_from(index)
            .ok()
            .and_then(|i| service_ai::PROVIDERS.get(i))
        else {
            return;
        };
        let changed = {
            let mut s = self.session.borrow_mut();
            let changed = s.ai.panel.change_provider(info.provider);
            if changed {
                // Whatever was in flight was for the old provider.
                s.ai.request.begin();
                s.ai.models.begin();
                s.ai.status.begin();
                s.ai.saving_key = false;
                s.ai.settings_error.clear();
                s.ai.sync_base_url = true;
            }
            changed
        };
        if !changed {
            return;
        }
        if let Some(w) = self.window.upgrade() {
            w.global::<AiState>().set_api_key("".into());
        }
        let settings = self.session.borrow().ai.panel.settings().clone();
        self.ai_persist(settings);
        self.ai_check_status();
    }

    fn ai_select_model(self: &Rc<Self>, index: i32) {
        let Ok(i) = usize::try_from(index) else {
            return;
        };
        let saved = self.session.borrow_mut().ai.panel.choose_model(i);
        if let Some(settings) = saved {
            self.ai_persist(settings);
        }
        self.ai_refresh();
    }

    fn ai_save_base_url(self: &Rc<Self>, url: &str) {
        let result = self.session.borrow_mut().ai.panel.set_base_url(url);
        match result {
            Ok(settings) => {
                {
                    let mut s = self.session.borrow_mut();
                    s.ai.settings_error.clear();
                    s.ai.sync_base_url = true;
                    s.ai.status.begin();
                    s.ai.models.begin();
                }
                self.ai_persist(settings);
                self.ai_check_status();
            }
            Err(message) => {
                self.session.borrow_mut().ai.settings_error = message;
                self.ai_refresh();
            }
        }
    }

    // -- Keys -------------------------------------------------------------------

    /// Hand the typed key to the store and clear the field.
    fn ai_save_key(self: &Rc<Self>) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let state = w.global::<AiState>();
        let key = state.get_api_key().to_string();
        if key.trim().is_empty() || self.session.borrow().ai.saving_key {
            return;
        }
        // Out of the UI the moment it is taken, so it is not sitting in a
        // property while the keyring call runs.
        state.set_api_key("".into());

        let (provider, store) = {
            let mut s = self.session.borrow_mut();
            s.ai.saving_key = true;
            (s.ai.panel.provider(), s.ai.store.clone())
        };
        self.ai_refresh();
        runtime::spawn(
            async move {
                store
                    .set(provider, &key)
                    .map_err(|e| error_text(&e.to_string()))
            },
            move |result| with_ctx(|ctx| ctx.ai_key_stored(provider, result, "API key saved")),
        );
    }

    fn ai_remove_key(self: &Rc<Self>) {
        let (provider, store) = {
            let mut s = self.session.borrow_mut();
            s.ai.saving_key = true;
            (s.ai.panel.provider(), s.ai.store.clone())
        };
        self.ai_refresh();
        runtime::spawn(
            async move {
                store
                    .delete(provider)
                    .map_err(|e| error_text(&e.to_string()))
            },
            move |result| with_ctx(|ctx| ctx.ai_key_stored(provider, result, "API key removed")),
        );
    }

    fn ai_key_stored(
        self: &Rc<Self>,
        provider: AiProviderType,
        result: Result<(), String>,
        done: &str,
    ) {
        let current = {
            let mut s = self.session.borrow_mut();
            s.ai.saving_key = false;
            s.ai.panel.provider() == provider
        };
        match result {
            Ok(()) => {
                if current {
                    self.session.borrow_mut().ai.panel.key_changed();
                    self.ai_check_status();
                }
                self.toast(done, Tone::Success);
            }
            Err(e) => {
                self.session
                    .borrow_mut()
                    .ai
                    .panel
                    .set_notice(format!("Could not use the system keyring: {e}"));
            }
        }
        self.ai_refresh();
    }

    // -- Schema context ----------------------------------------------------------

    /// Build the schema context for the open connection, unless it is already
    /// built (or being built) for this connection and database.
    fn ai_ensure_context(self: &Rc<Self>, force: bool) {
        let (connection_id, driver, database) = {
            let s = self.session.borrow();
            let ws = &s.workspace;
            (
                ws.connection_id.clone(),
                ws.driver.clone(),
                ws.database.clone().unwrap_or_default(),
            )
        };
        let Some(driver) = driver.filter(|_| !connection_id.is_empty()) else {
            return;
        };
        let label = context_database_label(&driver, &database);
        let key = (connection_id.clone(), label.clone());

        let ticket = {
            let mut s = self.session.borrow_mut();
            if force {
                s.ai.panel.invalidate_context();
            }
            if !s.ai.panel.context_wanted(&key) {
                return;
            }
            s.ai.panel.begin_context(key.clone());
            s.ai.context.begin()
        };
        self.ai_refresh();

        let state = self.state.clone();
        runtime::spawn(
            async move {
                service_ai::schema_context(&state, &connection_id, &label, force)
                    .await
                    .map_err(|e| error_text(&e.to_string()))
            },
            move |result| {
                with_ctx(|ctx| {
                    if !ticket.is_current() {
                        return;
                    }
                    ctx.session
                        .borrow_mut()
                        .ai
                        .panel
                        .context_result(&key, result);
                    ctx.ai_refresh();
                })
            },
        );
    }

    // -- Requests -------------------------------------------------------------------

    fn ai_ask(self: &Rc<Self>, kind: Kind) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let prompt = w.global::<AiState>().get_prompt().to_string();
        let chat = w.global::<AiState>().get_chat().to_string();
        let (sql, error) = self.ai_editor();
        let dispatch = {
            let mut s = self.session.borrow_mut();
            s.ai.panel.start(
                kind,
                Inputs {
                    current_sql: &sql,
                    last_error: error.as_deref(),
                    prompt: &prompt,
                    chat: &chat,
                },
            )
        };
        let Some(d) = dispatch else {
            self.ai_refresh();
            return;
        };
        if kind == Kind::Chat {
            // The question is in the conversation now; the box is for the next.
            w.global::<AiState>().set_chat("".into());
        }
        let (ticket, store) = {
            let s = self.session.borrow();
            (s.ai.request.begin(), s.ai.store.clone())
        };
        self.ai_refresh();
        let provider = d.provider;
        let kind = d.kind;
        runtime::spawn(
            async move {
                service_ai::run_task_with(store.as_ref(), d.settings, d.provider, d.model, d.task)
                    .await
                    .map_err(|e| error_text(&e.to_string()))
            },
            move |result| {
                with_ctx(|ctx| {
                    if !ticket.is_current() {
                        return;
                    }
                    ctx.session
                        .borrow_mut()
                        .ai
                        .panel
                        .finish(provider, kind, result);
                    ctx.ai_refresh();
                })
            },
        );
    }

    // -- Applying an answer ----------------------------------------------------------

    /// Put the statement of the answer at `turn` into the editor. Nothing is
    /// run: that is the editor's Run button, with its guard.
    fn ai_apply(self: &Rc<Self>, turn: i32, mode: i32) {
        let Some(sql) = usize::try_from(turn).ok().and_then(|i| {
            self.session
                .borrow()
                .ai
                .panel
                .turn_sql(i)
                .map(str::to_string)
        }) else {
            return;
        };
        let mode = match mode {
            0 => Apply::Replace,
            1 => Apply::Append,
            _ => Apply::NewTab,
        };

        let in_place = {
            let mut s = self.session.borrow_mut();
            match s.workspace.tabs.active_mut() {
                Some(tab) if tab.is_query() && mode != Apply::NewTab => {
                    tab.sql = apply_text(mode, &tab.sql, &sql);
                    if mode == Apply::Replace {
                        // The statement that failed is gone from the editor, so
                        // its error no longer describes it.
                        tab.error = None;
                    }
                    true
                }
                _ => false,
            }
        };
        if in_place {
            self.refresh_workspace();
            self.toast(
                if mode == Apply::Replace {
                    "Replaced the query in the editor"
                } else {
                    "Added to the editor"
                },
                Tone::Success,
            );
        } else {
            self.open_query_tab_with(&sql);
            self.toast("Opened in a new tab", Tone::Success);
        }
        self.ai_refresh();
    }

    fn ai_copy(self: &Rc<Self>, turn: i32) {
        let Some(sql) = usize::try_from(turn).ok().and_then(|i| {
            self.session
                .borrow()
                .ai
                .panel
                .turn_sql(i)
                .map(str::to_string)
        }) else {
            return;
        };
        if let Some(w) = self.window.upgrade() {
            w.invoke_copy_to_clipboard(sql.as_str().into());
        }
        self.toast("Copied the SQL", Tone::Success);
    }

    fn ai_empty_action(self: &Rc<Self>) {
        let (sql, error) = self.ai_editor();
        let kind = {
            let s = self.session.borrow();
            let view = s.ai.panel.view(Inputs {
                current_sql: &sql,
                last_error: error.as_deref(),
                prompt: "",
                chat: "",
            });
            view.empty.map(|e| e.kind)
        };
        match kind {
            Some(EmptyState::NeedsApiKey) => {
                self.session.borrow_mut().ai.panel.show_settings(true);
                self.ai_refresh();
            }
            Some(_) => {
                self.session.borrow_mut().ai.panel.refresh_models();
                self.ai_check_status();
            }
            None => {}
        }
    }
}

/// Connect every `AiState` callback to the controller.
pub(super) fn wire(window: &AppWindow, ctx: &Rc<Ctx>) {
    let a = window.global::<AiState>();

    macro_rules! on {
        ($setter:ident, |$c:ident $(, $arg:ident)*| $body:expr) => {{
            let $c = ctx.clone();
            a.$setter(move |$($arg),*| {
                let $c = &$c;
                $body
            });
        }};
    }

    on!(on_open_requested, |c| {
        c.dispatch(CommandId::GoAiAssistant);
    });
    on!(on_close, |c| c.ai_close());
    on!(on_toggle_settings, |c| {
        c.session.borrow_mut().ai.panel.toggle_settings();
        c.ai_refresh();
    });
    on!(on_provider_selected, |c, i| c.ai_select_provider(i));
    on!(on_model_selected, |c, i| c.ai_select_model(i));
    on!(on_refresh_status, |c| {
        c.session.borrow_mut().ai.panel.refresh_models();
        c.ai_check_status();
    });
    on!(on_save_key, |c| c.ai_save_key());
    on!(on_remove_key, |c| c.ai_remove_key());
    on!(on_save_base_url, |c, url| c.ai_save_base_url(&url));
    on!(on_empty_action_clicked, |c| c.ai_empty_action());
    on!(on_prompt_edited, |c, _text| c.ai_refresh());
    on!(on_generate, |c| c.ai_ask(Kind::Generate));
    on!(on_explain, |c| c.ai_ask(Kind::Explain));
    on!(on_optimize, |c| c.ai_ask(Kind::Optimize));
    on!(on_fix, |c| c.ai_ask(Kind::Fix));
    on!(on_chat_edited, |c, _text| c.ai_refresh());
    on!(on_send_chat, |c| c.ai_ask(Kind::Chat));
    on!(on_toggle_schema, |c| {
        {
            let mut s = c.session.borrow_mut();
            let on = !s.ai.panel.include_schema();
            s.ai.panel.set_include_schema(on);
        }
        c.ai_refresh();
    });
    on!(on_toggle_context, |c| {
        c.session.borrow_mut().ai.panel.toggle_context_viewer();
        c.ai_refresh();
    });
    on!(on_reload_context, |c| c.ai_ensure_context(true));
    on!(on_apply, |c, turn, mode| c.ai_apply(turn, mode));
    on!(on_copy_sql, |c, turn| c.ai_copy(turn));
    on!(on_clear, |c| {
        c.session.borrow_mut().ai.panel.clear();
        c.ai_refresh();
    });
    on!(on_dismiss_notice, |c| {
        c.session.borrow_mut().ai.panel.dismiss_notice();
        c.ai_refresh();
    });
}
