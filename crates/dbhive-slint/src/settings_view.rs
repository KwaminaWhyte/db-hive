//! What the Settings dialog shows, as pure functions of the editor state.
//!
//! `SettingsEditor` (in `dbhive-ui-model`) owns the decisions; this turns its
//! state into the `SettingsData` struct the dialog draws, and turns Slint key
//! events into the shell-neutral form the shortcut recorder takes. Neither
//! touches a window, so both are unit tested.

use dbhive_core::settings as core_settings;
use dbhive_ui_model::settings::{
    key_label_for_name, startup_label, KeyModifiers, Section, ShortcutField,
};
use dbhive_ui_model::settings_editor::{
    language_label, language_labels, startup_labels, theme_mode_labels, NumberField,
    SettingsEditor, ACCENT_PRESETS,
};
use slint::platform::Key;
use slint::{ModelRc, SharedString, VecModel};

use crate::{SettingsData, ShortcutRowData, Tone};

/// Where the settings file read stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Load {
    Ready,
    Loading,
    Failed(String),
}

/// A line at the top of the dialog, with its tone.
#[derive(Debug, Clone, PartialEq)]
pub struct Banner {
    pub message: String,
    pub tone: Tone,
}

/// The dialog's state beyond the editor: loading, saving, a banner.
pub struct View {
    pub load: Load,
    pub saving: bool,
    pub banner: Option<Banner>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            load: Load::Ready,
            saving: false,
            banner: None,
        }
    }
}

fn strings(items: Vec<&str>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        items
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<_>>(),
    ))
}

/// Project the editor into the dialog's data.
pub fn data(ed: &SettingsEditor, view: &View) -> SettingsData {
    let d = ed.draft();
    let err = |f: NumberField| -> SharedString { ed.number_error(f).unwrap_or_default().into() };

    let shortcuts: Vec<ShortcutRowData> = ed
        .shortcut_rows()
        .into_iter()
        .map(|r| ShortcutRowData {
            label: r.label.into(),
            group: r.group.unwrap_or("").into(),
            binding: r.display.as_str().into(),
            recording: r.recording,
            conflict: r.conflict,
            bound: !r.field.get(&d.shortcuts).is_empty(),
        })
        .collect();

    let (state, state_message) = match &view.load {
        Load::Ready => (0, String::new()),
        Load::Loading => (1, String::new()),
        Load::Failed(msg) => (2, msg.clone()),
    };
    let (banner, banner_tone) = match &view.banner {
        Some(b) => (b.message.as_str(), b.tone),
        None => ("", Tone::Danger),
    };

    SettingsData {
        section: ed.section_index() as i32,
        section_labels: strings(Section::ALL.iter().map(|s| s.label()).collect()),
        section_blurb: ed.section().blurb().into(),
        state,
        state_message: state_message.into(),
        banner: banner.into(),
        banner_tone,
        dirty: ed.is_dirty(),
        can_save: ed.can_save(),
        saving: view.saving,

        language_options: strings(language_labels()),
        language: language_label(&d.general.language).into(),
        startup_options: strings(startup_labels()),
        startup: startup_label(d.general.startup_behavior).into(),
        auto_save_connections: d.general.auto_save_connections,
        auto_check_updates: d.general.auto_check_updates,
        update_interval: ed.number_text(NumberField::UpdateInterval).into(),
        update_interval_error: err(NumberField::UpdateInterval),
        auto_download: d.general.auto_download_updates,
        auto_install: d.general.auto_install_updates,

        theme_options: strings(theme_mode_labels()),
        theme_mode: core_settings::theme_mode_label(d.theme.mode).into(),
        accent_presets: ModelRc::new(VecModel::from(
            ACCENT_PRESETS
                .iter()
                .map(|(_, (r, g, b))| slint::Color::from_rgb_u8(*r, *g, *b))
                .collect::<Vec<_>>(),
        )),
        accent_index: ed.accent_preset_index(),
        accent_text: ed.accent_input().into(),
        accent_error: if ed.accent_is_valid() {
            SharedString::default()
        } else {
            "Not a hex colour".into()
        },
        font_size: ed.number_text(NumberField::EditorFontSize).into(),
        font_size_error: err(NumberField::EditorFontSize),
        font_family: d.theme.editor_font_family.as_str().into(),
        line_numbers: d.theme.editor_line_numbers,
        minimap: d.theme.editor_minimap,
        word_wrap: d.theme.editor_word_wrap,

        timeout: ed.number_text(NumberField::QueryTimeout).into(),
        timeout_error: err(NumberField::QueryTimeout),
        max_rows: ed.number_text(NumberField::MaxRows).into(),
        max_rows_error: err(NumberField::MaxRows),
        confirm_destructive: d.query.confirm_destructive,
        auto_save_history: d.query.auto_save_history,
        max_history: ed.number_text(NumberField::MaxHistory).into(),
        max_history_error: err(NumberField::MaxHistory),

        recording: ed.recording().is_some(),
        shortcuts: ModelRc::new(VecModel::from(shortcuts)),
    }
}

/// Which shortcut a row index means, for tests and callers that want the field.
pub fn field_at(index: usize) -> Option<ShortcutField> {
    dbhive_ui_model::settings_editor::shortcut_fields()
        .get(index)
        .copied()
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

/// Slint's modifier flags as the recorder's. Slint reports Command as
/// `control` on macOS and Control as `meta`; elsewhere `control` is Control
/// and `meta` is the Windows or Super key, which is not bindable.
pub fn key_modifiers(
    control: bool,
    alt: bool,
    shift: bool,
    meta: bool,
    is_mac: bool,
) -> KeyModifiers {
    if is_mac {
        KeyModifiers {
            alt,
            ctrl: meta,
            shift,
            mac_cmd: control,
            command: control,
        }
    } else {
        KeyModifiers {
            alt,
            ctrl: control,
            shift,
            mac_cmd: false,
            command: control,
        }
    }
}

/// The stored label of the key a Slint key event carries, or an empty string
/// for a bare modifier press, an input-method string or anything unbindable.
pub fn key_label(text: &str) -> String {
    let mut chars = text.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return String::new();
    };
    let is = |k: Key| c == char::from(k);

    let named = [
        (Key::Escape, "Escape"),
        (Key::Backspace, "Backspace"),
        (Key::Delete, "Delete"),
        (Key::Return, "Enter"),
        (Key::Tab, "Tab"),
        (Key::UpArrow, "ArrowUp"),
        (Key::DownArrow, "ArrowDown"),
        (Key::LeftArrow, "ArrowLeft"),
        (Key::RightArrow, "ArrowRight"),
        (Key::Home, "Home"),
        (Key::End, "End"),
        (Key::PageUp, "PageUp"),
        (Key::PageDown, "PageDown"),
        (Key::Insert, "Insert"),
    ];
    if let Some((_, name)) = named.iter().find(|(k, _)| is(*k)) {
        return (*name).to_string();
    }

    // F1..F24 are consecutive private-use characters.
    let f1 = char::from(Key::F1) as u32;
    let f24 = char::from(Key::F24) as u32;
    if (f1..=f24).contains(&(c as u32)) {
        return format!("F{}", c as u32 - f1 + 1);
    }

    if c == ' ' {
        return "Space".to_string();
    }
    // The winit backend reports the logical key, so Ctrl+K arrives as "k",
    // never as a C0 control code. Everything else non-printing (modifier keys, CapsLock, the rest of the
    // private-use range) is not a key a binding can end in.
    if c.is_control() || ('\u{e000}'..='\u{f8ff}').contains(&c) {
        return String::new();
    }
    if c.is_alphabetic() {
        return c.to_uppercase().to_string();
    }
    key_label_for_name(&c.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbhive_core::models::settings::AppSettings;

    fn k(key: Key) -> String {
        key_label(&char::from(key).to_string())
    }

    #[test]
    fn named_keys_use_the_stored_spelling() {
        assert_eq!(k(Key::Escape), "Escape");
        assert_eq!(k(Key::Backspace), "Backspace");
        assert_eq!(k(Key::Return), "Enter");
        assert_eq!(k(Key::UpArrow), "ArrowUp");
        assert_eq!(k(Key::F1), "F1");
        assert_eq!(k(Key::F5), "F5");
        assert_eq!(k(Key::F12), "F12");
        assert_eq!(key_label(" "), "Space");
    }

    #[test]
    fn letters_are_stored_upper_case_and_punctuation_as_the_symbol() {
        assert_eq!(key_label("k"), "K");
        assert_eq!(key_label("K"), "K");
        assert_eq!(key_label(","), ",");
        assert_eq!(key_label("?"), "?");
        assert_eq!(key_label("7"), "7");
    }

    #[test]
    fn bare_modifiers_and_ime_strings_are_not_keys() {
        assert_eq!(k(Key::Control), "");
        assert_eq!(k(Key::Shift), "");
        assert_eq!(k(Key::Alt), "");
        assert_eq!(k(Key::Meta), "");
        assert_eq!(k(Key::CapsLock), "");
        assert_eq!(key_label(""), "");
        assert_eq!(key_label("ab"), "");
        // Slint's modifier codes sit in the C0 range; they are not letters.
        assert_eq!(key_label("\u{11}"), "");
    }

    #[test]
    fn mac_swaps_the_slint_modifier_names() {
        // Slint: control is Command on macOS.
        let m = key_modifiers(true, false, false, false, true);
        assert!(m.mac_cmd && m.command && !m.ctrl);
        let m = key_modifiers(false, false, false, true, true);
        assert!(
            m.ctrl && !m.mac_cmd,
            "meta is the real Control key on macOS"
        );
        let m = key_modifiers(true, false, false, false, false);
        assert!(m.ctrl && m.command && !m.mac_cmd);
        // The Windows key never becomes a binding.
        let m = key_modifiers(false, false, false, true, false);
        assert!(!m.ctrl && !m.mac_cmd);
    }

    #[test]
    fn the_projection_reflects_the_editor() {
        let mut ed = SettingsEditor::new(AppSettings::default());
        let view = View::default();
        let d = data(&ed, &view);
        assert_eq!(d.section, 0);
        assert!(!d.dirty && !d.can_save && !d.recording);
        assert_eq!(d.language.as_str(), "English");
        assert_eq!(d.startup.as_str(), "Show Connection List");
        assert_eq!(d.theme_mode.as_str(), "System");
        assert_eq!(d.update_interval.as_str(), "24");
        assert_eq!(slint::Model::row_count(&d.shortcuts), 10);
        assert_eq!(slint::Model::row_count(&d.section_labels), 4);

        ed.set_number_text(NumberField::MaxRows, "5");
        ed.set_accent_text("zz");
        let d = data(&ed, &view);
        assert!(d.dirty && !d.can_save, "an invalid value blocks Save");
        assert!(d.max_rows_error.contains("between 100 and 100000"));
        assert_eq!(d.max_rows.as_str(), "5");
        assert!(d.accent_error.contains("hex"));

        ed.start_recording_index(0);
        let d = data(&ed, &view);
        assert!(d.recording);
        let first = slint::Model::row_data(&d.shortcuts, 0).unwrap();
        assert!(first.recording);
        assert_eq!(first.group.as_str(), "Editor");
        assert_eq!(first.binding.as_str(), "Press shortcut\u{2026}");
    }

    #[test]
    fn load_states_map_to_the_dialog_states() {
        let ed = SettingsEditor::new(AppSettings::default());
        let mut view = View::default();
        assert_eq!(data(&ed, &view).state, 0);
        view.load = Load::Loading;
        assert_eq!(data(&ed, &view).state, 1);
        view.load = Load::Failed("settings.json is not valid JSON".into());
        let d = data(&ed, &view);
        assert_eq!(d.state, 2);
        assert!(d.state_message.contains("not valid JSON"));
    }

    #[test]
    fn field_at_matches_the_rows() {
        let ed = SettingsEditor::new(AppSettings::default());
        let rows = ed.shortcut_rows();
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(field_at(i), Some(row.field));
        }
        assert_eq!(field_at(99), None);
    }
}
