//! Settings.
//!
//! Parity target: the React `SettingsPage` (`/settings`, also an overlay
//! dialog on Ctrl+,). Four sections behind a left nav: General, Appearance,
//! Query Execution, Keyboard Shortcuts.
//!
//! Save semantics follow the web shell: General/Appearance/Query edits are
//! held in a draft and written by "Save Changes", while a shortcut rebinding
//! persists the moment it is recorded (a half-applied keymap is worse than
//! an unsaved font size). "Reset to Defaults" resets the whole object, behind
//! a confirmation.
//!
//! Theme mode, accent colour and editor font size apply live as they are
//! edited, before saving, because you cannot judge a colour you cannot see.
//!
//! NOTE on the accent colour: in the React shell this control wrote
//! `theme.accentColor` and nothing ever read it, so it did nothing at all.
//! Here it is wired: [`apply_theme`] overrides every visual the palette
//! derives from `primary` (selection fill/stroke, hyperlinks, the active and
//! hovered widget strokes) with the chosen colour, and picks a readable
//! foreground for text sitting on it. The remaining gap is that screens which
//! read `palette.primary` directly still get the static brand honey; closing
//! that needs `theme::Mode::palette()` to return a runtime-overridable
//! palette, which is a change to `theme.rs`.
//!
//! The pure logic (sections, shortcut fields and validation, keystroke
//! formatting, theme-mode resolution, readable text on the accent) lives in
//! `dbhive_ui_model::settings` and is re-exported here; this file keeps the
//! drawing, the egui adapters and the job glue.

use dbhive_core::models::settings::{AppSettings, ThemeMode};
use dbhive_core::models::DbError;
use dbhive_core::settings as core_settings;

pub use dbhive_ui_model::settings::*;

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::theme::{text, Mode, Palette};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// What was applied to the egui context last, so the visuals are only rebuilt
/// when something actually changed rather than every frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Applied {
    mode: Mode,
    accent: (u8, u8, u8),
    font_size: u32,
}

pub struct Settings {
    /// Set by the shell when Settings is shown as an overlay dialog.
    pub open: bool,
    pub section: Section,

    /// The edited copy. Everything the UI binds to.
    pub draft: AppSettings,
    /// The last state known to be on disk, for discarding edits.
    saved: AppSettings,
    /// Unsaved General/Appearance/Query edits.
    pub dirty: bool,

    load_job: Job<Result<AppSettings, DbError>>,
    save_job: Job<Result<AppSettings, DbError>>,
    load_requested: bool,

    /// Hex text buffer, kept apart from the model so half-typed input like
    /// "#f5" never reaches it.
    accent_input: String,
    recording: Option<ShortcutField>,
    confirm_reset: bool,
    applied: Option<Applied>,
}

impl Default for Settings {
    fn default() -> Self {
        let draft = AppSettings::default();
        Self {
            open: false,
            section: Section::General,
            accent_input: draft.theme.accent_color.clone(),
            saved: draft.clone(),
            draft,
            dirty: false,
            load_job: Job::default(),
            save_job: Job::default(),
            load_requested: false,
            recording: None,
            confirm_reset: false,
            applied: None,
        }
    }
}

impl Settings {
    fn adopt(&mut self, settings: AppSettings) {
        self.accent_input = settings.theme.accent_color.clone();
        self.saved = settings.clone();
        self.draft = settings;
        self.dirty = false;
    }

    /// Accent as RGB, falling back to the brand honey if the stored value is
    /// unparseable.
    fn accent_rgb(&self) -> (u8, u8, u8) {
        accent_rgb(&self.draft.theme.accent_color)
    }
}

// ---------------------------------------------------------------------------
// Loading, saving, theme application
// ---------------------------------------------------------------------------

/// Kick off the one-time read of `settings.json`.
///
/// Safe to call every frame; only the first call spawns anything. The shell
/// should call this at startup so the saved theme applies even when the
/// Settings screen is never opened.
pub fn ensure_loaded(app: &mut DbHiveApp, ctx: &egui::Context) {
    if app.screens.settings.load_requested {
        return;
    }
    app.screens.settings.load_requested = true;
    app.screens
        .settings
        .load_job
        .start(app.rt.handle(), ctx, async move { core_settings::load() });
}

/// Poll the load/save jobs and report their outcome. Call once per frame.
pub fn poll(app: &mut DbHiveApp) {
    if let Some(result) = app.screens.settings.load_job.take_result() {
        match result {
            Ok(loaded) => {
                // Do not stamp on edits the user made while the read was in
                // flight; the file is only authoritative until they touch it.
                if !app.screens.settings.dirty {
                    app.screens.settings.adopt(loaded);
                }
            }
            Err(e) => app.toasts.error(format!("Failed to load settings: {}", e)),
        }
    }

    if let Some(result) = app.screens.settings.save_job.take_result() {
        match result {
            Ok(written) => {
                app.screens.settings.adopt(written);
                app.toasts.success("Settings saved");
            }
            Err(e) => app.toasts.error(format!("Failed to save settings: {}", e)),
        }
    }
}

/// Resolve the configured theme mode against the OS preference.
pub fn resolve_mode(ctx: &egui::Context, mode: ThemeMode) -> Mode {
    let system_dark = ctx.system_theme().map(|t| t == egui::Theme::Dark);
    mode_from(resolve_theme_mode(mode, system_dark))
}

fn mode_from(resolved: ResolvedMode) -> Mode {
    match resolved {
        ResolvedMode::Light => Mode::Light,
        ResolvedMode::Dark => Mode::Dark,
    }
}

fn resolved_from(mode: Mode) -> ResolvedMode {
    match mode {
        Mode::Light => ResolvedMode::Light,
        Mode::Dark => ResolvedMode::Dark,
    }
}

/// Apply the palette, the accent override and the editor font size.
///
/// Call once per change, not once per frame: `set_visuals` invalidates
/// egui's style cache.
pub fn apply_theme(ctx: &egui::Context, mode: Mode, accent: (u8, u8, u8), font_size: u32) {
    crate::theme::apply(ctx, mode);

    let accent = egui::Color32::from_rgb(accent.0, accent.1, accent.2);
    let on_accent = readable_on(accent);

    let mut v = ctx.style().visuals.clone();
    v.selection.bg_fill = accent.linear_multiply(0.35);
    v.selection.stroke = egui::Stroke::new(1.0_f32, accent);
    v.hyperlink_color = accent;
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, accent);
    v.widgets.active.bg_fill = accent;
    v.widgets.active.weak_bg_fill = accent;
    v.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, accent);
    // Text drawn on top of the accent. `theme::text::strong` sets its own
    // colour, so this only affects pressed-widget labels, which is what it
    // is for.
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, on_accent);
    ctx.set_visuals(v);

    // The SQL editor renders through `TextStyle::Monospace`, so resizing that
    // style is what makes the editor font-size setting real.
    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::monospace(font_size as f32),
    );
    ctx.set_style(style);
}

/// Black or white, whichever stays legible on `bg`. The decision is
/// [`readable_on_rgb`].
pub fn readable_on(bg: egui::Color32) -> egui::Color32 {
    let (r, g, b) = readable_on_rgb((bg.r(), bg.g(), bg.b()));
    egui::Color32::from_rgb(r, g, b)
}

/// Keep the egui visuals in step with the settings. Cheap; call every frame.
///
/// Also adopts a mode change made elsewhere (the title-bar Light/Dark toggle)
/// into the draft, so the two controls agree instead of fighting each other
/// frame by frame.
pub fn ensure_theme(app: &mut DbHiveApp, ctx: &egui::Context) {
    if let Some(applied) = app.screens.settings.applied {
        if app.mode != applied.mode {
            let s = &mut app.screens.settings;
            s.draft.theme.mode = theme_mode_for(resolved_from(app.mode));
            s.dirty = true;
            s.applied = None;
        }
    }

    let want = Applied {
        mode: resolve_mode(ctx, app.screens.settings.draft.theme.mode),
        accent: app.screens.settings.accent_rgb(),
        font_size: app.screens.settings.draft.theme.editor_font_size,
    };
    if app.screens.settings.applied != Some(want) {
        apply_theme(ctx, want.mode, want.accent, want.font_size);
        app.screens.settings.applied = Some(want);
    }
    app.mode = want.mode;
}

/// The accent colour to draw with: the user's choice, or the brand primary.
///
/// Screens that currently reach for `palette.primary` can move to this to
/// follow the accent setting.
pub fn accent(app: &DbHiveApp) -> egui::Color32 {
    let (r, g, b) = app.screens.settings.accent_rgb();
    egui::Color32::from_rgb(r, g, b)
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Draw Settings as an overlay dialog when `open` is set.
pub fn show_window(app: &mut DbHiveApp, ctx: &egui::Context) {
    if !app.screens.settings.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Settings")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size(egui::vec2(860.0, 620.0))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            show(app, ui);
        });
    if !open {
        app.screens.settings.open = false;
    }
}

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    ensure_loaded(app, ui.ctx());
    poll(app);
    ensure_theme(app, ui.ctx());

    let palette = app.mode.palette();
    let saving = app.screens.settings.save_job.is_running();
    let loading = app.screens.settings.load_job.is_running();

    // Immediate mode: the widgets record intent, and it is acted on once the
    // borrow of the screen state has ended.
    let mut actions: Vec<Action> = Vec::new();

    egui::SidePanel::left("settings_nav")
        .exact_width(200.0)
        .resizable(false)
        .frame(
            egui::Frame::none()
                .fill(palette.sidebar)
                .inner_margin(egui::Margin::same(12.0)),
        )
        .show_inside(ui, |ui| {
            ui.label(text::strong(palette, "Settings"));
            ui.add_space(10.0);

            let current = app.screens.settings.section;
            for section in Section::ALL {
                if ui
                    .add_sized(
                        [ui.available_width(), 28.0],
                        egui::SelectableLabel::new(current == section, section.label()),
                    )
                    .clicked()
                {
                    app.screens.settings.section = section;
                }
            }

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);

            let dirty = app.screens.settings.dirty;
            ui.add_enabled_ui(!saving && !loading, |ui| {
                let label = if saving { "Saving…" } else { "Save Changes" };
                if ui
                    .add_sized([ui.available_width(), 28.0], egui::Button::new(label))
                    .clicked()
                {
                    actions.push(Action::Save);
                }
                if ui
                    .add_sized(
                        [ui.available_width(), 28.0],
                        egui::Button::new("Reset to Defaults"),
                    )
                    .clicked()
                {
                    actions.push(Action::AskReset);
                }
                if dirty
                    && ui
                        .add_sized(
                            [ui.available_width(), 28.0],
                            egui::Button::new("Discard Changes"),
                        )
                        .clicked()
                {
                    actions.push(Action::Discard);
                }
            });

            if dirty {
                ui.add_space(6.0);
                ui.label(text::muted(palette, "Unsaved changes"));
            }
            if loading {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label(text::muted(palette, "Loading settings…"));
                });
            }
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::none().inner_margin(egui::Margin::same(16.0)))
        .show_inside(ui, |ui| {
            let section = app.screens.settings.section;
            ui.label(text::heading(palette, section.title()));
            ui.label(text::muted(palette, section.blurb()));
            ui.add_space(10.0);

            egui::ScrollArea::vertical()
                .id_salt("settings_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let s = &mut app.screens.settings;
                    match section {
                        Section::General => general_section(s, ui, palette),
                        Section::Appearance => appearance_section(s, ui, palette),
                        Section::Query => query_section(s, ui, palette),
                        Section::Shortcuts => shortcuts_section(s, ui, palette, &mut actions),
                    }
                });
        });

    if app.screens.settings.confirm_reset {
        confirm_reset_dialog(app, ui.ctx(), &mut actions);
    }

    for action in actions {
        apply_action(app, ui.ctx(), action);
    }
}

enum Action {
    Save,
    Discard,
    AskReset,
    CancelReset,
    Reset,
    /// Shortcut rebinding, persisted immediately.
    SaveShortcuts,
    Warn(String),
    Error(String),
}

fn apply_action(app: &mut DbHiveApp, ctx: &egui::Context, action: Action) {
    match action {
        Action::Save | Action::SaveShortcuts => {
            let settings = app.screens.settings.draft.clone();
            app.screens
                .settings
                .save_job
                .start(app.rt.handle(), ctx, async move {
                    core_settings::save(&settings)
                });
        }
        Action::Discard => {
            let saved = app.screens.settings.saved.clone();
            app.screens.settings.adopt(saved);
        }
        Action::AskReset => app.screens.settings.confirm_reset = true,
        Action::CancelReset => app.screens.settings.confirm_reset = false,
        Action::Reset => {
            app.screens.settings.confirm_reset = false;
            app.screens.settings.adopt(AppSettings::default());
            app.screens
                .settings
                .save_job
                .start(app.rt.handle(), ctx, async move { core_settings::reset() });
        }
        Action::Warn(msg) => app.toasts.info(msg),
        Action::Error(msg) => app.toasts.error(msg),
    }
}

fn confirm_reset_dialog(app: &mut DbHiveApp, ctx: &egui::Context, actions: &mut Vec<Action>) {
    let palette = app.mode.palette();
    let mut open = true;
    egui::Window::new("Reset all settings to defaults?")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label(text::muted(
                palette,
                "This restores every setting (not only shortcuts) to its default value. This action cannot be undone.",
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    actions.push(Action::CancelReset);
                }
                if ui.button("Reset").clicked() {
                    actions.push(Action::Reset);
                }
            });
        });
    if !open {
        actions.push(Action::CancelReset);
    }
}

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

/// One titled card, matching the web shell's `Card`/`CardHeader` grouping.
fn card<R>(
    ui: &mut egui::Ui,
    palette: &Palette,
    title: &str,
    description: &str,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let inner = egui::Frame::none()
        .fill(palette.card)
        .stroke(egui::Stroke::new(1.0_f32, palette.border))
        .rounding(egui::Rounding::same(8.0))
        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width() - 4.0);
            ui.label(text::strong(palette, title));
            if !description.is_empty() {
                ui.label(text::muted(palette, description));
            }
            ui.add_space(8.0);
            body(ui)
        })
        .inner;
    ui.add_space(10.0);
    inner
}

/// A checkbox with an optional second line of explanation.
fn toggle(ui: &mut egui::Ui, palette: &Palette, value: &mut bool, label: &str, hint: &str) -> bool {
    let changed = ui.checkbox(value, label).changed();
    if !hint.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(22.0);
            ui.label(text::muted(palette, hint));
        });
    }
    changed
}

fn number(ui: &mut egui::Ui, value: &mut u32, range: (u32, u32), speed: f64) -> bool {
    ui.add(
        egui::DragValue::new(value)
            .range(range.0..=range.1)
            .speed(speed),
    )
    .changed()
}

fn general_section(s: &mut Settings, ui: &mut egui::Ui, palette: &Palette) {
    let mut changed = false;

    card(
        ui,
        palette,
        "Language",
        "Choose your preferred language",
        |ui| {
            let current = core_settings::LANGUAGES
                .iter()
                .find(|(code, _)| *code == s.draft.general.language)
                .map(|(_, label)| *label)
                .unwrap_or("English");
            egui::ComboBox::from_id_salt("settings_language")
                .selected_text(current)
                .width(220.0)
                .show_ui(ui, |ui| {
                    for (code, label) in core_settings::LANGUAGES {
                        let selected = s.draft.general.language == *code;
                        if ui.selectable_label(selected, *label).clicked() && !selected {
                            s.draft.general.language = (*code).to_string();
                            changed = true;
                        }
                    }
                });
            ui.label(text::muted(
                palette,
                "Stored only: there is no translated interface behind this yet.",
            ));
        },
    );

    card(
        ui,
        palette,
        "Startup Behavior",
        "What to show when the application starts",
        |ui| {
            egui::ComboBox::from_id_salt("settings_startup")
                .selected_text(startup_label(s.draft.general.startup_behavior))
                .width(220.0)
                .show_ui(ui, |ui| {
                    for option in STARTUP_BEHAVIORS {
                        let selected = s.draft.general.startup_behavior == option;
                        if ui
                            .selectable_label(selected, startup_label(option))
                            .clicked()
                            && !selected
                        {
                            s.draft.general.startup_behavior = option;
                            changed = true;
                        }
                    }
                });
        },
    );

    card(
        ui,
        palette,
        "Auto-Save Connections",
        "Automatically save connection profiles after testing",
        |ui| {
            changed |= ui
                .checkbox(
                    &mut s.draft.general.auto_save_connections,
                    "Enable auto-save",
                )
                .changed();
        },
    );

    card(
        ui,
        palette,
        "Automatic Updates",
        "Configure automatic update checking and installation",
        |ui| {
            changed |= toggle(
                ui,
                palette,
                &mut s.draft.general.auto_check_updates,
                "Check for updates automatically",
                "Periodically check for new versions and notify you",
            );

            if s.draft.general.auto_check_updates {
                ui.add_space(6.0);
                ui.indent("update_opts", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Check interval (hours)");
                        changed |= number(
                            ui,
                            &mut s.draft.general.update_check_interval_hours,
                            core_settings::UPDATE_INTERVAL_HOURS,
                            1.0,
                        );
                    });
                    ui.label(text::muted(
                        palette,
                        "How often to check for updates (1-168 hours)",
                    ));
                    ui.add_space(6.0);

                    changed |= toggle(
                        ui,
                        palette,
                        &mut s.draft.general.auto_download_updates,
                        "Download updates automatically",
                        "Automatically download updates when available",
                    );

                    if s.draft.general.auto_download_updates {
                        ui.add_space(4.0);
                        ui.indent("install_opt", |ui| {
                            changed |= toggle(
                                ui,
                                palette,
                                &mut s.draft.general.auto_install_updates,
                                "Install updates automatically",
                                "Install downloaded updates and restart",
                            );
                        });
                    } else if s.draft.general.auto_install_updates {
                        // Installing without downloading is not a state the
                        // form can express; keep the model honest.
                        s.draft.general.auto_install_updates = false;
                        changed = true;
                    }
                });
            }
        },
    );

    s.dirty |= changed;
}

fn appearance_section(s: &mut Settings, ui: &mut egui::Ui, palette: &Palette) {
    let mut changed = false;

    card(
        ui,
        palette,
        "Theme Mode",
        "Choose between light, dark, or system theme",
        |ui| {
            egui::ComboBox::from_id_salt("settings_theme_mode")
                .selected_text(core_settings::theme_mode_label(s.draft.theme.mode))
                .width(220.0)
                .show_ui(ui, |ui| {
                    for option in [ThemeMode::Light, ThemeMode::Dark, ThemeMode::System] {
                        let selected = s.draft.theme.mode == option;
                        if ui
                            .selectable_label(selected, core_settings::theme_mode_label(option))
                            .clicked()
                            && !selected
                        {
                            // Applied on the next `ensure_theme`, i.e. this
                            // same frame's end: the mode changes live.
                            s.draft.theme.mode = option;
                            changed = true;
                        }
                    }
                });
        },
    );

    card(
        ui,
        palette,
        "Accent Color",
        "Primary accent color for the UI, applied live",
        |ui| {
            let (r, g, b) = s.accent_rgb();
            let mut rgb = [r, g, b];
            ui.horizontal(|ui| {
                if ui.color_edit_button_srgb(&mut rgb).changed() {
                    let hex = core_settings::hex_string((rgb[0], rgb[1], rgb[2]));
                    s.accent_input = hex.clone();
                    s.draft.theme.accent_color = hex;
                    changed = true;
                }
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut s.accent_input)
                            .desired_width(120.0)
                            .hint_text("#f59e0b"),
                    )
                    .changed()
                {
                    // Only a colour that parses reaches the model, so a
                    // half-typed "#f5" cannot repaint the window.
                    if let Some(parsed) = core_settings::parse_hex_color(&s.accent_input) {
                        s.draft.theme.accent_color = core_settings::hex_string(parsed);
                        changed = true;
                    }
                }
                if ui
                    .button("Reset")
                    .on_hover_text("Back to DB Hive honey")
                    .clicked()
                {
                    let hex = core_settings::hex_string(core_settings::default_accent_rgb());
                    s.accent_input = hex.clone();
                    s.draft.theme.accent_color = hex;
                    changed = true;
                }
            });
            if core_settings::parse_hex_color(&s.accent_input).is_none() {
                ui.colored_label(palette.destructive, "Not a hex colour, e.g. #f59e0b");
            }
        },
    );

    card(
        ui,
        palette,
        "Editor Font",
        "Customize SQL editor font settings",
        |ui| {
            ui.horizontal(|ui| {
                ui.label("Font size (px)");
                changed |= number(
                    ui,
                    &mut s.draft.theme.editor_font_size,
                    core_settings::EDITOR_FONT_SIZE,
                    0.2,
                );
            });
            ui.add_space(6.0);

            ui.label("Font family");
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut s.draft.theme.editor_font_family)
                        .desired_width(320.0)
                        .hint_text("Monaco, 'Courier New', monospace"),
                )
                .changed();
            ui.add_space(8.0);

            changed |= ui
                .checkbox(&mut s.draft.theme.editor_line_numbers, "Show line numbers")
                .changed();
            changed |= ui
                .checkbox(&mut s.draft.theme.editor_minimap, "Show minimap")
                .changed();
            changed |= ui
                .checkbox(&mut s.draft.theme.editor_word_wrap, "Word wrap")
                .changed();

            ui.add_space(6.0);
            ui.label(text::muted(
                palette,
                "Font size applies live to the SQL editor. Font family, line numbers, minimap and word wrap are stored and take effect once the native editor reads them.",
            ));
        },
    );

    s.dirty |= changed;
}

fn query_section(s: &mut Settings, ui: &mut egui::Ui, palette: &Palette) {
    let mut changed = false;

    card(
        ui,
        palette,
        "Query Timeout",
        "Maximum time to wait for query execution (0 = no timeout)",
        |ui| {
            ui.horizontal(|ui| {
                changed |= number(
                    ui,
                    &mut s.draft.query.timeout_seconds,
                    core_settings::QUERY_TIMEOUT_SECONDS,
                    1.0,
                );
                ui.label(text::muted(palette, "seconds"));
            });
        },
    );

    card(
        ui,
        palette,
        "Maximum Rows",
        "Maximum number of rows to fetch per query",
        |ui| {
            changed |= number(
                ui,
                &mut s.draft.query.max_rows,
                core_settings::MAX_ROWS,
                25.0,
            );
        },
    );

    card(
        ui,
        palette,
        "Safety & History",
        "Configure safety and history options",
        |ui| {
            changed |= toggle(
                ui,
                palette,
                &mut s.draft.query.confirm_destructive,
                "Confirm destructive queries",
                "Show confirmation before DELETE or DROP",
            );
            ui.add_space(4.0);
            changed |= toggle(
                ui,
                palette,
                &mut s.draft.query.auto_save_history,
                "Auto-save to history",
                "Automatically save queries to history",
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label("Max history entries");
                changed |= number(
                    ui,
                    &mut s.draft.query.max_history_entries,
                    core_settings::MAX_HISTORY_ENTRIES,
                    10.0,
                );
            });
        },
    );

    s.dirty |= changed;
}

// ---------------------------------------------------------------------------
// Keyboard shortcuts
// ---------------------------------------------------------------------------

/// Format a keystroke the way `settings.json` stores it, e.g. "Ctrl+Shift+F".
///
/// The egui adapter for [`combo_string`]. `capture` goes through
/// `record_keystroke` instead, so outside the tests nothing calls this yet.
#[allow(dead_code)]
pub fn combo_from(modifiers: egui::Modifiers, key: egui::Key, is_mac: bool) -> String {
    combo_string(key_modifiers(modifiers), &key_label(key), is_mac)
}

fn key_modifiers(m: egui::Modifiers) -> KeyModifiers {
    KeyModifiers {
        alt: m.alt,
        ctrl: m.ctrl,
        shift: m.shift,
        mac_cmd: m.mac_cmd,
        command: m.command,
    }
}

/// egui's `Key::name()` spells punctuation out ("Comma"); the stored format
/// uses the symbol, so "Ctrl+," round-trips.
pub fn key_label(key: egui::Key) -> String {
    key_label_for_name(key.name())
}

/// Interpret the frame's key events while recording: the first pressed key
/// decides, via [`record_keystroke`].
pub fn capture(events: &[egui::Event], is_mac: bool) -> Recorded {
    for event in events {
        let egui::Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } = event
        else {
            continue;
        };
        return record_keystroke(key_modifiers(*modifiers), &key_label(*key), is_mac);
    }
    Recorded::Pending
}

fn shortcuts_section(
    s: &mut Settings,
    ui: &mut egui::Ui,
    palette: &Palette,
    actions: &mut Vec<Action>,
) {
    // While recording, every keystroke belongs to the binding: swallow the
    // frame's key events so they cannot also trigger a shortcut elsewhere.
    if let Some(field) = s.recording {
        let outcome = ui.input_mut(|i| {
            let outcome = capture(&i.events, cfg!(target_os = "macos"));
            i.events.retain(|e| !matches!(e, egui::Event::Key { .. }));
            outcome
        });
        match outcome {
            Recorded::Pending => {}
            Recorded::Cancel => s.recording = None,
            Recorded::Clear => {
                field.set(&mut s.draft.shortcuts, String::new());
                s.recording = None;
                actions.push(Action::SaveShortcuts);
            }
            Recorded::Bind(combo) => {
                let dupes = duplicates(&s.draft.shortcuts, field, &combo);
                field.set(&mut s.draft.shortcuts, combo.clone());
                s.recording = None;
                if !dupes.is_empty() {
                    actions.push(Action::Warn(duplicate_warning(&combo, &dupes)));
                }
                actions.push(Action::SaveShortcuts);
            }
            Recorded::Invalid(message) => actions.push(Action::Error(message)),
        }
    }

    ui.horizontal(|ui| {
        ui.label(text::muted(
            palette,
            "Rebindings save immediately. Escape cancels, Backspace unbinds.",
        ));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Reset all to defaults").clicked() {
                actions.push(Action::AskReset);
            }
        });
    });
    ui.add_space(8.0);

    card(
        ui,
        palette,
        "Editor Shortcuts",
        "Bindings active inside the SQL editor and query view",
        |ui| {
            for field in EDITOR_SHORTCUTS {
                shortcut_row(s, ui, palette, field, actions);
            }
        },
    );

    card(
        ui,
        palette,
        "Navigation Shortcuts",
        "Global navigation and window-level bindings",
        |ui| {
            for field in NAVIGATION_SHORTCUTS {
                shortcut_row(s, ui, palette, field, actions);
            }
        },
    );
}

fn shortcut_row(
    s: &mut Settings,
    ui: &mut egui::Ui,
    palette: &Palette,
    field: ShortcutField,
    actions: &mut Vec<Action>,
) {
    let recording = s.recording == Some(field);
    let binding = field.get(&s.draft.shortcuts).to_string();
    let display = shortcut_display(recording, &binding);

    ui.horizontal(|ui| {
        ui.label(field.label());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !binding.is_empty()
                && !recording
                && ui
                    .small_button("clear")
                    .on_hover_text("Clear binding")
                    .clicked()
            {
                field.set(&mut s.draft.shortcuts, String::new());
                actions.push(Action::SaveShortcuts);
            }
            let button = egui::Button::new(egui::RichText::new(display).monospace())
                .min_size(egui::vec2(140.0, 22.0))
                .fill(if recording {
                    palette.accent
                } else {
                    palette.muted
                });
            if ui
                .add(button)
                .on_hover_text(format!("Rebind {}", field.label()))
                .clicked()
            {
                s.recording = if recording { None } else { Some(field) };
            }
        });
    });
    ui.add_space(2.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(ctrl: bool, cmd: bool, alt: bool, shift: bool) -> egui::Modifiers {
        egui::Modifiers {
            alt,
            ctrl,
            shift,
            mac_cmd: cmd,
            command: cmd || ctrl,
        }
    }

    fn key_event(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn combo_matches_the_stored_format() {
        assert_eq!(
            combo_from(mods(true, false, false, false), egui::Key::Enter, false),
            "Ctrl+Enter"
        );
        assert_eq!(
            combo_from(mods(true, false, false, true), egui::Key::F, false),
            "Ctrl+Shift+F"
        );
        assert_eq!(
            combo_from(mods(true, false, false, false), egui::Key::Comma, false),
            "Ctrl+,"
        );
        assert_eq!(
            combo_from(mods(false, false, false, false), egui::Key::F5, false),
            "F5"
        );
    }

    #[test]
    fn command_is_spelled_per_platform() {
        // egui sets `command` alongside ctrl off macOS; the binding must not
        // come out as "Ctrl+Cmd+K".
        let m = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        assert_eq!(combo_from(m, egui::Key::K, false), "Ctrl+K");

        let m = egui::Modifiers {
            mac_cmd: true,
            command: true,
            ..Default::default()
        };
        assert_eq!(combo_from(m, egui::Key::K, true), "Cmd+K");

        // Real Control on a Mac stays Control.
        let m = egui::Modifiers {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(combo_from(m, egui::Key::K, true), "Ctrl+K");
    }

    #[test]
    fn alt_is_option_on_mac() {
        let m = egui::Modifiers {
            alt: true,
            ..Default::default()
        };
        assert_eq!(combo_from(m, egui::Key::T, true), "Option+T");
        assert_eq!(combo_from(m, egui::Key::T, false), "Alt+T");
    }

    #[test]
    fn punctuation_keys_use_their_symbol() {
        assert_eq!(key_label(egui::Key::Comma), ",");
        assert_eq!(key_label(egui::Key::Questionmark), "?");
        assert_eq!(key_label(egui::Key::Slash), "/");
        assert_eq!(key_label(egui::Key::Enter), "Enter");
        assert_eq!(key_label(egui::Key::ArrowUp), "Up");
    }

    #[test]
    fn capture_reports_cancel_clear_and_bind() {
        assert_eq!(capture(&[], false), Recorded::Pending);
        assert_eq!(
            capture(&[key_event(egui::Key::Escape, Default::default())], false),
            Recorded::Cancel
        );
        assert_eq!(
            capture(
                &[key_event(egui::Key::Backspace, Default::default())],
                false
            ),
            Recorded::Clear
        );
        assert_eq!(
            capture(
                &[key_event(egui::Key::Enter, mods(true, false, false, false))],
                false
            ),
            Recorded::Bind("Ctrl+Enter".into())
        );
        assert!(matches!(
            capture(&[key_event(egui::Key::K, Default::default())], false),
            Recorded::Invalid(_)
        ));
    }

    #[test]
    fn capture_ignores_key_releases_and_non_key_events() {
        let release = egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Default::default(),
        };
        let events = vec![
            egui::Event::PointerMoved(egui::pos2(0.0, 0.0)),
            release,
            key_event(egui::Key::Enter, mods(true, false, false, false)),
        ];
        assert_eq!(capture(&events, false), Recorded::Bind("Ctrl+Enter".into()));
    }

    #[test]
    fn ctrl_backspace_is_a_binding_not_an_unbind() {
        assert_eq!(
            capture(
                &[key_event(
                    egui::Key::Backspace,
                    mods(true, false, false, false)
                )],
                false
            ),
            Recorded::Bind("Ctrl+Backspace".into())
        );
    }

    #[test]
    fn accent_falls_back_to_the_brand_colour_when_unparseable() {
        let mut s = Settings::default();
        assert_eq!(s.accent_rgb(), core_settings::default_accent_rgb());
        s.draft.theme.accent_color = "not a colour".into();
        assert_eq!(s.accent_rgb(), core_settings::default_accent_rgb());
        s.draft.theme.accent_color = "#102030".into();
        assert_eq!(s.accent_rgb(), (0x10, 0x20, 0x30));
    }

    #[test]
    fn text_on_a_light_accent_is_dark_and_vice_versa() {
        let dark = readable_on(egui::Color32::from_rgb(248, 164, 0));
        let light = readable_on(egui::Color32::from_rgb(20, 30, 90));
        assert!(dark.r() < 50);
        assert!(light.r() > 200);
    }

    #[test]
    fn adopt_clears_the_dirty_flag_and_resyncs_the_hex_field() {
        let mut s = Settings::default();
        s.dirty = true;
        s.accent_input = "#00".into();

        let mut loaded = AppSettings::default();
        loaded.theme.accent_color = "#123456".into();
        s.adopt(loaded);

        assert!(!s.dirty);
        assert_eq!(s.accent_input, "#123456");
        assert_eq!(s.draft.theme.accent_color, "#123456");
    }

    #[test]
    fn resolve_mode_adapters_round_trip() {
        for m in [Mode::Light, Mode::Dark] {
            assert_eq!(mode_from(resolved_from(m)), m);
        }
    }
}
