//! The Settings screen's editing state, shell-agnostic.
//!
//! [`crate::settings`] holds the pure pieces (sections, shortcut table,
//! keystroke formatting). This module is the stateful part both shells need
//! and the egui screen kept inline: a draft copy of [`AppSettings`] next to
//! the last saved copy, numeric fields that hold half-typed text without
//! corrupting the draft, the accent colour buffer, shortcut recording, and
//! the derived facts a shell applies live (dark or light, accent, editor
//! font size).
//!
//! Save semantics follow the web shell and the egui screen:
//!
//! * General, Appearance and Query edits sit in the draft until "Save".
//! * A shortcut binding is persisted the moment it is recorded, but only the
//!   shortcuts: [`SettingsEditor::saved_with_draft_shortcuts`] builds that
//!   write so an unsaved font size is not saved by accident.
//! * "Reset to defaults" replaces everything and is saved at once.
//!
//! Nothing here touches disk; the shell calls `dbhive_core::settings`.

use std::collections::HashMap;

use dbhive_core::models::settings::{
    AppSettings, QuerySettings, ShortcutsSettings, StartupBehavior, ThemeMode,
};
use dbhive_core::settings as core_settings;

use crate::settings::{
    accent_rgb, duplicate_warning, duplicates, readable_on_rgb, record_keystroke,
    resolve_theme_mode, KeyModifiers, Recorded, ResolvedMode, Section, ShortcutField,
    EDITOR_SHORTCUTS, NAVIGATION_SHORTCUTS, STARTUP_BEHAVIORS,
};

// ---------------------------------------------------------------------------
// Numeric fields
// ---------------------------------------------------------------------------

/// A setting edited as a number in a text box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NumberField {
    UpdateInterval,
    EditorFontSize,
    QueryTimeout,
    MaxRows,
    MaxHistory,
}

impl NumberField {
    pub fn range(self) -> (u32, u32) {
        match self {
            NumberField::UpdateInterval => core_settings::UPDATE_INTERVAL_HOURS,
            NumberField::EditorFontSize => core_settings::EDITOR_FONT_SIZE,
            NumberField::QueryTimeout => core_settings::QUERY_TIMEOUT_SECONDS,
            NumberField::MaxRows => core_settings::MAX_ROWS,
            NumberField::MaxHistory => core_settings::MAX_HISTORY_ENTRIES,
        }
    }

    pub fn get(self, s: &AppSettings) -> u32 {
        match self {
            NumberField::UpdateInterval => s.general.update_check_interval_hours,
            NumberField::EditorFontSize => s.theme.editor_font_size,
            NumberField::QueryTimeout => s.query.timeout_seconds,
            NumberField::MaxRows => s.query.max_rows,
            NumberField::MaxHistory => s.query.max_history_entries,
        }
    }

    pub fn set(self, s: &mut AppSettings, value: u32) {
        match self {
            NumberField::UpdateInterval => s.general.update_check_interval_hours = value,
            NumberField::EditorFontSize => s.theme.editor_font_size = value,
            NumberField::QueryTimeout => s.query.timeout_seconds = value,
            NumberField::MaxRows => s.query.max_rows = value,
            NumberField::MaxHistory => s.query.max_history_entries = value,
        }
    }

    const ALL: [NumberField; 5] = [
        NumberField::UpdateInterval,
        NumberField::EditorFontSize,
        NumberField::QueryTimeout,
        NumberField::MaxRows,
        NumberField::MaxHistory,
    ];
}

// ---------------------------------------------------------------------------
// Option lists the pickers show
// ---------------------------------------------------------------------------

pub const THEME_MODES: [ThemeMode; 3] = [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark];

pub fn language_labels() -> Vec<&'static str> {
    core_settings::LANGUAGES.iter().map(|(_, l)| *l).collect()
}

pub fn language_label(code: &str) -> &'static str {
    core_settings::LANGUAGES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, l)| *l)
        .unwrap_or("English")
}

pub fn language_code_at(index: usize) -> Option<&'static str> {
    core_settings::LANGUAGES.get(index).map(|(c, _)| *c)
}

pub fn startup_labels() -> Vec<&'static str> {
    STARTUP_BEHAVIORS
        .iter()
        .map(|b| crate::settings::startup_label(*b))
        .collect()
}

pub fn theme_mode_labels() -> Vec<&'static str> {
    THEME_MODES
        .iter()
        .map(|m| core_settings::theme_mode_label(*m))
        .collect()
}

/// Accent swatches. The first is the brand honey, which doubles as "reset".
pub const ACCENT_PRESETS: [(&str, (u8, u8, u8)); 8] = [
    ("Honey", (0xf5, 0x9e, 0x0b)),
    ("Blue", (0x3b, 0x82, 0xf6)),
    ("Violet", (0x8b, 0x5c, 0xf6)),
    ("Pink", (0xec, 0x48, 0x99)),
    ("Red", (0xef, 0x44, 0x44)),
    ("Green", (0x22, 0xc5, 0x5e)),
    ("Teal", (0x14, 0xb8, 0xa6)),
    ("Slate", (0x64, 0x74, 0x8b)),
];

// ---------------------------------------------------------------------------
// Live appearance
// ---------------------------------------------------------------------------

/// What a shell paints from the current settings, before they are saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Appearance {
    pub mode: ResolvedMode,
    /// `None` when the accent is the brand honey: the theme's own per-mode
    /// primary is better tuned than one colour reused for both modes.
    pub accent: Option<(u8, u8, u8)>,
    /// Readable text on `accent` (only meaningful with an accent).
    pub accent_foreground: (u8, u8, u8),
    pub editor_font_size: u32,
    pub editor_line_numbers: bool,
}

/// Derive the live appearance. `system_dark` is the OS preference if known.
pub fn appearance_of(settings: &AppSettings, system_dark: Option<bool>) -> Appearance {
    let rgb = accent_rgb(&settings.theme.accent_color);
    let is_brand = rgb == core_settings::default_accent_rgb();
    Appearance {
        mode: resolve_theme_mode(settings.theme.mode, system_dark),
        accent: (!is_brand).then_some(rgb),
        accent_foreground: readable_on_rgb(rgb),
        editor_font_size: settings.theme.editor_font_size.clamp(
            core_settings::EDITOR_FONT_SIZE.0,
            core_settings::EDITOR_FONT_SIZE.1,
        ),
        editor_line_numbers: settings.theme.editor_line_numbers,
    }
}

// ---------------------------------------------------------------------------
// Settings that change what the app does
// ---------------------------------------------------------------------------

/// Statements the destructive-query guard flags, or none when the user turned
/// "Confirm destructive queries" off.
pub fn destructive_warnings(query: &QuerySettings, sql: &str) -> Vec<crate::sql_guard::Warning> {
    if !query.confirm_destructive {
        return Vec::new();
    }
    crate::sql_guard::warnings(sql)
}

/// How long a query may run, or `None` for no limit (timeout 0).
pub fn query_timeout(query: &QuerySettings) -> Option<std::time::Duration> {
    (query.timeout_seconds > 0)
        .then(|| std::time::Duration::from_secs(u64::from(query.timeout_seconds)))
}

/// The error a query that hit the timeout reports, with the way out.
pub fn timeout_message(query: &QuerySettings) -> String {
    format!(
        "Query timed out after {} seconds. Change the limit in Settings > Query Execution.",
        query.timeout_seconds
    )
}

/// Keep only the newest `max_history_entries` (oldest first in `items`).
pub fn trim_history<T>(items: &mut Vec<T>, query: &QuerySettings) {
    let max = query.max_history_entries as usize;
    if items.len() > max {
        let excess = items.len() - max;
        items.drain(..excess);
    }
}

// ---------------------------------------------------------------------------
// Shortcut rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutRow {
    pub field: ShortcutField,
    /// Set on the first row of a group: "Editor" or "Navigation".
    pub group: Option<&'static str>,
    pub label: &'static str,
    /// Binding text, "Press shortcut\u{2026}" while recording, or "(unbound)".
    pub display: String,
    pub recording: bool,
    /// Another action already uses this binding.
    pub conflict: bool,
}

/// What recording a keystroke did, for the shell to react to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordOutcome {
    /// No recording in progress.
    Idle,
    /// Escape.
    Cancelled,
    /// Backspace: the binding is now empty. Persist.
    Cleared,
    /// A binding was stored. Persist; `warning` names any collision.
    Bound {
        combo: String,
        warning: Option<String>,
    },
    /// Not usable as a binding; still recording.
    Invalid(String),
    /// Nothing usable yet (a bare modifier); still recording.
    Waiting,
}

// ---------------------------------------------------------------------------
// The editor
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SettingsEditor {
    section: Section,
    draft: AppSettings,
    saved: AppSettings,
    /// What the hex box holds; the draft only ever sees parseable colours.
    accent_input: String,
    /// Text of numeric boxes whose text is not the draft's value (empty,
    /// half-typed, or out of range). Absent means "show the draft's value".
    numbers: HashMap<NumberField, String>,
    recording: Option<ShortcutField>,
}

impl SettingsEditor {
    pub fn new(settings: AppSettings) -> Self {
        Self {
            section: Section::General,
            accent_input: settings.theme.accent_color.clone(),
            saved: settings.clone(),
            draft: settings,
            numbers: HashMap::new(),
            recording: None,
        }
    }

    /// Replace both copies with `settings` (a load, a save result, a reset)
    /// and drop every half-typed value. The open section is kept.
    pub fn adopt(&mut self, settings: AppSettings) {
        let section = self.section;
        *self = Self::new(settings);
        self.section = section;
    }

    pub fn draft(&self) -> &AppSettings {
        &self.draft
    }

    pub fn saved(&self) -> &AppSettings {
        &self.saved
    }

    pub fn section(&self) -> Section {
        self.section
    }

    pub fn set_section(&mut self, section: Section) {
        self.section = section;
        self.recording = None;
    }

    pub fn section_index(&self) -> usize {
        Section::ALL
            .iter()
            .position(|s| *s == self.section)
            .unwrap_or(0)
    }

    pub fn select_section_index(&mut self, index: usize) {
        if let Some(s) = Section::ALL.get(index) {
            self.set_section(*s);
        }
    }

    /// Edit the draft directly. For plain toggles and pickers; anything with
    /// validation has its own method below.
    pub fn edit(&mut self, f: impl FnOnce(&mut AppSettings)) {
        f(&mut self.draft);
    }

    pub fn is_dirty(&self) -> bool {
        self.draft != self.saved || self.has_errors()
    }

    /// Whether Save would write something valid.
    pub fn can_save(&self) -> bool {
        self.draft != self.saved && !self.has_errors()
    }

    pub fn has_errors(&self) -> bool {
        !self.accent_is_valid()
            || NumberField::ALL
                .iter()
                .any(|f| self.number_error(*f).is_some())
    }

    /// Throw away unsaved edits.
    pub fn discard(&mut self) {
        let saved = self.saved.clone();
        self.adopt(saved);
    }

    // -- General ---------------------------------------------------------

    pub fn set_language_index(&mut self, index: usize) {
        if let Some(code) = language_code_at(index) {
            self.draft.general.language = code.to_string();
        }
    }

    pub fn set_startup_index(&mut self, index: usize) {
        if let Some(b) = STARTUP_BEHAVIORS.get(index) {
            self.draft.general.startup_behavior = *b;
        }
    }

    pub fn startup_index(&self) -> usize {
        STARTUP_BEHAVIORS
            .iter()
            .position(|b| *b == self.draft.general.startup_behavior)
            .unwrap_or(0)
    }

    /// Installing implies downloading, so turning download off turns install
    /// off with it.
    pub fn set_auto_download(&mut self, on: bool) {
        self.draft.general.auto_download_updates = on;
        if !on {
            self.draft.general.auto_install_updates = false;
        }
    }

    pub fn set_auto_check(&mut self, on: bool) {
        self.draft.general.auto_check_updates = on;
    }

    pub fn startup_behavior(&self) -> StartupBehavior {
        self.draft.general.startup_behavior
    }

    // -- Numbers ---------------------------------------------------------

    /// Type into a numeric box. A valid in-range number goes to the draft;
    /// anything else is kept as text and reported by [`number_error`].
    pub fn set_number_text(&mut self, field: NumberField, text: &str) {
        let trimmed = text.trim();
        match trimmed.parse::<u32>() {
            Ok(v) if in_range(v, field.range()) => {
                field.set(&mut self.draft, v);
                self.numbers.remove(&field);
            }
            _ => {
                self.numbers.insert(field, text.to_string());
            }
        }
    }

    /// What the box should show.
    pub fn number_text(&self, field: NumberField) -> String {
        match self.numbers.get(&field) {
            Some(t) => t.clone(),
            None => field.get(&self.draft).to_string(),
        }
    }

    pub fn number_error(&self, field: NumberField) -> Option<String> {
        let text = self.numbers.get(&field)?;
        let (lo, hi) = field.range();
        Some(match text.trim().parse::<u32>() {
            Ok(_) => format!("Must be between {} and {}", lo, hi),
            Err(_) => format!("Enter a whole number between {} and {}", lo, hi),
        })
    }

    // -- Accent ----------------------------------------------------------

    pub fn accent_input(&self) -> &str {
        &self.accent_input
    }

    pub fn accent_is_valid(&self) -> bool {
        core_settings::parse_hex_color(&self.accent_input).is_some()
    }

    /// Typing in the hex box. Only a colour that parses reaches the draft, so
    /// a half-typed "#f5" cannot repaint the window.
    pub fn set_accent_text(&mut self, text: &str) {
        self.accent_input = text.to_string();
        if let Some(rgb) = core_settings::parse_hex_color(text) {
            self.draft.theme.accent_color = core_settings::hex_string(rgb);
        }
    }

    pub fn set_accent_rgb(&mut self, rgb: (u8, u8, u8)) {
        let hex = core_settings::hex_string(rgb);
        self.accent_input = hex.clone();
        self.draft.theme.accent_color = hex;
    }

    pub fn reset_accent(&mut self) {
        self.set_accent_rgb(core_settings::default_accent_rgb());
    }

    /// Index into [`ACCENT_PRESETS`] of the current accent, or -1.
    pub fn accent_preset_index(&self) -> i32 {
        let current = accent_rgb(&self.draft.theme.accent_color);
        ACCENT_PRESETS
            .iter()
            .position(|(_, rgb)| *rgb == current)
            .map(|i| i as i32)
            .unwrap_or(-1)
    }

    /// A swatch was picked. Picking the selected one again (index -1) goes
    /// back to the brand honey.
    pub fn pick_accent_preset(&mut self, index: i32) {
        if index < 0 {
            self.reset_accent();
        } else if let Some((_, rgb)) = ACCENT_PRESETS.get(index as usize) {
            self.set_accent_rgb(*rgb);
        }
    }

    pub fn set_theme_mode_index(&mut self, index: usize) {
        if let Some(m) = THEME_MODES.get(index) {
            self.draft.theme.mode = *m;
        }
    }

    pub fn theme_mode_index(&self) -> usize {
        THEME_MODES
            .iter()
            .position(|m| *m == self.draft.theme.mode)
            .unwrap_or(0)
    }

    /// Change the mode in both copies, for the title-bar light/dark toggle:
    /// it is a one-click preference, not a pending edit, so it must not leave
    /// the dialog "dirty". Returns the settings to write.
    pub fn set_theme_mode_saved(&mut self, mode: ThemeMode) -> AppSettings {
        self.draft.theme.mode = mode;
        self.saved.theme.mode = mode;
        self.saved.clone()
    }

    /// Live look of the draft.
    pub fn appearance(&self, system_dark: Option<bool>) -> Appearance {
        appearance_of(&self.draft, system_dark)
    }

    // -- Shortcuts -------------------------------------------------------

    pub fn recording(&self) -> Option<ShortcutField> {
        self.recording
    }

    pub fn start_recording(&mut self, field: ShortcutField) {
        self.recording = Some(field);
    }

    pub fn start_recording_index(&mut self, index: usize) {
        if let Some(f) = shortcut_fields().get(index) {
            self.start_recording(*f);
        }
    }

    pub fn stop_recording(&mut self) {
        self.recording = None;
    }

    /// Unbind one shortcut.
    pub fn clear_shortcut(&mut self, field: ShortcutField) {
        field.set(&mut self.draft.shortcuts, String::new());
        if self.recording == Some(field) {
            self.recording = None;
        }
    }

    pub fn clear_shortcut_index(&mut self, index: usize) {
        if let Some(f) = shortcut_fields().get(index) {
            self.clear_shortcut(*f);
        }
    }

    /// Feed one keystroke to the recorder.
    ///
    /// `key` is the stored label of the key (see
    /// [`crate::settings::key_label_for_name`]); an empty label means a bare
    /// modifier press.
    pub fn record(&mut self, modifiers: KeyModifiers, key: &str, is_mac: bool) -> RecordOutcome {
        let Some(field) = self.recording else {
            return RecordOutcome::Idle;
        };
        if key.is_empty() {
            return RecordOutcome::Waiting;
        }
        match record_keystroke(modifiers, key, is_mac) {
            Recorded::Pending => RecordOutcome::Waiting,
            Recorded::Cancel => {
                self.recording = None;
                RecordOutcome::Cancelled
            }
            Recorded::Clear => {
                field.set(&mut self.draft.shortcuts, String::new());
                self.recording = None;
                RecordOutcome::Cleared
            }
            Recorded::Invalid(msg) => RecordOutcome::Invalid(msg),
            Recorded::Bind(combo) => {
                let dupes = duplicates(&self.draft.shortcuts, field, &combo);
                let warning = (!dupes.is_empty()).then(|| duplicate_warning(&combo, &dupes));
                field.set(&mut self.draft.shortcuts, combo.clone());
                self.recording = None;
                RecordOutcome::Bound { combo, warning }
            }
        }
    }

    /// The settings to write when a shortcut changes: the saved copy with the
    /// draft's shortcuts, so no other unsaved edit is saved by accident.
    pub fn saved_with_draft_shortcuts(&self) -> AppSettings {
        let mut s = self.saved.clone();
        s.shortcuts = self.draft.shortcuts.clone();
        s
    }

    /// Mark the shortcuts as saved without touching other unsaved edits.
    pub fn mark_shortcuts_saved(&mut self, written: &ShortcutsSettings) {
        self.saved.shortcuts = written.clone();
    }

    pub fn shortcut_rows(&self) -> Vec<ShortcutRow> {
        let mut rows = Vec::new();
        for (group, fields) in [
            ("Editor", &EDITOR_SHORTCUTS[..]),
            ("Navigation", &NAVIGATION_SHORTCUTS[..]),
        ] {
            for (i, field) in fields.iter().enumerate() {
                let binding = field.get(&self.draft.shortcuts);
                let recording = self.recording == Some(*field);
                rows.push(ShortcutRow {
                    field: *field,
                    group: (i == 0).then_some(group),
                    label: field.label(),
                    display: crate::settings::shortcut_display(recording, binding),
                    recording,
                    conflict: !recording
                        && !duplicates(&self.draft.shortcuts, *field, binding).is_empty(),
                });
            }
        }
        rows
    }

    /// Every rebindable field, in the order [`shortcut_rows`] lists them.
    pub fn shortcut_count() -> usize {
        shortcut_fields().len()
    }
}

/// All shortcut fields in display order.
pub fn shortcut_fields() -> Vec<ShortcutField> {
    EDITOR_SHORTCUTS
        .iter()
        .chain(NAVIGATION_SHORTCUTS.iter())
        .copied()
        .collect()
}

fn in_range(v: u32, (lo, hi): (u32, u32)) -> bool {
    v >= lo && v <= hi
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl() -> KeyModifiers {
        KeyModifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        }
    }

    fn editor() -> SettingsEditor {
        SettingsEditor::new(AppSettings::default())
    }

    #[test]
    fn a_fresh_editor_is_clean_and_saving_does_nothing() {
        let e = editor();
        assert!(!e.is_dirty());
        assert!(!e.can_save());
        assert_eq!(e.section(), Section::General);
    }

    #[test]
    fn editing_makes_it_dirty_and_discard_puts_it_back() {
        let mut e = editor();
        e.edit(|s| s.query.confirm_destructive = false);
        assert!(e.is_dirty() && e.can_save());
        e.discard();
        assert!(!e.is_dirty());
        assert!(e.draft().query.confirm_destructive);
    }

    #[test]
    fn discard_keeps_the_open_section() {
        let mut e = editor();
        e.set_section(Section::Query);
        e.edit(|s| s.query.max_rows = 500);
        e.discard();
        assert_eq!(e.section(), Section::Query);
    }

    #[test]
    fn a_valid_number_reaches_the_draft() {
        let mut e = editor();
        e.set_number_text(NumberField::QueryTimeout, "45");
        assert_eq!(e.draft().query.timeout_seconds, 45);
        assert_eq!(e.number_text(NumberField::QueryTimeout), "45");
        assert!(e.number_error(NumberField::QueryTimeout).is_none());
    }

    #[test]
    fn an_out_of_range_number_is_kept_as_text_and_blocks_saving() {
        let mut e = editor();
        e.set_number_text(NumberField::MaxRows, "50");
        // The draft keeps the last good value, the box keeps what was typed.
        assert_eq!(e.draft().query.max_rows, 1000);
        assert_eq!(e.number_text(NumberField::MaxRows), "50");
        assert_eq!(
            e.number_error(NumberField::MaxRows).as_deref(),
            Some("Must be between 100 and 100000")
        );
        e.edit(|s| s.query.auto_save_history = false);
        assert!(e.is_dirty());
        assert!(
            !e.can_save(),
            "saving with an invalid field would silently clamp it"
        );
    }

    #[test]
    fn non_numeric_text_is_an_error_not_a_zero() {
        let mut e = editor();
        e.set_number_text(NumberField::EditorFontSize, "abc");
        assert_eq!(e.draft().theme.editor_font_size, 14);
        assert!(e
            .number_error(NumberField::EditorFontSize)
            .unwrap()
            .contains("whole number"));
        e.set_number_text(NumberField::EditorFontSize, "");
        assert!(e.number_error(NumberField::EditorFontSize).is_some());
    }

    #[test]
    fn fixing_a_number_clears_its_error() {
        let mut e = editor();
        e.set_number_text(NumberField::MaxHistory, "5");
        assert!(e.has_errors());
        e.set_number_text(NumberField::MaxHistory, "200");
        assert!(!e.has_errors());
        assert_eq!(e.draft().query.max_history_entries, 200);
    }

    #[test]
    fn zero_is_allowed_for_the_timeout_only() {
        let mut e = editor();
        e.set_number_text(NumberField::QueryTimeout, "0");
        assert_eq!(e.draft().query.timeout_seconds, 0);
        e.set_number_text(NumberField::MaxRows, "0");
        assert!(e.number_error(NumberField::MaxRows).is_some());
    }

    #[test]
    fn half_typed_hex_never_reaches_the_draft() {
        let mut e = editor();
        e.set_accent_text("#f5");
        assert_eq!(e.draft().theme.accent_color, "#f59e0b");
        assert!(!e.accent_is_valid());
        assert!(e.has_errors());
        e.set_accent_text("#3b82f6");
        assert!(e.accent_is_valid());
        assert_eq!(e.draft().theme.accent_color, "#3b82f6");
    }

    #[test]
    fn short_and_upper_case_hex_is_normalised() {
        let mut e = editor();
        e.set_accent_text("#ABC");
        assert_eq!(e.draft().theme.accent_color, "#aabbcc");
    }

    #[test]
    fn presets_select_and_the_selected_one_resets_to_honey() {
        let mut e = editor();
        assert_eq!(e.accent_preset_index(), 0);
        e.pick_accent_preset(2);
        assert_eq!(e.accent_preset_index(), 2);
        assert_eq!(e.accent_input(), "#8b5cf6");
        e.pick_accent_preset(-1);
        assert_eq!(e.accent_preset_index(), 0);
        assert!(!e.is_dirty());
    }

    #[test]
    fn a_custom_accent_matches_no_preset() {
        let mut e = editor();
        e.set_accent_text("#123456");
        assert_eq!(e.accent_preset_index(), -1);
    }

    #[test]
    fn download_off_turns_install_off() {
        let mut e = editor();
        e.set_auto_download(true);
        e.edit(|s| s.general.auto_install_updates = true);
        e.set_auto_download(false);
        assert!(!e.draft().general.auto_install_updates);
    }

    #[test]
    fn picker_indexes_map_to_values_and_ignore_out_of_range() {
        let mut e = editor();
        e.set_language_index(2);
        assert_eq!(e.draft().general.language, "fr");
        e.set_language_index(99);
        assert_eq!(e.draft().general.language, "fr");
        e.set_startup_index(1);
        assert_eq!(
            e.draft().general.startup_behavior,
            StartupBehavior::OpenLastConnection
        );
        assert_eq!(e.startup_index(), 1);
        e.set_theme_mode_index(2);
        assert_eq!(e.draft().theme.mode, ThemeMode::Dark);
        assert_eq!(e.theme_mode_index(), 2);
        assert_eq!(theme_mode_labels(), ["System", "Light", "Dark"]);
        assert_eq!(startup_labels().len(), 4);
        assert_eq!(language_labels().len(), core_settings::LANGUAGES.len());
    }

    #[test]
    fn the_brand_accent_leaves_the_theme_alone_and_others_override_it() {
        let mut s = AppSettings::default();
        assert_eq!(appearance_of(&s, Some(true)).accent, None);
        s.theme.accent_color = "#3b82f6".into();
        let a = appearance_of(&s, Some(true));
        assert_eq!(a.accent, Some((0x3b, 0x82, 0xf6)));
        // Navy-ish blue needs light text; honey needs dark.
        assert_eq!(a.accent_foreground, crate::settings::TEXT_ON_DARK);
        s.theme.accent_color = "#fde047".into();
        assert_eq!(
            appearance_of(&s, None).accent_foreground,
            crate::settings::TEXT_ON_LIGHT
        );
    }

    #[test]
    fn an_unparseable_stored_accent_falls_back_to_the_brand() {
        let mut s = AppSettings::default();
        s.theme.accent_color = "not a colour".into();
        assert_eq!(appearance_of(&s, None).accent, None);
    }

    #[test]
    fn the_mode_follows_the_setting_and_the_os() {
        let mut s = AppSettings::default();
        s.theme.mode = ThemeMode::System;
        assert_eq!(appearance_of(&s, Some(false)).mode, ResolvedMode::Light);
        assert_eq!(appearance_of(&s, Some(true)).mode, ResolvedMode::Dark);
        s.theme.mode = ThemeMode::Light;
        assert_eq!(appearance_of(&s, Some(true)).mode, ResolvedMode::Light);
    }

    #[test]
    fn editor_font_size_is_clamped_for_a_hand_edited_file() {
        let mut s = AppSettings::default();
        s.theme.editor_font_size = 400;
        assert_eq!(appearance_of(&s, None).editor_font_size, 24);
        s.theme.editor_font_size = 1;
        assert_eq!(appearance_of(&s, None).editor_font_size, 10);
    }

    #[test]
    fn the_destructive_guard_follows_the_setting() {
        let mut q = QuerySettings::default();
        let sql = "DELETE FROM orders";
        assert!(!destructive_warnings(&q, sql).is_empty());
        q.confirm_destructive = false;
        assert!(destructive_warnings(&q, sql).is_empty());
        // A harmless statement never warns either way.
        q.confirm_destructive = true;
        assert!(destructive_warnings(&q, "SELECT 1").is_empty());
    }

    #[test]
    fn a_zero_timeout_means_no_limit() {
        let mut q = QuerySettings::default();
        assert_eq!(query_timeout(&q), Some(std::time::Duration::from_secs(30)));
        q.timeout_seconds = 0;
        assert_eq!(query_timeout(&q), None);
        q.timeout_seconds = 5;
        assert!(timeout_message(&q).starts_with("Query timed out after 5 seconds."));
    }

    #[test]
    fn the_title_bar_toggle_changes_the_mode_without_making_the_form_dirty() {
        let mut e = editor();
        e.edit(|s| s.query.max_rows = 2000);
        let written = e.set_theme_mode_saved(ThemeMode::Light);
        assert_eq!(written.theme.mode, ThemeMode::Light);
        assert_eq!(
            written.query.max_rows, 1000,
            "an unsaved edit must not ride along"
        );
        assert_eq!(e.draft().theme.mode, ThemeMode::Light);
        e.discard();
        assert_eq!(
            e.draft().theme.mode,
            ThemeMode::Light,
            "discard keeps the toggled mode"
        );
        assert!(!e.is_dirty());
    }

    #[test]
    fn history_keeps_the_newest_entries() {
        let mut q = QuerySettings::default();
        q.max_history_entries = 3;
        let mut v: Vec<u32> = (1..=5).collect();
        trim_history(&mut v, &q);
        assert_eq!(v, [3, 4, 5]);
        let mut short = vec![1, 2];
        trim_history(&mut short, &q);
        assert_eq!(short, [1, 2]);
    }

    #[test]
    fn recording_binds_a_combo_and_stops() {
        let mut e = editor();
        e.start_recording(ShortcutField::NewTab);
        assert_eq!(
            e.record(ctrl(), "J", false),
            RecordOutcome::Bound {
                combo: "Ctrl+J".into(),
                warning: None
            }
        );
        assert_eq!(e.draft().shortcuts.new_tab, "Ctrl+J");
        assert!(e.recording().is_none());
        // Not recording any more: further keys do nothing.
        assert_eq!(e.record(ctrl(), "Z", false), RecordOutcome::Idle);
        assert_eq!(e.draft().shortcuts.new_tab, "Ctrl+J");
    }

    #[test]
    fn a_duplicate_binding_is_stored_with_a_warning() {
        let mut e = editor();
        e.start_recording(ShortcutField::NewTab);
        let out = e.record(ctrl(), "W", false);
        match out {
            RecordOutcome::Bound { combo, warning } => {
                assert_eq!(combo, "Ctrl+W");
                assert_eq!(
                    warning.unwrap(),
                    "\"Ctrl+W\" is already bound to: Close Tab"
                );
            }
            other => panic!("expected a binding, got {:?}", other),
        }
        assert_eq!(e.draft().shortcuts.new_tab, "Ctrl+W");
        let rows = e.shortcut_rows();
        let new_tab = rows
            .iter()
            .find(|r| r.field == ShortcutField::NewTab)
            .unwrap();
        let close_tab = rows
            .iter()
            .find(|r| r.field == ShortcutField::CloseTab)
            .unwrap();
        assert!(new_tab.conflict && close_tab.conflict);
    }

    #[test]
    fn backspace_unbinds_and_escape_cancels() {
        let mut e = editor();
        e.start_recording(ShortcutField::Search);
        assert_eq!(
            e.record(KeyModifiers::default(), "Escape", false),
            RecordOutcome::Cancelled
        );
        assert_eq!(e.draft().shortcuts.search, "Ctrl+F");
        e.start_recording(ShortcutField::Search);
        assert_eq!(
            e.record(KeyModifiers::default(), "Backspace", false),
            RecordOutcome::Cleared
        );
        assert_eq!(e.draft().shortcuts.search, "");
        let rows = e.shortcut_rows();
        let row = rows
            .iter()
            .find(|r| r.field == ShortcutField::Search)
            .unwrap();
        assert_eq!(row.display, "(unbound)");
    }

    #[test]
    fn a_bare_letter_is_refused_and_recording_continues() {
        let mut e = editor();
        e.start_recording(ShortcutField::Search);
        match e.record(KeyModifiers::default(), "Q", false) {
            RecordOutcome::Invalid(msg) => assert!(msg.contains("needs a modifier")),
            other => panic!("expected invalid, got {:?}", other),
        }
        assert_eq!(e.recording(), Some(ShortcutField::Search));
        assert_eq!(e.draft().shortcuts.search, "Ctrl+F");
        // A bare modifier press is just waiting.
        assert_eq!(e.record(ctrl(), "", false), RecordOutcome::Waiting);
    }

    #[test]
    fn the_recording_row_shows_the_prompt() {
        let mut e = editor();
        e.start_recording_index(0);
        let rows = e.shortcut_rows();
        assert!(rows[0].recording);
        assert_eq!(rows[0].display, "Press shortcut\u{2026}");
        assert!(rows.iter().skip(1).all(|r| !r.recording));
    }

    #[test]
    fn rows_are_grouped_once_per_group_in_a_stable_order() {
        let rows = editor().shortcut_rows();
        assert_eq!(rows.len(), SettingsEditor::shortcut_count());
        assert_eq!(rows.len(), 10);
        let groups: Vec<_> = rows.iter().filter_map(|r| r.group).collect();
        assert_eq!(groups, ["Editor", "Navigation"]);
        assert_eq!(rows[0].field, ShortcutField::ExecuteQuery);
        assert!(
            rows.iter().all(|r| !r.conflict),
            "defaults have no collisions"
        );
    }

    #[test]
    fn persisting_a_shortcut_does_not_save_other_unsaved_edits() {
        let mut e = editor();
        e.edit(|s| s.query.timeout_seconds = 99);
        e.start_recording(ShortcutField::NewTab);
        e.record(ctrl(), "J", false);
        let to_write = e.saved_with_draft_shortcuts();
        assert_eq!(to_write.shortcuts.new_tab, "Ctrl+J");
        assert_eq!(
            to_write.query.timeout_seconds, 30,
            "the unsaved timeout must stay unsaved"
        );
        e.mark_shortcuts_saved(&to_write.shortcuts);
        assert!(e.is_dirty(), "the timeout is still unsaved");
        assert_eq!(e.saved().shortcuts.new_tab, "Ctrl+J");
    }

    #[test]
    fn changing_section_stops_recording() {
        let mut e = editor();
        e.start_recording(ShortcutField::Search);
        e.set_section(Section::General);
        assert!(e.recording().is_none());
    }

    #[test]
    fn adopt_clears_typed_text_and_resyncs_the_hex_box() {
        let mut e = editor();
        e.set_number_text(NumberField::MaxRows, "1");
        e.set_accent_text("zzz");
        let mut s = AppSettings::default();
        s.theme.accent_color = "#22c55e".into();
        e.adopt(s);
        assert!(!e.has_errors());
        assert_eq!(e.accent_input(), "#22c55e");
        assert!(!e.is_dirty());
    }

    #[test]
    fn index_selection_of_a_section_wraps_nothing() {
        let mut e = editor();
        e.select_section_index(3);
        assert_eq!(e.section(), Section::Shortcuts);
        assert_eq!(e.section_index(), 3);
        e.select_section_index(9);
        assert_eq!(e.section(), Section::Shortcuts);
    }
}
