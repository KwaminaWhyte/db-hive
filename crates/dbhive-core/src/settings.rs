//! Loading, validating and persisting `AppSettings`.
//!
//! The model itself lives in [`crate::models::settings`]; this module is the
//! shell-agnostic persistence and validation layer both shells call.
//!
//! On disk this is `settings.json` in the application data directory, with a
//! single `"settings"` key: byte-for-byte the shape `tauri_plugin_store`
//! wrote from `src-tauri/src/commands/settings.rs`, so an existing
//! installation keeps its preferences when it moves to the native shell.
//!
//! Two behaviours are deliberate:
//!
//! * **Merge over defaults.** A stored object that is missing keys (written
//!   by an older version, or hand-edited) is merged onto the defaults rather
//!   than rejected. Strict deserialisation would turn "we added a field" into
//!   "the user's settings file no longer loads".
//! * **Sanitise on load and on save.** The React UI enforced its ranges with
//!   HTML `min`/`max` attributes, which are advisory: a hand-edited file could
//!   carry `maxRows: 0` and quietly break every query. Clamping happens here so
//!   no shell has to remember to do it.

use serde_json::Value;

use crate::models::settings::{
    AppSettings, GeneralSettings, QuerySettings, ThemeMode, ThemeSettings,
};
use crate::models::DbError;
use crate::storage::JsonStore;

/// Store file name. Matches what the Tauri shell used.
pub const STORE_NAME: &str = "settings.json";

/// Key inside the store holding the whole settings object.
pub const STORE_KEY: &str = "settings";

// Accepted ranges, mirroring the bounds the React settings form advertised.
pub const UPDATE_INTERVAL_HOURS: (u32, u32) = (1, 168);
pub const EDITOR_FONT_SIZE: (u32, u32) = (10, 24);
pub const QUERY_TIMEOUT_SECONDS: (u32, u32) = (0, 300);
pub const MAX_ROWS: (u32, u32) = (100, 100_000);
pub const MAX_HISTORY_ENTRIES: (u32, u32) = (50, 2000);

/// Languages the picker offers. There is no i18n behind these yet; the value
/// is stored and nothing else, exactly as in the web shell.
pub const LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("es", "Español"),
    ("fr", "Français"),
    ("de", "Deutsch"),
];

/// Read the saved settings, falling back to defaults on a first run.
pub fn load() -> Result<AppSettings, DbError> {
    let store = JsonStore::open(STORE_NAME)?;
    load_from(&store)
}

/// Read settings out of an already-open store.
pub fn load_from(store: &JsonStore) -> Result<AppSettings, DbError> {
    match store.get(STORE_KEY) {
        Some(value) => from_value(value),
        None => Ok(AppSettings::default()),
    }
}

/// Deserialise a stored settings object, merged over the defaults and
/// clamped to the accepted ranges.
pub fn from_value(value: &Value) -> Result<AppSettings, DbError> {
    let mut merged = serde_json::to_value(AppSettings::default()).map_err(|e| {
        DbError::InternalError(format!("Failed to serialize default settings: {}", e))
    })?;
    deep_merge(&mut merged, value);

    let mut settings: AppSettings = serde_json::from_value(merged)
        .map_err(|e| DbError::InternalError(format!("Failed to deserialize settings: {}", e)))?;
    sanitize(&mut settings);
    Ok(settings)
}

/// Write settings to disk, clamping them first so an out-of-range value can
/// never be persisted.
pub fn save(settings: &AppSettings) -> Result<AppSettings, DbError> {
    let mut store = JsonStore::open(STORE_NAME)?;
    save_to(&mut store, settings)
}

/// Write settings into an already-open store and flush it.
pub fn save_to(store: &mut JsonStore, settings: &AppSettings) -> Result<AppSettings, DbError> {
    let mut settings = settings.clone();
    sanitize(&mut settings);

    let value = serde_json::to_value(&settings)
        .map_err(|e| DbError::InternalError(format!("Failed to serialize settings: {}", e)))?;
    store.set(STORE_KEY, value);
    store.save()?;
    Ok(settings)
}

/// Replace the stored settings with the factory defaults and return them.
pub fn reset() -> Result<AppSettings, DbError> {
    save(&AppSettings::default())
}

/// Clamp every bounded field into range and drop values the UI cannot
/// represent. Idempotent, and safe to call on anything.
pub fn sanitize(settings: &mut AppSettings) {
    sanitize_general(&mut settings.general);
    sanitize_theme(&mut settings.theme);
    sanitize_query(&mut settings.query);
}

fn sanitize_general(g: &mut GeneralSettings) {
    if !LANGUAGES.iter().any(|(code, _)| *code == g.language) {
        g.language = "en".to_string();
    }
    g.update_check_interval_hours = clamp(g.update_check_interval_hours, UPDATE_INTERVAL_HOURS);
    // Installing implies downloading; the React form nested the control so
    // the impossible combination was unreachable there.
    if !g.auto_download_updates {
        g.auto_install_updates = false;
    }
    if g.default_database.as_deref() == Some("") {
        g.default_database = None;
    }
}

fn sanitize_theme(t: &mut ThemeSettings) {
    t.editor_font_size = clamp(t.editor_font_size, EDITOR_FONT_SIZE);
    match parse_hex_color(&t.accent_color) {
        // Normalise "#F5A" and "f59e0b" to the canonical "#f59e0b" form so a
        // comparison against the default is meaningful.
        Some(rgb) => t.accent_color = hex_string(rgb),
        None => t.accent_color = ThemeSettings::default().accent_color,
    }
    if t.editor_font_family.trim().is_empty() {
        t.editor_font_family = ThemeSettings::default().editor_font_family;
    }
}

fn sanitize_query(q: &mut QuerySettings) {
    q.timeout_seconds = clamp(q.timeout_seconds, QUERY_TIMEOUT_SECONDS);
    q.max_rows = clamp(q.max_rows, MAX_ROWS);
    q.max_history_entries = clamp(q.max_history_entries, MAX_HISTORY_ENTRIES);
}

fn clamp(v: u32, (lo, hi): (u32, u32)) -> u32 {
    v.max(lo).min(hi)
}

/// The default accent, as RGB. Used when the stored value is unparseable.
pub fn default_accent_rgb() -> (u8, u8, u8) {
    parse_hex_color(&ThemeSettings::default().accent_color).expect("default accent is valid hex")
}

/// Parse `#rrggbb`, `#rgb`, or the same without the leading `#`.
///
/// Returns `None` rather than a partial colour: a half-parsed accent would
/// paint the UI a colour the user never chose.
pub fn parse_hex_color(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.trim().trim_start_matches('#');
    if !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    match h.len() {
        6 => Some((byte(0)?, byte(2)?, byte(4)?)),
        3 => {
            let d = |i: usize| u8::from_str_radix(&h[i..i + 1], 16).ok().map(|v| v * 17);
            Some((d(0)?, d(1)?, d(2)?))
        }
        _ => None,
    }
}

/// Render RGB back to the lower-case `#rrggbb` form the store holds.
pub fn hex_string((r, g, b): (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

/// Human label for a theme mode, shared by both shells' pickers.
pub fn theme_mode_label(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Light => "Light",
        ThemeMode::Dark => "Dark",
        ThemeMode::System => "System",
    }
}

/// Recursively overlay `patch` onto `base`.
///
/// Objects merge key by key; anything else replaces wholesale. This is what
/// lets a settings file written before a field existed still load.
fn deep_merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(base_map), Value::Object(patch_map)) => {
            for (k, v) in patch_map {
                match base_map.get_mut(k) {
                    Some(slot) => deep_merge(slot, v),
                    None => {
                        base_map.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (base, patch) => *base = patch.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::settings::StartupBehavior;
    use std::path::PathBuf;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dbhive-settings-test-{}-{}.json",
            std::process::id(),
            name
        ))
    }

    #[test]
    fn missing_store_yields_defaults() {
        let p = temp_path("missing");
        let _ = std::fs::remove_file(&p);
        let store = JsonStore::open_at(p).unwrap();
        let loaded = load_from(&store).unwrap();
        assert_eq!(loaded.general.language, "en");
        assert_eq!(loaded.theme.mode, ThemeMode::System);
        assert_eq!(loaded.query.max_rows, 1000);
        assert_eq!(loaded.shortcuts.execute_query, "Ctrl+Enter");
    }

    #[test]
    fn round_trips_through_disk() {
        let p = temp_path("roundtrip");
        let _ = std::fs::remove_file(&p);

        let mut settings = AppSettings::default();
        settings.general.language = "fr".into();
        settings.general.startup_behavior = StartupBehavior::ShowQueryEditor;
        settings.theme.mode = ThemeMode::Dark;
        settings.theme.accent_color = "#3366ff".into();
        settings.theme.editor_font_size = 18;
        settings.theme.editor_word_wrap = true;
        settings.query.max_rows = 5000;
        settings.shortcuts.execute_query = "Cmd+Enter".into();

        let mut store = JsonStore::open_at(p.clone()).unwrap();
        save_to(&mut store, &settings).unwrap();

        let reopened = JsonStore::open_at(p.clone()).unwrap();
        let loaded = load_from(&reopened).unwrap();
        assert_eq!(loaded.general.language, "fr");
        assert_eq!(
            loaded.general.startup_behavior,
            StartupBehavior::ShowQueryEditor
        );
        assert_eq!(loaded.theme.mode, ThemeMode::Dark);
        assert_eq!(loaded.theme.accent_color, "#3366ff");
        assert_eq!(loaded.theme.editor_font_size, 18);
        assert!(loaded.theme.editor_word_wrap);
        assert_eq!(loaded.query.max_rows, 5000);
        assert_eq!(loaded.shortcuts.execute_query, "Cmd+Enter");

        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn on_disk_shape_matches_the_tauri_store() {
        // The native shell must read and write the file the Tauri shell
        // wrote: one "settings" key, camelCase field names.
        let p = temp_path("shape");
        let _ = std::fs::remove_file(&p);

        let mut store = JsonStore::open_at(p.clone()).unwrap();
        save_to(&mut store, &AppSettings::default()).unwrap();

        let raw = std::fs::read_to_string(&p).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        let s = v.get("settings").expect("settings key");
        assert_eq!(s["general"]["updateCheckIntervalHours"], 24);
        assert_eq!(s["general"]["startupBehavior"], "showConnectionList");
        assert_eq!(s["theme"]["accentColor"], "#f59e0b");
        assert_eq!(s["theme"]["editorLineNumbers"], true);
        assert_eq!(s["query"]["maxHistoryEntries"], 500);
        assert_eq!(s["shortcuts"]["formatSql"], "Ctrl+Shift+F");

        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn partial_stored_object_merges_over_defaults() {
        // A file written before a field existed must still load, keeping the
        // keys it does have.
        let stored = serde_json::json!({
            "theme": { "mode": "dark" },
            "query": { "maxRows": 250 }
        });
        let loaded = from_value(&stored).unwrap();
        assert_eq!(loaded.theme.mode, ThemeMode::Dark);
        assert_eq!(loaded.query.max_rows, 250);
        // Untouched keys keep their defaults.
        assert_eq!(loaded.theme.accent_color, "#f59e0b");
        assert_eq!(loaded.general.language, "en");
        assert_eq!(loaded.shortcuts.close_tab, "Ctrl+W");
    }

    #[test]
    fn unknown_keys_are_ignored_not_fatal() {
        let stored =
            serde_json::json!({ "somethingFromTheFuture": 1, "theme": { "mode": "light" } });
        assert_eq!(from_value(&stored).unwrap().theme.mode, ThemeMode::Light);
    }

    #[test]
    fn wrong_type_is_an_error_rather_than_silent_defaults() {
        let stored = serde_json::json!({ "query": { "maxRows": "lots" } });
        assert!(from_value(&stored).is_err());
    }

    #[test]
    fn out_of_range_numbers_are_clamped() {
        let mut s = AppSettings::default();
        s.theme.editor_font_size = 400;
        s.query.max_rows = 0;
        s.query.timeout_seconds = 99_999;
        s.query.max_history_entries = 1;
        s.general.update_check_interval_hours = 0;
        sanitize(&mut s);

        assert_eq!(s.theme.editor_font_size, 24);
        assert_eq!(s.query.max_rows, 100);
        assert_eq!(s.query.timeout_seconds, 300);
        assert_eq!(s.query.max_history_entries, 50);
        assert_eq!(s.general.update_check_interval_hours, 1);
    }

    #[test]
    fn zero_timeout_means_no_timeout_and_survives_clamping() {
        let mut s = AppSettings::default();
        s.query.timeout_seconds = 0;
        sanitize(&mut s);
        assert_eq!(s.query.timeout_seconds, 0);
    }

    #[test]
    fn sanitize_is_idempotent() {
        let mut once = AppSettings::default();
        once.theme.editor_font_size = 999;
        once.theme.accent_color = "nonsense".into();
        sanitize(&mut once);
        let mut twice = once.clone();
        sanitize(&mut twice);
        assert_eq!(
            serde_json::to_value(&once).unwrap(),
            serde_json::to_value(&twice).unwrap()
        );
    }

    #[test]
    fn unknown_language_falls_back_to_english() {
        let mut s = AppSettings::default();
        s.general.language = "klingon".into();
        sanitize(&mut s);
        assert_eq!(s.general.language, "en");
    }

    #[test]
    fn auto_install_requires_auto_download() {
        let mut s = AppSettings::default();
        s.general.auto_download_updates = false;
        s.general.auto_install_updates = true;
        sanitize(&mut s);
        assert!(!s.general.auto_install_updates);
    }

    #[test]
    fn empty_font_family_falls_back() {
        let mut s = AppSettings::default();
        s.theme.editor_font_family = "   ".into();
        sanitize(&mut s);
        assert_eq!(
            s.theme.editor_font_family,
            ThemeSettings::default().editor_font_family
        );
    }

    #[test]
    fn accent_colour_is_parsed_and_normalised() {
        assert_eq!(parse_hex_color("#f59e0b"), Some((0xf5, 0x9e, 0x0b)));
        assert_eq!(parse_hex_color("f59e0b"), Some((0xf5, 0x9e, 0x0b)));
        assert_eq!(parse_hex_color("  #F59E0B "), Some((0xf5, 0x9e, 0x0b)));
        assert_eq!(parse_hex_color("#abc"), Some((0xaa, 0xbb, 0xcc)));
        assert_eq!(parse_hex_color("#ab"), None);
        assert_eq!(parse_hex_color("#gggggg"), None);
        assert_eq!(parse_hex_color(""), None);
        assert_eq!(hex_string((0xf5, 0x9e, 0x0b)), "#f59e0b");
        assert_eq!(hex_string(default_accent_rgb()), "#f59e0b");
    }

    #[test]
    fn invalid_accent_reverts_to_the_brand_colour() {
        let mut s = AppSettings::default();
        s.theme.accent_color = "rgb(1,2,3)".into();
        sanitize(&mut s);
        assert_eq!(s.theme.accent_color, "#f59e0b");

        let mut s = AppSettings::default();
        s.theme.accent_color = "#ABC".into();
        sanitize(&mut s);
        assert_eq!(s.theme.accent_color, "#aabbcc");
    }

    #[test]
    fn saving_clamps_before_it_writes() {
        let p = temp_path("clamp-on-save");
        let _ = std::fs::remove_file(&p);

        let mut s = AppSettings::default();
        s.query.max_rows = 10;
        let mut store = JsonStore::open_at(p.clone()).unwrap();
        let written = save_to(&mut store, &s).unwrap();
        assert_eq!(written.query.max_rows, 100);

        let reopened = JsonStore::open_at(p.clone()).unwrap();
        assert_eq!(load_from(&reopened).unwrap().query.max_rows, 100);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn deep_merge_replaces_scalars_and_merges_objects() {
        let mut base = serde_json::json!({"a": {"b": 1, "c": 2}, "d": 3});
        deep_merge(&mut base, &serde_json::json!({"a": {"c": 9}, "e": 4}));
        assert_eq!(
            base,
            serde_json::json!({"a": {"b": 1, "c": 9}, "d": 3, "e": 4})
        );
    }

    #[test]
    fn theme_mode_labels() {
        assert_eq!(theme_mode_label(ThemeMode::System), "System");
        assert_eq!(theme_mode_label(ThemeMode::Light), "Light");
        assert_eq!(theme_mode_label(ThemeMode::Dark), "Dark");
    }
}
