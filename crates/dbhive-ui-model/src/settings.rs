//! Settings view logic, lifted from the egui `settings` screen. See lib.rs.
//!
//! The section list, the shortcut table (fields, groups, duplicate
//! detection, validation, and turning a keystroke into the stored
//! "Ctrl+Shift+F" spelling), the startup-behaviour labels, and the pure
//! halves of the theme code: resolving `ThemeMode::System` against the OS
//! preference, the accent fallback, and choosing readable text on a colour.
//!
//! Keystrokes arrive here as a [`KeyModifiers`] and the key's name, so a
//! shell converts its own event type and gets identical bindings.

use dbhive_core::models::settings::{ShortcutsSettings, StartupBehavior, ThemeMode};
use dbhive_core::settings as core_settings;

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    General,
    Appearance,
    Query,
    Shortcuts,
}

impl Section {
    pub const ALL: [Section; 4] = [
        Section::General,
        Section::Appearance,
        Section::Query,
        Section::Shortcuts,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Appearance => "Appearance",
            Section::Query => "Query Execution",
            Section::Shortcuts => "Keyboard Shortcuts",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Section::General => "General Settings",
            Section::Appearance => "Appearance",
            Section::Query => "Query Execution",
            Section::Shortcuts => "Keyboard Shortcuts",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Section::General => "Configure general application behavior and preferences",
            Section::Appearance => "Customize the look and feel of the application",
            Section::Query => "Configure how queries are executed and displayed",
            Section::Shortcuts => {
                "View and customize keyboard shortcuts (click any shortcut to rebind)"
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Theme decisions
// ---------------------------------------------------------------------------

/// A theme mode with `System` resolved away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedMode {
    Light,
    Dark,
}

/// Resolve the configured theme mode against the OS preference.
///
/// `system_dark` is `None` until the window manager has reported one.
pub fn resolve_theme_mode(mode: ThemeMode, system_dark: Option<bool>) -> ResolvedMode {
    match mode {
        ThemeMode::Light => ResolvedMode::Light,
        ThemeMode::Dark => ResolvedMode::Dark,
        // Before the window manager has reported a preference, keep the
        // shell's own default rather than flashing the other theme.
        ThemeMode::System => match system_dark {
            Some(false) => ResolvedMode::Light,
            Some(true) => ResolvedMode::Dark,
            None => ResolvedMode::Dark,
        },
    }
}

/// The setting to store when the mode was changed somewhere other than the
/// Settings screen (the title-bar Light/Dark toggle).
pub fn theme_mode_for(resolved: ResolvedMode) -> ThemeMode {
    match resolved {
        ResolvedMode::Light => ThemeMode::Light,
        ResolvedMode::Dark => ThemeMode::Dark,
    }
}

/// Accent as RGB, falling back to the brand honey if the stored value is
/// unparseable.
pub fn accent_rgb(accent_color: &str) -> (u8, u8, u8) {
    core_settings::parse_hex_color(accent_color).unwrap_or_else(core_settings::default_accent_rgb)
}

/// Near-black text, for light backgrounds.
pub const TEXT_ON_LIGHT: (u8, u8, u8) = (9, 11, 15);
/// Near-white text, for dark backgrounds.
pub const TEXT_ON_DARK: (u8, u8, u8) = (248, 248, 248);

/// Black or white, whichever stays legible on `bg`.
///
/// A user-chosen accent can be any colour; hard-coding the brand's dark
/// foreground would make a navy accent unreadable.
pub fn readable_on_rgb((r, g, b): (u8, u8, u8)) -> (u8, u8, u8) {
    let lum = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
    if lum > 140.0 {
        TEXT_ON_LIGHT
    } else {
        TEXT_ON_DARK
    }
}

// ---------------------------------------------------------------------------
// Startup behaviour
// ---------------------------------------------------------------------------

pub const STARTUP_BEHAVIORS: [StartupBehavior; 4] = [
    StartupBehavior::ShowConnectionList,
    StartupBehavior::OpenLastConnection,
    StartupBehavior::OpenDefaultConnection,
    StartupBehavior::ShowQueryEditor,
];

pub fn startup_label(behavior: StartupBehavior) -> &'static str {
    match behavior {
        StartupBehavior::ShowConnectionList => "Show Connection List",
        StartupBehavior::OpenLastConnection => "Open Last Connection",
        StartupBehavior::OpenDefaultConnection => "Open Default Connection",
        StartupBehavior::ShowQueryEditor => "Show Query Editor",
    }
}

// ---------------------------------------------------------------------------
// Keyboard shortcuts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutField {
    ExecuteQuery,
    ClearEditor,
    FormatSql,
    SaveSnippet,
    NewTab,
    CloseTab,
    ToggleSidebar,
    Search,
    OpenSettings,
    ShowShortcuts,
}

pub const EDITOR_SHORTCUTS: [ShortcutField; 4] = [
    ShortcutField::ExecuteQuery,
    ShortcutField::ClearEditor,
    ShortcutField::FormatSql,
    ShortcutField::SaveSnippet,
];

pub const NAVIGATION_SHORTCUTS: [ShortcutField; 6] = [
    ShortcutField::NewTab,
    ShortcutField::CloseTab,
    ShortcutField::ToggleSidebar,
    ShortcutField::Search,
    ShortcutField::OpenSettings,
    ShortcutField::ShowShortcuts,
];

impl ShortcutField {
    pub fn label(self) -> &'static str {
        match self {
            ShortcutField::ExecuteQuery => "Execute Query",
            ShortcutField::ClearEditor => "Clear Editor",
            ShortcutField::FormatSql => "Format SQL",
            ShortcutField::SaveSnippet => "Save Snippet",
            ShortcutField::NewTab => "New Tab",
            ShortcutField::CloseTab => "Close Tab",
            ShortcutField::ToggleSidebar => "Toggle Sidebar",
            ShortcutField::Search => "Search",
            ShortcutField::OpenSettings => "Open Settings",
            ShortcutField::ShowShortcuts => "Show Shortcuts",
        }
    }

    pub fn get(self, s: &ShortcutsSettings) -> &str {
        match self {
            ShortcutField::ExecuteQuery => &s.execute_query,
            ShortcutField::ClearEditor => &s.clear_editor,
            ShortcutField::FormatSql => &s.format_sql,
            ShortcutField::SaveSnippet => &s.save_snippet,
            ShortcutField::NewTab => &s.new_tab,
            ShortcutField::CloseTab => &s.close_tab,
            ShortcutField::ToggleSidebar => &s.toggle_sidebar,
            ShortcutField::Search => &s.search,
            ShortcutField::OpenSettings => &s.open_settings,
            ShortcutField::ShowShortcuts => &s.show_shortcuts,
        }
    }

    pub fn set(self, s: &mut ShortcutsSettings, value: String) {
        let slot = match self {
            ShortcutField::ExecuteQuery => &mut s.execute_query,
            ShortcutField::ClearEditor => &mut s.clear_editor,
            ShortcutField::FormatSql => &mut s.format_sql,
            ShortcutField::SaveSnippet => &mut s.save_snippet,
            ShortcutField::NewTab => &mut s.new_tab,
            ShortcutField::CloseTab => &mut s.close_tab,
            ShortcutField::ToggleSidebar => &mut s.toggle_sidebar,
            ShortcutField::Search => &mut s.search,
            ShortcutField::OpenSettings => &mut s.open_settings,
            ShortcutField::ShowShortcuts => &mut s.show_shortcuts,
        };
        *slot = value;
    }
}

/// Other bindings already using `value`. A warning, never a block: the web
/// shell allowed the duplicate and so does this.
pub fn duplicates(
    shortcuts: &ShortcutsSettings,
    own: ShortcutField,
    value: &str,
) -> Vec<&'static str> {
    if value.is_empty() {
        return Vec::new();
    }
    EDITOR_SHORTCUTS
        .iter()
        .chain(NAVIGATION_SHORTCUTS.iter())
        .filter(|f| **f != own)
        .filter(|f| f.get(shortcuts).eq_ignore_ascii_case(value))
        .map(|f| f.label())
        .collect()
}

/// Outcome of watching the keyboard while a binding is being recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recorded {
    /// Nothing usable yet; keep listening.
    Pending,
    /// Escape: leave the binding as it was.
    Cancel,
    /// Backspace: unbind.
    Clear,
    Bind(String),
    /// A keystroke that cannot be a binding, with the reason.
    Invalid(String),
}

/// The modifier state of one keystroke, in the same shape egui reports it:
/// `command` is Ctrl off macOS and Cmd on macOS, set alongside the real key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyModifiers {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub mac_cmd: bool,
    pub command: bool,
}

/// Format a keystroke the way `settings.json` stores it, e.g. "Ctrl+Shift+F".
///
/// `key` is the stored label of the key (see [`key_label_for_name`]).
/// `is_mac` is a parameter rather than a `cfg!` so both spellings can be
/// tested on either platform.
pub fn combo_string(modifiers: KeyModifiers, key: &str, is_mac: bool) -> String {
    // egui folds Ctrl into `command` off macOS and Cmd into it on macOS;
    // untangle that first or every binding gains a phantom "Ctrl".
    let (ctrl, cmd) = if is_mac {
        (modifiers.ctrl, modifiers.mac_cmd || modifiers.command)
    } else {
        (modifiers.ctrl || modifiers.command, false)
    };

    let mut parts: Vec<&str> = Vec::new();
    if ctrl {
        parts.push("Ctrl");
    }
    if cmd {
        parts.push("Cmd");
    }
    if modifiers.alt {
        parts.push(if is_mac { "Option" } else { "Alt" });
    }
    if modifiers.shift {
        parts.push("Shift");
    }
    parts.push(key);
    parts.join("+")
}

/// Key names spell punctuation out ("Comma"); the stored format uses the
/// symbol, so "Ctrl+," round-trips. Any other name is used as is.
pub fn key_label_for_name(name: &str) -> String {
    match name {
        "Comma" => ",",
        "Period" => ".",
        "Semicolon" => ";",
        "Colon" => ":",
        "Minus" => "-",
        "Plus" => "+",
        "Equals" => "=",
        "Slash" => "/",
        "Backslash" => "\\",
        "Pipe" => "|",
        "Questionmark" => "?",
        "OpenBracket" => "[",
        "CloseBracket" => "]",
        "Backtick" => "`",
        "Quote" => "'",
        other => other,
    }
    .to_string()
}

/// A binding needs a real modifier unless it is a named key ("F5", "Escape")
/// or "?": a bare letter would swallow ordinary typing.
pub fn is_valid_shortcut(combo: &str) -> bool {
    if combo.is_empty() {
        return false;
    }
    let parts: Vec<&str> = combo.split('+').collect();
    let last = *parts.last().unwrap_or(&"");
    // A trailing "+" means the bound key *is* "+", e.g. "Ctrl++".
    let last = if last.is_empty() { "+" } else { last };
    let has_real_modifier = parts[..parts.len().saturating_sub(1)].iter().any(|m| {
        matches!(
            m.to_ascii_lowercase().as_str(),
            "ctrl" | "cmd" | "meta" | "alt" | "option"
        )
    });
    if !has_real_modifier && last.chars().count() == 1 && last != "?" {
        return false;
    }
    true
}

/// Interpret one pressed key while recording.
///
/// `key` is the stored label of the key (so Escape is "Escape" and Backspace
/// is "Backspace").
pub fn record_keystroke(modifiers: KeyModifiers, key: &str, is_mac: bool) -> Recorded {
    if key == "Escape" {
        return Recorded::Cancel;
    }
    if key == "Backspace"
        && !modifiers.ctrl
        && !modifiers.mac_cmd
        && !modifiers.command
        && !modifiers.alt
    {
        return Recorded::Clear;
    }

    let combo = combo_string(modifiers, key, is_mac);
    if is_valid_shortcut(&combo) {
        Recorded::Bind(combo)
    } else {
        Recorded::Invalid(format!(
            "\"{}\" needs a modifier (Ctrl, Cmd or Alt) to be a shortcut",
            combo
        ))
    }
}

/// The toast shown when a new binding collides with others.
pub fn duplicate_warning(combo: &str, dupes: &[&str]) -> String {
    format!("\"{}\" is already bound to: {}", combo, dupes.join(", "))
}

/// Text on a shortcut's rebind button.
pub fn shortcut_display(recording: bool, binding: &str) -> String {
    if recording {
        "Press shortcut\u{2026}".to_string()
    } else if binding.is_empty() {
        "(unbound)".to_string()
    } else {
        binding.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(ctrl: bool, cmd: bool, alt: bool, shift: bool) -> KeyModifiers {
        KeyModifiers {
            alt,
            ctrl,
            shift,
            mac_cmd: cmd,
            command: cmd || ctrl,
        }
    }

    #[test]
    fn combo_matches_the_stored_format() {
        assert_eq!(
            combo_string(mods(true, false, false, false), "Enter", false),
            "Ctrl+Enter"
        );
        assert_eq!(
            combo_string(mods(true, false, false, true), "F", false),
            "Ctrl+Shift+F"
        );
        assert_eq!(
            combo_string(
                mods(true, false, false, false),
                &key_label_for_name("Comma"),
                false
            ),
            "Ctrl+,"
        );
        assert_eq!(
            combo_string(mods(false, false, false, false), "F5", false),
            "F5"
        );
    }

    #[test]
    fn command_is_spelled_per_platform() {
        let m = KeyModifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        assert_eq!(combo_string(m, "K", false), "Ctrl+K");

        let m = KeyModifiers {
            mac_cmd: true,
            command: true,
            ..Default::default()
        };
        assert_eq!(combo_string(m, "K", true), "Cmd+K");

        // Real Control on a Mac stays Control.
        let m = KeyModifiers {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(combo_string(m, "K", true), "Ctrl+K");
    }

    #[test]
    fn alt_is_option_on_mac() {
        let m = KeyModifiers {
            alt: true,
            ..Default::default()
        };
        assert_eq!(combo_string(m, "T", true), "Option+T");
        assert_eq!(combo_string(m, "T", false), "Alt+T");
    }

    #[test]
    fn punctuation_key_names_use_their_symbol() {
        assert_eq!(key_label_for_name("Comma"), ",");
        assert_eq!(key_label_for_name("Questionmark"), "?");
        assert_eq!(key_label_for_name("Slash"), "/");
        assert_eq!(key_label_for_name("Backslash"), "\\");
        assert_eq!(key_label_for_name("Quote"), "'");
        assert_eq!(key_label_for_name("Enter"), "Enter");
        assert_eq!(key_label_for_name("Up"), "Up");
    }

    #[test]
    fn record_keystroke_reports_cancel_clear_and_bind() {
        assert_eq!(
            record_keystroke(Default::default(), "Escape", false),
            Recorded::Cancel
        );
        assert_eq!(
            record_keystroke(mods(true, false, false, false), "Escape", false),
            Recorded::Cancel
        );
        assert_eq!(
            record_keystroke(Default::default(), "Backspace", false),
            Recorded::Clear
        );
        assert_eq!(
            record_keystroke(mods(false, false, false, true), "Backspace", false),
            Recorded::Clear
        );
        assert_eq!(
            record_keystroke(mods(true, false, false, false), "Enter", false),
            Recorded::Bind("Ctrl+Enter".into())
        );
        assert_eq!(
            record_keystroke(Default::default(), "K", false),
            Recorded::Invalid("\"K\" needs a modifier (Ctrl, Cmd or Alt) to be a shortcut".into())
        );
    }

    #[test]
    fn modified_backspace_is_a_binding_not_an_unbind() {
        assert_eq!(
            record_keystroke(mods(true, false, false, false), "Backspace", false),
            Recorded::Bind("Ctrl+Backspace".into())
        );
        assert_eq!(
            record_keystroke(mods(false, true, false, false), "Backspace", true),
            Recorded::Bind("Cmd+Backspace".into())
        );
        assert_eq!(
            record_keystroke(mods(false, false, true, false), "Backspace", false),
            Recorded::Bind("Alt+Backspace".into())
        );
    }

    #[test]
    fn duplicate_warning_lists_every_collision() {
        assert_eq!(
            duplicate_warning("Ctrl+K", &["Clear Editor", "New Tab"]),
            "\"Ctrl+K\" is already bound to: Clear Editor, New Tab"
        );
    }

    #[test]
    fn shortcut_display_shows_recording_unbound_or_the_binding() {
        assert_eq!(shortcut_display(true, "Ctrl+K"), "Press shortcut\u{2026}");
        assert_eq!(shortcut_display(false, ""), "(unbound)");
        assert_eq!(shortcut_display(false, "Ctrl+K"), "Ctrl+K");
    }

    #[test]
    fn system_mode_follows_the_os_and_defaults_to_dark() {
        assert_eq!(
            resolve_theme_mode(ThemeMode::Light, Some(true)),
            ResolvedMode::Light
        );
        assert_eq!(
            resolve_theme_mode(ThemeMode::Dark, Some(false)),
            ResolvedMode::Dark
        );
        assert_eq!(
            resolve_theme_mode(ThemeMode::System, Some(false)),
            ResolvedMode::Light
        );
        assert_eq!(
            resolve_theme_mode(ThemeMode::System, Some(true)),
            ResolvedMode::Dark
        );
        assert_eq!(
            resolve_theme_mode(ThemeMode::System, None),
            ResolvedMode::Dark
        );
    }

    #[test]
    fn theme_mode_for_round_trips_through_resolve() {
        for r in [ResolvedMode::Light, ResolvedMode::Dark] {
            assert_eq!(resolve_theme_mode(theme_mode_for(r), None), r);
        }
    }

    #[test]
    fn accent_rgb_falls_back_to_the_brand_colour_when_unparseable() {
        assert_eq!(
            accent_rgb("not a colour"),
            core_settings::default_accent_rgb()
        );
        assert_eq!(accent_rgb(""), core_settings::default_accent_rgb());
        assert_eq!(accent_rgb("#102030"), (0x10, 0x20, 0x30));
    }

    #[test]
    fn readable_text_on_rgb_is_dark_on_light_and_light_on_dark() {
        assert_eq!(readable_on_rgb((248, 164, 0)), TEXT_ON_LIGHT);
        assert_eq!(readable_on_rgb((20, 30, 90)), TEXT_ON_DARK);
        assert_eq!(readable_on_rgb((255, 255, 255)), TEXT_ON_LIGHT);
        assert_eq!(readable_on_rgb((0, 0, 0)), TEXT_ON_DARK);
    }

    #[test]
    fn bare_letters_are_rejected_but_named_keys_are_not() {
        assert!(!is_valid_shortcut("K"));
        assert!(!is_valid_shortcut("Shift+K"));
        assert!(!is_valid_shortcut(""));
        assert!(is_valid_shortcut("Ctrl+K"));
        assert!(is_valid_shortcut("Cmd+Enter"));
        assert!(is_valid_shortcut("Option+K"));
        assert!(is_valid_shortcut("F5"));
        assert!(is_valid_shortcut("Escape"));
        assert!(is_valid_shortcut("?"));
        assert!(is_valid_shortcut("Shift+?"));
        assert!(is_valid_shortcut("Ctrl++"));
    }

    #[test]
    fn every_default_binding_is_valid() {
        let defaults = ShortcutsSettings::default();
        for field in EDITOR_SHORTCUTS.iter().chain(NAVIGATION_SHORTCUTS.iter()) {
            let binding = field.get(&defaults);
            assert!(
                is_valid_shortcut(binding),
                "default binding {:?} = {:?} is rejected by the validator",
                field.label(),
                binding
            );
        }
    }

    #[test]
    fn duplicates_are_found_across_both_groups_but_not_self() {
        let mut s = ShortcutsSettings::default();
        s.new_tab = "Ctrl+K".into(); // same as the default clearEditor
        assert_eq!(
            duplicates(&s, ShortcutField::NewTab, "Ctrl+K"),
            vec!["Clear Editor"]
        );
        // A field never reports itself.
        assert!(duplicates(&s, ShortcutField::ClearEditor, "Ctrl+K").contains(&"New Tab"));
        assert!(!duplicates(&s, ShortcutField::ClearEditor, "Ctrl+K").contains(&"Clear Editor"));
        // Case-insensitive, and an empty binding is never a duplicate.
        assert_eq!(
            duplicates(&s, ShortcutField::NewTab, "ctrl+k"),
            vec!["Clear Editor"]
        );
        assert!(duplicates(&s, ShortcutField::NewTab, "").is_empty());
    }

    #[test]
    fn default_bindings_have_no_duplicates() {
        let defaults = ShortcutsSettings::default();
        for field in EDITOR_SHORTCUTS.iter().chain(NAVIGATION_SHORTCUTS.iter()) {
            let binding = field.get(&defaults).to_string();
            assert!(
                duplicates(&defaults, *field, &binding).is_empty(),
                "{} collides out of the box",
                field.label()
            );
        }
    }

    #[test]
    fn shortcut_field_get_and_set_address_the_same_slot() {
        let mut s = ShortcutsSettings::default();
        for field in EDITOR_SHORTCUTS.iter().chain(NAVIGATION_SHORTCUTS.iter()) {
            field.set(&mut s, format!("Ctrl+{}", field.label()));
            assert_eq!(field.get(&s), format!("Ctrl+{}", field.label()));
        }
    }

    #[test]
    fn sections_cover_the_four_react_tabs() {
        let labels: Vec<&str> = Section::ALL.iter().map(|s| s.label()).collect();
        assert_eq!(
            labels,
            vec![
                "General",
                "Appearance",
                "Query Execution",
                "Keyboard Shortcuts"
            ]
        );
    }

    #[test]
    fn startup_labels_cover_every_variant() {
        assert_eq!(STARTUP_BEHAVIORS.len(), 4);
        for b in STARTUP_BEHAVIORS {
            assert!(!startup_label(b).is_empty());
        }
    }
}
