//! Plugin manager view logic: the tab set, carrying field edits across a
//! re-render, bundled plugin lookup, and date display.
//!
//! Lifted from the egui `screens::plugins`, which keeps the drawing, the
//! jobs and the engine calls. The plugin engine itself stays in
//! `dbhive_core::plugins`.

use std::path::PathBuf;

use dbhive_core::plugins::ui as descriptor;

#[cfg(test)]
use descriptor::FieldValue;
use descriptor::PluginUi;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Installed,
    Marketplace,
}

impl Tab {
    pub const ALL: [Tab; 2] = [Tab::Installed, Tab::Marketplace];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Installed => "Installed",
            Tab::Marketplace => "Marketplace",
        }
    }
}

/// Where a plugin shipped with the application might be.
pub fn bundled_source(plugin_id: &str) -> Option<PathBuf> {
    let folder = plugin_id.rsplit('.').next().unwrap_or(plugin_id);
    if folder.is_empty() {
        return None;
    }
    let mut roots = vec![PathBuf::from("plugins"), PathBuf::from("../plugins")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.join("plugins"));
            roots.push(dir.join("../Resources/plugins"));
        }
    }
    roots
        .into_iter()
        .map(|root| root.join(folder))
        .find(|c| c.join("manifest.json").is_file())
}

/// Carry the user's edits across a re-render.
///
/// A plugin that returns a fresh descriptor after every keystroke would
/// otherwise reset the field being typed into. Values are kept only for
/// fields the new descriptor still declares.
pub fn merge_fields(next: &PluginUi, previous: &descriptor::Fields) -> descriptor::Fields {
    let mut fields = next.initial_fields();
    for (id, value) in previous {
        if next.has_field(id) {
            fields.insert(id.clone(), value.clone());
        }
    }
    fields
}

/// "2026-01-05T10:00:00Z" -> "2026-01-05". The React shell used date-fns;
/// the date part of an RFC 3339 stamp needs no library, and anything that is
/// not one is shown as it arrived rather than as "Invalid Date".
pub fn short_date(raw: &str) -> String {
    match raw.split('T').next() {
        Some(date) if date.len() == 10 => date.to_string(),
        _ => raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The descriptor schema, manifest parsing, dispatch mapping, the
    // marketplace filters and the plugin store are tested in their own files
    // under crates/dbhive-core/src/plugins/. What is left to test here is the
    // screen's own logic.

    fn ui_with_fields() -> PluginUi {
        PluginUi::parse(
            r#"{"panels":[{"title":"p","items":[
                {"type":"input","id":"name","label":"n","value":"default"},
                {"type":"checkbox","id":"flag","label":"f","value":true}
            ]}]}"#,
        )
        .unwrap()
    }

    #[test]
    fn re_rendering_keeps_what_the_user_typed() {
        let d = ui_with_fields();
        let mut edited = d.initial_fields();
        edited.insert("name".into(), FieldValue::Text("typed".into()));

        let merged = merge_fields(&d, &edited);
        assert_eq!(merged["name"], FieldValue::Text("typed".into()));
        assert_eq!(merged["flag"], FieldValue::Bool(true));
    }

    #[test]
    fn fields_the_new_descriptor_dropped_are_not_carried_over() {
        let before = ui_with_fields();
        let mut edited = before.initial_fields();
        edited.insert("name".into(), FieldValue::Text("typed".into()));

        let after = PluginUi::parse(
            r#"{"panels":[{"title":"p","items":[
                {"type":"checkbox","id":"flag","label":"f","value":false}
            ]}]}"#,
        )
        .unwrap();

        let merged = merge_fields(&after, &edited);
        assert!(!merged.contains_key("name"), "a removed field is dropped");
        assert_eq!(
            merged["flag"],
            FieldValue::Bool(true),
            "a surviving field keeps the user's value"
        );
    }

    #[test]
    fn a_brand_new_field_takes_the_plugins_value() {
        let after = ui_with_fields();
        let merged = merge_fields(&after, &descriptor::Fields::new());
        assert_eq!(merged["name"], FieldValue::Text("default".into()));
    }

    #[test]
    fn install_dates_render_without_a_date_library() {
        assert_eq!(short_date("2026-01-05T10:00:00Z"), "2026-01-05");
        assert_eq!(short_date("2026-01-05T10:00:00.123+02:00"), "2026-01-05");
        assert_eq!(short_date("not a date"), "not a date");
        assert_eq!(short_date(""), "");
    }

    #[test]
    fn bundled_lookup_uses_the_last_segment_of_the_id() {
        // No assertion about the filesystem: the point is that odd ids do
        // not panic or build a path out of nothing.
        assert!(bundled_source("").is_none());
        assert!(bundled_source("com.dbhive.does-not-exist").is_none());
    }

    #[test]
    fn a_settings_form_is_built_from_the_config_schema_with_no_javascript() {
        // The parity item "configure it via a dynamically-generated settings
        // form" is pure data, and works whether or not the engine is wired.
        let schema = serde_json::json!({
            "properties": {
                "delimiter": { "type": "string", "enum": [",", ";"], "default": "," },
                "headers": { "type": "boolean", "default": true }
            }
        });
        let panel = descriptor::config_schema_panel(&schema, None);
        let form = PluginUi {
            title: None,
            toolbar: vec![],
            menu: vec![],
            panels: vec![panel],
        };
        let fields = form.initial_fields();
        assert_eq!(fields["delimiter"], FieldValue::Text(",".into()));
        assert_eq!(fields["headers"], FieldValue::Bool(true));

        assert_eq!(
            descriptor::fields_to_config(&fields),
            serde_json::json!({ "delimiter": ",", "headers": true })
        );
    }
}
