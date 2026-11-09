//! Window-level controller: menu shortcuts, the Theme menu, help links,
//! startup behaviour, auto-save after a successful connection test, window
//! commands, context-menu type-ahead and the crash boundary.
//!
//! A child module of `app` so it reaches `Ctx` without widening it. The
//! decisions are `dbhive_ui_model::{shell, keymap, typeahead}`; this file
//! carries them out.

use std::rc::Rc;
use std::sync::{Once, OnceLock};

use dbhive_core::models::settings::ThemeMode;
use dbhive_ui_model::command_palette::{CommandId, Scope};
use dbhive_ui_model::connection_editor::{build_profile, save_profile, validate};
use dbhive_ui_model::palette_session::live_chord;
use dbhive_ui_model::shell::{
    menu_key_parts, open_url_command, sanitize_panic_message, startup_plan, worker_panic_toast,
    StartupPlan, DOCS_URL, GITHUB_URL,
};
use dbhive_ui_model::typeahead::{is_typeahead_text, next_match};
use slint::{ComponentHandle, Model};

use super::{with_ctx, Ctx};
use crate::{runtime, AppWindow, CtxTypeahead, ShellState, Tone};

impl Ctx {
    // -- Menu shortcuts ---------------------------------------------------------

    /// Give every menu item its live shortcut. Called whenever the keymap is
    /// rebuilt. A chord without Cmd/Ctrl or Alt (the bare "?") is never put
    /// on a native menu: the menu would take that character from every text
    /// field.
    pub(super) fn apply_menu_keys(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let keymap = self.session.borrow().settings.keymap.clone();
        let keys = |id: CommandId, screen: Scope| -> slint::Keys {
            live_chord(&keymap, id, screen)
                .filter(|c| c.command || c.alt)
                .and_then(|c| slint::Keys::from_parts(menu_key_parts(c)).ok())
                .unwrap_or_default()
        };
        let st = w.global::<ShellState>();
        st.set_key_new_connection(keys(CommandId::NewConnection, Scope::Connections));
        st.set_key_settings(keys(CommandId::GoSettings, Scope::Connections));
        st.set_key_palette(keys(CommandId::OpenCommandPalette, Scope::Connections));
        st.set_key_history(keys(CommandId::GoHistory, Scope::Workspace));
        st.set_key_new_tab(keys(CommandId::NewTab, Scope::Workspace));
        st.set_key_close_tab(keys(CommandId::CloseTab, Scope::Workspace));
    }

    // -- Theme ------------------------------------------------------------------

    /// Mark the saved theme mode in the View > Theme menu.
    pub(super) fn show_theme_mode(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let mode = self.session.borrow().settings.editor.draft().theme.mode;
        w.global::<ShellState>().set_theme_mode(match mode {
            ThemeMode::Light => 0,
            ThemeMode::Dark => 1,
            ThemeMode::System => 2,
        });
    }

    fn theme_mode_picked(self: &Rc<Self>, index: i32) {
        let mode = match index {
            0 => ThemeMode::Light,
            1 => ThemeMode::Dark,
            _ => ThemeMode::System,
        };
        // Pick up the OS preference now, in case no change was reported yet.
        if mode == ThemeMode::System {
            if let Some(w) = self.window.upgrade() {
                self.session.borrow_mut().settings.system_dark =
                    super::settings::read_system_dark(&w);
            }
        }
        self.set_theme_mode(mode);
    }

    // -- Links --------------------------------------------------------------------

    fn open_link(self: &Rc<Self>, which: &str) {
        let url = match which {
            "docs" => DOCS_URL,
            "github" => GITHUB_URL,
            _ => return,
        };
        let Some((program, args)) = open_url_command(url, std::env::consts::OS) else {
            return;
        };
        if let Err(e) = std::process::Command::new(&program).args(&args).spawn() {
            self.toast(
                format!("Could not open the browser: {e}. The address is {url}"),
                Tone::Danger,
            );
        }
    }

    // -- Window commands ----------------------------------------------------------

    pub(super) fn window_minimize(&self) {
        if let Some(w) = self.window.upgrade() {
            w.window().set_minimized(true);
        }
    }

    pub(super) fn window_toggle_maximized(&self) {
        if let Some(w) = self.window.upgrade() {
            let win = w.window();
            win.set_maximized(!win.is_maximized());
        }
    }

    // -- Startup ------------------------------------------------------------------

    /// Carry out Settings > General > Startup behavior, once, after the
    /// window is built.
    pub(super) fn run_startup_behavior(self: &Rc<Self>) {
        let settings = self.app_settings();
        let profiles = self.profiles();
        match startup_plan(
            settings.general.startup_behavior,
            settings.general.default_database.as_deref(),
            &profiles,
        ) {
            StartupPlan::ShowConnections => {}
            StartupPlan::Open { profile_id } => self.connect(profile_id),
            StartupPlan::Explain(message) => self.toast(message, Tone::Warning),
        }
    }

    // -- Auto-save after a successful test ----------------------------------------

    /// Settings > General > Auto-save connections: once Test connection
    /// succeeds, save the profile (and its secrets, to the keyring) without
    /// waiting for Save. The dialog stays open; it now edits the saved
    /// profile, so a later Save updates it rather than adding a second one.
    pub(super) fn auto_save_after_test(self: &Rc<Self>) {
        if !self.app_settings().general.auto_save_connections {
            return;
        }
        let (profile, password, name, is_new) = {
            let mut s = self.session.borrow_mut();
            if s.editor.saving || !validate(&s.editor.form).is_valid() {
                return;
            }
            // Pin the id first, so Test, auto-save and Save all mean the same
            // profile.
            if s.editor.form.id.is_none() {
                s.editor.form.id = Some(uuid::Uuid::new_v4().to_string());
            }
            let profile = build_profile(&s.editor.form);
            s.editor.saving = true;
            (
                profile.clone(),
                s.editor.password.clone(),
                profile.name,
                s.editor.is_new,
            )
        };
        let id = profile.id.clone();
        let ssh_password = self.editor_ssh_password();
        let state = self.state.clone();
        runtime::spawn(
            async move { save_profile(state, profile, id, password, ssh_password).await },
            move |result| {
                with_ctx(|ctx| {
                    {
                        let mut s = ctx.session.borrow_mut();
                        s.editor.saving = false;
                        match &result {
                            Ok(_) => {
                                s.editor.is_new = false;
                                s.editor.test_message = if is_new {
                                    "Connected successfully. Saved to your connections.".into()
                                } else {
                                    "Connected successfully. Changes saved.".into()
                                };
                            }
                            Err(e) => {
                                s.editor.test_message = format!(
                                    "Connected successfully, but it could not be saved: {e}"
                                );
                            }
                        }
                    }
                    ctx.refresh_editor();
                    ctx.refresh_connections();
                    if let Ok((_, Some(warning))) = result {
                        ctx.toast(warning, Tone::Warning);
                    } else if result.is_ok() && is_new {
                        ctx.toast(format!("Added {name}"), Tone::Success);
                    }
                })
            },
        );
    }

    // -- Crash boundary -------------------------------------------------------------

    /// Background work panicked. Its own completion will never run, so this
    /// is the only word the user gets: say it, in plain terms. The sanitized
    /// detail is already in the log (see [`install_panic_hook`]).
    fn worker_panicked(self: &Rc<Self>) {
        self.toast(worker_panic_toast(), Tone::Danger);
    }
}

/// Called on the UI thread by `runtime::spawn` when a worker task panicked.
pub(crate) fn report_worker_panic() {
    with_ctx(|ctx| ctx.worker_panicked());
}

/// Log every panic, on any thread, with secrets stripped (see
/// `dbhive_ui_model::shell::sanitize_panic_message`), to stderr and to
/// `crash.log` in the data folder. The default hook is replaced rather than
/// chained because it prints the raw message.
///
/// A panic on a worker task is also turned into a toast (by
/// `runtime::spawn`); a panic on the UI thread ends the event loop, so the
/// log is all that is left of it.
pub fn install_panic_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            let payload = info.payload();
            let raw = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "(no message)".to_string());
            let thread = std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_string();
            let location = info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_default();
            let line = format!(
                "{} panic on thread '{}' at {}: {}",
                chrono::Utc::now().to_rfc3339(),
                thread,
                location,
                sanitize_panic_message(&raw)
            );
            eprintln!("{line}");
            if let Ok(dir) = dbhive_core::storage::app_data_dir() {
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join("crash.log"))
                {
                    let _ = writeln!(f, "{line}");
                }
            }
        }));
    });
}

/// Hook the `ShellState` and `CtxTypeahead` globals to the controller.
pub(super) fn wire(window: &AppWindow, ctx: &Rc<Ctx>) {
    let st = window.global::<ShellState>();
    {
        let ctx = ctx.clone();
        st.on_theme_mode_picked(move |i| ctx.theme_mode_picked(i));
    }
    {
        let ctx = ctx.clone();
        st.on_open_link(move |which| ctx.open_link(&which));
    }
    {
        let ctx = ctx.clone();
        st.on_os_color_scheme_changed(move || ctx.system_scheme_changed());
    }

    let ta = window.global::<CtxTypeahead>();
    ta.on_is_key(|text| is_typeahead_text(&text));
    ta.on_now(|| {
        static START: OnceLock<std::time::Instant> = OnceLock::new();
        START
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_millis() as i32
    });
    ta.on_next(|items, current, typed| {
        let labels: Vec<(String, bool)> = items
            .iter()
            .map(|it| (it.label.to_string(), it.enabled))
            .collect();
        let refs: Vec<(&str, bool)> = labels.iter().map(|(l, e)| (l.as_str(), *e)).collect();
        next_match(&refs, current, &typed).map_or(-1, |i| i as i32)
    });
}
