//! The Settings dialog's controller.
//!
//! A child module of `app` so it can reach `Ctx` and `Session` without
//! widening their visibility, and so the Settings wiring stays out of the
//! middle of `app.rs`. The decisions live in
//! `dbhive_ui_model::settings_editor`; this file moves state between that
//! editor, the window, and `settings.json`.
//!
//! Three things happen live, before Save, because a colour you cannot see is
//! a colour you cannot judge: theme mode, accent colour, and the SQL editor's
//! font size and line numbers. Everything else waits for Save.
//!
//! The saved shortcuts are also turned into the live [`Keymap`] here, every
//! time they change, so dispatch, the help, the palette and the menus follow
//! a rebind at once.

use std::rc::Rc;

use dbhive_core::models::settings::{AppSettings, ThemeMode};
use dbhive_core::models::DbError;
use dbhive_core::settings as core_settings;
use dbhive_ui_model::keymap::{binding_problem, Keymap};
use dbhive_ui_model::settings::{theme_mode_for, ResolvedMode, ShortcutField};
use dbhive_ui_model::settings_editor::{
    shortcut_fields, NumberField, RecordOutcome, SettingsEditor,
};
use slint::ComponentHandle;

use super::{with_ctx, Ctx, PendingAction};
use crate::settings_view::{self, Banner, Load, View};
use crate::{AppWindow, Theme, Tone};

/// Settings state held in the session.
pub(super) struct SettingsState {
    pub editor: SettingsEditor,
    pub view: View,
    /// The OS light/dark preference. Read when the window opens and again
    /// whenever the OS reports a change (`ShellState.os-color-scheme`), so
    /// the "System" theme mode follows the OS live.
    pub system_dark: Option<bool>,
    /// The live key bindings, built from the *saved* shortcuts.
    pub keymap: Keymap,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            editor: SettingsEditor::new(AppSettings::default()),
            view: View::default(),
            system_dark: None,
            keymap: Keymap::default(),
        }
    }
}

pub(super) fn read_system_dark(w: &AppWindow) -> Option<bool> {
    // `Palette` is the style's own colour-scheme global; the format avoids
    // naming the generated enum.
    let scheme = format!("{:?}", w.global::<crate::Palette>().get_color_scheme());
    match scheme.as_str() {
        "Dark" => Some(true),
        "Light" => Some(false),
        _ => None,
    }
}

impl Ctx {
    /// The settings as last saved. What the rest of the app should obey: the
    /// draft is only a preview until Save.
    pub(super) fn app_settings(&self) -> AppSettings {
        self.session.borrow().settings.editor.saved().clone()
    }

    /// Read `settings.json` once, at startup, on the UI thread. It is one
    /// small file and the theme has to be right before the first frame
    /// (the same trade `load_persisted` makes for profiles). A failure is
    /// kept and shown in the dialog, with a retry; the app runs on defaults.
    pub(super) fn init_settings(&self) {
        let system_dark = self.window.upgrade().and_then(|w| read_system_dark(&w));
        let loaded = core_settings::load();
        {
            let mut s = self.session.borrow_mut();
            s.settings.system_dark = system_dark;
            match loaded {
                Ok(settings) => s.settings.editor.adopt(settings),
                Err(e) => s.settings.view.load = Load::Failed(e.to_string()),
            }
        }
        self.rebuild_keymap();
        self.apply_appearance();
        self.refresh_settings();
    }

    /// Rebuild the live keymap from the saved shortcuts and push it to the
    /// menus. Called whenever the saved shortcuts may have changed.
    pub(super) fn rebuild_keymap(&self) {
        {
            let mut s = self.session.borrow_mut();
            let map = Keymap::from_settings(&s.settings.editor.saved().shortcuts);
            s.settings.keymap = map;
        }
        self.apply_menu_keys();
        self.refresh_shortcuts();
    }

    /// The OS switched between light and dark. Only the "System" theme mode
    /// changes what is on screen; the others ignore it.
    pub(super) fn system_scheme_changed(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let now = read_system_dark(&w);
        {
            let mut s = self.session.borrow_mut();
            if s.settings.system_dark == now {
                return;
            }
            s.settings.system_dark = now;
        }
        self.apply_appearance();
        self.refresh_history_panel();
    }

    /// Project the editor into the dialog.
    pub(super) fn refresh_settings(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let profiles = self.profiles();
        let data = {
            let s = self.session.borrow();
            let d = settings_view::data(&s.settings.editor, &s.settings.view);
            settings_view::with_default_connection(d, &s.settings.editor, &profiles)
        };
        w.set_settings(data);
    }

    /// Paint the window from the draft: theme mode, accent colour, and the
    /// SQL editor's font size and line numbers.
    pub(super) fn apply_appearance(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let a = {
            let s = self.session.borrow();
            s.settings.editor.appearance(s.settings.system_dark)
        };
        let theme = w.global::<Theme>();
        theme.set_dark(a.mode == ResolvedMode::Dark);
        theme.set_accent_custom(a.accent.is_some());
        if let Some((r, g, b)) = a.accent {
            theme.set_accent_custom_color(slint::Color::from_rgb_u8(r, g, b));
            let (fr, fg, fb) = a.accent_foreground;
            theme.set_accent_custom_foreground(slint::Color::from_rgb_u8(fr, fg, fb));
        }
        theme.set_editor_font_size(a.editor_font_size as f32);
        theme.set_editor_line_numbers(a.editor_line_numbers);
        // Font family, word wrap and minimap come from the same draft.
        self.editor_prefs_apply();
        self.show_theme_mode();
        // Syntax colours are chosen per theme in Rust.
        self.refresh_workspace();
    }

    fn open_settings(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        // The connection editor holds typed work; do not stack on top of it.
        if w.get_show_connection_editor() {
            return;
        }
        {
            let mut s = self.session.borrow_mut();
            s.settings.view.banner = None;
            s.settings.editor.stop_recording();
        }
        self.refresh_settings();
        w.set_show_settings(true);
    }

    fn hide_settings(&self) {
        {
            let mut s = self.session.borrow_mut();
            s.settings.view.banner = None;
            s.settings.editor.stop_recording();
        }
        if let Some(w) = self.window.upgrade() {
            w.set_show_settings(false);
        }
        self.refresh_settings();
    }

    /// Close, asking first when there are unsaved edits.
    fn close_settings(&self) {
        if self.session.borrow().settings.editor.is_dirty() {
            self.session.borrow_mut().pending_action = Some(PendingAction::DiscardSettings);
            self.show_confirm(
                "Discard unsaved changes?",
                "Your edits to settings have not been saved.",
                "Discard",
            );
        } else {
            self.hide_settings();
        }
    }

    pub(super) fn discard_settings_and_close(&self) {
        self.session.borrow_mut().settings.editor.discard();
        self.apply_appearance();
        self.hide_settings();
    }

    fn discard_settings(&self) {
        self.session.borrow_mut().settings.editor.discard();
        self.apply_appearance();
        self.refresh_settings();
    }

    /// Edit the draft, then show the result and any live effect. Ignored
    /// while a write is in flight: the result replaces the draft, so an edit
    /// made now would vanish.
    fn edit_settings(&self, f: impl FnOnce(&mut SettingsEditor)) {
        {
            let mut s = self.session.borrow_mut();
            if s.settings.view.saving || s.settings.view.load != Load::Ready {
                return;
            }
            f(&mut s.settings.editor);
        }
        self.apply_appearance();
        self.refresh_settings();
    }

    fn set_number(&self, field: NumberField, text: &str) {
        self.edit_settings(|e| e.set_number_text(field, text));
    }

    fn set_banner(&self, banner: Option<Banner>) {
        self.session.borrow_mut().settings.view.banner = banner;
        self.refresh_settings();
    }

    // -- Saving --------------------------------------------------------------

    fn save_settings(self: &Rc<Self>) {
        let to_write = {
            let mut s = self.session.borrow_mut();
            let st = &mut s.settings;
            if st.view.saving || !st.editor.can_save() {
                return;
            }
            st.view.saving = true;
            st.view.banner = None;
            st.editor.draft().clone()
        };
        self.refresh_settings();
        crate::runtime::spawn(
            self.ui_id,
            async move { core_settings::save(&to_write) },
            |res| with_ctx(|ctx| ctx.settings_written(res, "Settings saved")),
        );
    }

    /// Reset everything to defaults and save them. The editor only changes
    /// once the write succeeds, so a failed write leaves screen and disk in
    /// agreement.
    pub(super) fn reset_settings(self: &Rc<Self>) {
        {
            let mut s = self.session.borrow_mut();
            if s.settings.view.saving {
                return;
            }
            s.settings.view.saving = true;
            s.settings.view.banner = None;
        }
        self.refresh_settings();
        crate::runtime::spawn(self.ui_id, async move { core_settings::reset() }, |res| {
            with_ctx(|ctx| ctx.settings_written(res, "Settings reset to defaults"))
        });
    }

    fn settings_written(self: &Rc<Self>, res: Result<AppSettings, DbError>, ok_message: &str) {
        match res {
            Ok(written) => {
                {
                    let mut s = self.session.borrow_mut();
                    s.settings.editor.adopt(written);
                    s.settings.view.saving = false;
                    s.settings.view.load = Load::Ready;
                    s.settings.view.banner = None;
                }
                self.rebuild_keymap();
                self.apply_appearance();
                self.refresh_settings();
                self.toast(ok_message, Tone::Success);
            }
            Err(e) => {
                {
                    let mut s = self.session.borrow_mut();
                    s.settings.view.saving = false;
                    s.settings.view.banner = Some(Banner {
                        message: format!("Could not save settings: {}", e),
                        tone: Tone::Danger,
                    });
                }
                self.refresh_settings();
            }
        }
    }

    fn retry_settings(self: &Rc<Self>) {
        self.session.borrow_mut().settings.view.load = Load::Loading;
        self.refresh_settings();
        crate::runtime::spawn(self.ui_id, async move { core_settings::load() }, |res| {
            with_ctx(|ctx| {
                {
                    let mut s = ctx.session.borrow_mut();
                    match res {
                        Ok(settings) => {
                            s.settings.editor.adopt(settings);
                            s.settings.view.load = Load::Ready;
                        }
                        Err(e) => s.settings.view.load = Load::Failed(e.to_string()),
                    }
                }
                ctx.rebuild_keymap();
                ctx.apply_appearance();
                ctx.refresh_settings();
            })
        });
    }

    /// The title-bar and View-menu light/dark toggle. It is a one-click
    /// preference, saved at once, and does not make the form dirty.
    pub(super) fn toggle_theme_mode(self: &Rc<Self>) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let was_dark = w.global::<Theme>().get_dark();
        self.set_theme_dark(!was_dark);
    }

    /// Switch to the light or dark theme and save it as the theme mode. The
    /// palette's theme commands, the View menu and the title-bar button all
    /// come through here, so they cannot disagree about what gets saved.
    pub(super) fn set_theme_dark(self: &Rc<Self>, dark: bool) {
        self.set_theme_mode(theme_mode_for(if dark {
            ResolvedMode::Dark
        } else {
            ResolvedMode::Light
        }));
    }

    /// Save `mode` (Light, Dark or System) as the theme mode and show it.
    /// The View > Theme menu comes here directly; System then follows the
    /// OS through [`Self::system_scheme_changed`].
    pub(super) fn set_theme_mode(self: &Rc<Self>, mode: ThemeMode) {
        let (to_write, can_persist) = {
            let mut s = self.session.borrow_mut();
            let can_persist = s.settings.view.load == Load::Ready;
            (s.settings.editor.set_theme_mode_saved(mode), can_persist)
        };
        self.apply_appearance();
        self.refresh_settings();
        // The editor's syntax colours are chosen per theme in Rust.
        self.refresh_workspace();
        self.refresh_history_panel();
        // A settings file that failed to load is not overwritten from here.
        if can_persist {
            crate::runtime::spawn(
                self.ui_id,
                async move { core_settings::save(&to_write) },
                |res| {
                    if let Err(e) = res {
                        with_ctx(|ctx| {
                            ctx.toast(format!("Could not save the theme: {}", e), Tone::Danger)
                        });
                    }
                },
            );
        }
    }

    // -- Shortcuts -----------------------------------------------------------

    fn record_shortcut_key(
        self: &Rc<Self>,
        text: &str,
        control: bool,
        alt: bool,
        shift: bool,
        meta: bool,
    ) {
        let is_mac = cfg!(target_os = "macos");
        let key = settings_view::key_label(text);
        let mods = settings_view::key_modifiers(control, alt, shift, meta, is_mac);
        let (field, outcome) = {
            let mut s = self.session.borrow_mut();
            if s.settings.view.saving {
                return;
            }
            let field = s.settings.editor.recording();
            (field, s.settings.editor.record(mods, &key, is_mac))
        };
        match outcome {
            RecordOutcome::Idle | RecordOutcome::Waiting => {}
            RecordOutcome::Cancelled => self.session.borrow_mut().settings.view.banner = None,
            RecordOutcome::Cleared => {
                self.session.borrow_mut().settings.view.banner = None;
                self.persist_shortcuts();
            }
            RecordOutcome::Bound { combo, warning } => {
                // A binding the keymap cannot dispatch is still saved (it is
                // what the person pressed), but they are told it will not fire.
                let problem = field.and_then(|f| binding_problem(f, &combo));
                let message = match (warning, problem) {
                    (Some(w), Some(p)) => Some(format!("{w} {p}")),
                    (w, p) => w.or(p),
                };
                self.session.borrow_mut().settings.view.banner = message.map(|message| Banner {
                    message,
                    tone: Tone::Warning,
                });
                self.persist_shortcuts();
            }
            RecordOutcome::Invalid(message) => {
                self.session.borrow_mut().settings.view.banner = Some(Banner {
                    message,
                    tone: Tone::Warning,
                });
            }
        }
        self.refresh_settings();
    }

    /// A shortcut change is saved the moment it is made, on its own, so an
    /// unsaved font size is not saved by accident.
    fn persist_shortcuts(&self) {
        let to_write = self
            .session
            .borrow()
            .settings
            .editor
            .saved_with_draft_shortcuts();
        crate::runtime::spawn(
            self.ui_id,
            async move { core_settings::save(&to_write) },
            |res| {
                with_ctx(|ctx| {
                    match res {
                        Ok(written) => {
                            ctx.session
                                .borrow_mut()
                                .settings
                                .editor
                                .mark_shortcuts_saved(&written.shortcuts);
                            ctx.rebuild_keymap();
                        }
                        Err(e) => {
                            ctx.session.borrow_mut().settings.view.banner = Some(Banner {
                                message: format!("Could not save the shortcut: {}", e),
                                tone: Tone::Danger,
                            })
                        }
                    }
                    ctx.refresh_settings();
                })
            },
        );
    }

    fn shortcut_at(index: i32) -> Option<ShortcutField> {
        usize::try_from(index)
            .ok()
            .and_then(|i| shortcut_fields().get(i).copied())
    }
}

/// `edit!(window, ctx, on_settings_x, |ed, a| body)`: one settings callback
/// that edits the draft.
macro_rules! edit_cb {
    ($window:expr, $ctx:expr, $setter:ident, |$ed:ident $(, $arg:ident)*| $body:expr) => {{
        let ctx = $ctx.clone();
        $window.$setter(move |$($arg),*| {
            ctx.edit_settings(|$ed| $body);
        });
    }};
}

/// Hook every `settings-*` callback and the title-bar gear to the controller.
pub(super) fn wire(window: &AppWindow, ctx: &Rc<Ctx>) {
    {
        let ctx = ctx.clone();
        window.on_open_settings(move || ctx.open_settings());
    }
    {
        let ctx = ctx.clone();
        window.on_settings_close(move || ctx.close_settings());
    }
    {
        let ctx = ctx.clone();
        window.on_settings_save(move || ctx.save_settings());
    }
    {
        let ctx = ctx.clone();
        window.on_settings_discard(move || ctx.discard_settings());
    }
    {
        let ctx = ctx.clone();
        window.on_settings_reset(move || {
            ctx.session.borrow_mut().pending_action = Some(PendingAction::ResetSettings);
            ctx.show_confirm(
                "Reset all settings to defaults?",
                "This restores every setting, not only shortcuts, to its default value. It cannot be undone.",
                "Reset",
            );
        });
    }
    {
        let ctx = ctx.clone();
        window.on_settings_retry(move || ctx.retry_settings());
    }
    {
        let ctx = ctx.clone();
        window.on_settings_dismiss_banner(move || ctx.set_banner(None));
    }

    edit_cb!(window, ctx, on_settings_section_selected, |e, i| e
        .select_section_index(i.max(0) as usize));
    edit_cb!(window, ctx, on_settings_startup_selected, |e, i| e
        .set_startup_index(i.max(0) as usize));
    {
        let ctx = ctx.clone();
        window.on_settings_default_connection_selected(move |i| {
            let profiles = ctx.profiles();
            let id = dbhive_ui_model::shell::default_connection_at(&profiles, i.max(0) as usize);
            ctx.edit_settings(|e| e.edit(|s| s.general.default_database = id));
        });
    }
    edit_cb!(window, ctx, on_settings_toggle_auto_save_connections, |e| e
        .edit(|s| {
            s.general.auto_save_connections = !s.general.auto_save_connections
        }));
    edit_cb!(window, ctx, on_settings_toggle_auto_check_updates, |e| {
        let on = !e.draft().general.auto_check_updates;
        e.set_auto_check(on)
    });
    edit_cb!(window, ctx, on_settings_toggle_auto_download, |e| {
        let on = !e.draft().general.auto_download_updates;
        e.set_auto_download(on)
    });
    edit_cb!(window, ctx, on_settings_toggle_auto_install, |e| e.edit(
        |s| { s.general.auto_install_updates = !s.general.auto_install_updates }
    ));
    edit_cb!(window, ctx, on_settings_theme_selected, |e, i| e
        .set_theme_mode_index(i.max(0) as usize));
    edit_cb!(window, ctx, on_settings_accent_preset_picked, |e, i| e
        .pick_accent_preset(i));
    edit_cb!(window, ctx, on_settings_accent_text_changed, |e, t| e
        .set_accent_text(&t));
    edit_cb!(window, ctx, on_settings_font_family_changed, |e, t| e
        .edit(|s| { s.theme.editor_font_family = t.to_string() }));
    edit_cb!(window, ctx, on_settings_toggle_line_numbers, |e| e.edit(
        |s| { s.theme.editor_line_numbers = !s.theme.editor_line_numbers }
    ));
    edit_cb!(window, ctx, on_settings_toggle_minimap, |e| e
        .edit(|s| { s.theme.editor_minimap = !s.theme.editor_minimap }));
    edit_cb!(window, ctx, on_settings_toggle_word_wrap, |e| e.edit(|s| {
        s.theme.editor_word_wrap = !s.theme.editor_word_wrap
    }));
    edit_cb!(window, ctx, on_settings_toggle_confirm_destructive, |e| e
        .edit(|s| {
            s.query.confirm_destructive = !s.query.confirm_destructive
        }));
    edit_cb!(window, ctx, on_settings_toggle_auto_save_history, |e| e
        .edit(|s| {
            s.query.auto_save_history = !s.query.auto_save_history
        }));
    edit_cb!(window, ctx, on_settings_shortcut_record, |e, i| e
        .start_recording_index(i.max(0) as usize));
    edit_cb!(window, ctx, on_settings_shortcut_clear, |e, i| e
        .clear_shortcut_index(i.max(0) as usize));

    macro_rules! number_cb {
        ($setter:ident, $field:expr) => {{
            let ctx = ctx.clone();
            window.$setter(move |t| ctx.set_number($field, &t));
        }};
    }
    number_cb!(
        on_settings_update_interval_changed,
        NumberField::UpdateInterval
    );
    number_cb!(on_settings_font_size_changed, NumberField::EditorFontSize);
    number_cb!(on_settings_timeout_changed, NumberField::QueryTimeout);
    number_cb!(on_settings_max_rows_changed, NumberField::MaxRows);
    number_cb!(on_settings_max_history_changed, NumberField::MaxHistory);

    {
        let ctx = ctx.clone();
        window.on_settings_shortcut_key(move |text, control, alt, shift, meta| {
            ctx.record_shortcut_key(&text, control, alt, shift, meta)
        });
    }
}
