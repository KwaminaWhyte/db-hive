//! Plugin manifests, permissions and installed-plugin state.
//!
//! The on-disk format is exactly the `manifest.json` the Tauri shell read,
//! so an existing plugin directory keeps working. What changed is that
//! parsing now *validates* instead of trusting:
//!
//! * `id` must be a plain identifier. It becomes a directory name and a
//!   storage key, so `../` in it was a path-traversal primitive.
//! * `main` must be a relative path with no `..`. It is joined to the plugin
//!   directory and read; the previous code would happily read
//!   `../../../../etc/passwd` as "plugin source".
//! * version compatibility compares versions numerically. The previous code
//!   compared them as strings, which makes "0.9.0" newer than "0.13.0".
//!
//! A manifest that fails any of these is rejected outright. Failing closed on
//! a plugin is cheap; loading a hostile one is not.

#![allow(dead_code)] // shared by both shells; neither uses every accessor.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::error::{PluginError, PluginResult};

/// Longest accepted plugin id. Long enough for reverse-DNS ids, short enough
/// that it cannot be used to blow up a path.
pub const MAX_ID_LEN: usize = 128;

/// Plugin manifest: metadata and declared requirements.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    /// Unique identifier, e.g. "com.example.myplugin".
    pub id: String,
    pub name: String,
    /// Semver-ish, e.g. "1.0.0".
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub author: PluginAuthor,
    pub category: PluginCategory,
    /// Entry point, relative to the plugin directory.
    pub main: String,
    pub plugin_type: PluginType,
    #[serde(default)]
    pub permissions: Vec<PluginPermission>,
    #[serde(default)]
    pub min_version: String,
    #[serde(default)]
    pub max_version: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub dependencies: Option<HashMap<String, String>>,
    /// JSON-schema-like description of the plugin's user settings. Rendered
    /// by the host as a form; see `super::ui::config_schema_panel`.
    #[serde(default)]
    pub config_schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginAuthor {
    pub name: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum PluginCategory {
    Driver,
    Theme,
    Tool,
    Export,
    Import,
    Formatter,
    Analyzer,
    Visualizer,
    Extension,
}

impl PluginCategory {
    pub const ALL: [PluginCategory; 9] = [
        PluginCategory::Driver,
        PluginCategory::Theme,
        PluginCategory::Tool,
        PluginCategory::Export,
        PluginCategory::Import,
        PluginCategory::Formatter,
        PluginCategory::Analyzer,
        PluginCategory::Visualizer,
        PluginCategory::Extension,
    ];

    /// Wire value, matching the manifest JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            PluginCategory::Driver => "driver",
            PluginCategory::Theme => "theme",
            PluginCategory::Tool => "tool",
            PluginCategory::Export => "export",
            PluginCategory::Import => "import",
            PluginCategory::Formatter => "formatter",
            PluginCategory::Analyzer => "analyzer",
            PluginCategory::Visualizer => "visualizer",
            PluginCategory::Extension => "extension",
        }
    }

    /// Marketplace tab label, matching the React `categoryLabels`.
    pub fn label(self) -> &'static str {
        match self {
            PluginCategory::Driver => "Database Drivers",
            PluginCategory::Theme => "Themes",
            PluginCategory::Tool => "Tools",
            PluginCategory::Export => "Export Formats",
            PluginCategory::Import => "Import Formats",
            PluginCategory::Formatter => "Formatters",
            PluginCategory::Analyzer => "Analyzers",
            PluginCategory::Visualizer => "Visualizers",
            PluginCategory::Extension => "Extensions",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PluginType {
    JavaScript,
    WebAssembly,
}

impl PluginType {
    pub fn label(self) -> &'static str {
        match self {
            PluginType::JavaScript => "JavaScript",
            PluginType::WebAssembly => "WebAssembly",
        }
    }
}

/// A capability a plugin declares in its manifest.
///
/// An unknown permission string fails the whole manifest parse on purpose: a
/// permission the host does not understand cannot be shown to the user, and
/// silently dropping it would let a plugin describe itself as needing less
/// than it does.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum PluginPermission {
    ReadFiles,
    WriteFiles,
    ExecuteQuery,
    ModifySchema,
    ReadMetadata,
    CreateTab,
    /// Spelled `modifyUI` on the wire, not `modifyUi`: that is what every
    /// shipped `manifest.json` and the web shell's permission table say, and
    /// renaming it would make existing plugins fail to load.
    #[serde(rename = "modifyUI")]
    ModifyUi,
    ShowNotification,
    NetworkAccess,
    RunCommand,
    AccessClipboard,
    AccessOtherPlugins,
}

impl PluginPermission {
    pub const ALL: [PluginPermission; 12] = [
        PluginPermission::ReadFiles,
        PluginPermission::WriteFiles,
        PluginPermission::ExecuteQuery,
        PluginPermission::ModifySchema,
        PluginPermission::ReadMetadata,
        PluginPermission::CreateTab,
        PluginPermission::ModifyUi,
        PluginPermission::ShowNotification,
        PluginPermission::NetworkAccess,
        PluginPermission::RunCommand,
        PluginPermission::AccessClipboard,
        PluginPermission::AccessOtherPlugins,
    ];

    /// Wire value, matching the manifest JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            PluginPermission::ReadFiles => "readFiles",
            PluginPermission::WriteFiles => "writeFiles",
            PluginPermission::ExecuteQuery => "executeQuery",
            PluginPermission::ModifySchema => "modifySchema",
            PluginPermission::ReadMetadata => "readMetadata",
            PluginPermission::CreateTab => "createTab",
            PluginPermission::ModifyUi => "modifyUI",
            PluginPermission::ShowNotification => "showNotification",
            PluginPermission::NetworkAccess => "networkAccess",
            PluginPermission::RunCommand => "runCommand",
            PluginPermission::AccessClipboard => "accessClipboard",
            PluginPermission::AccessOtherPlugins => "accessOtherPlugins",
        }
    }

    /// The 12 human-readable labels the React shell showed.
    pub fn label(self) -> &'static str {
        match self {
            PluginPermission::ReadFiles => "Read Files",
            PluginPermission::WriteFiles => "Write Files",
            PluginPermission::ExecuteQuery => "Execute Queries",
            PluginPermission::ModifySchema => "Modify Schema",
            PluginPermission::ReadMetadata => "Read Metadata",
            PluginPermission::CreateTab => "Create Tabs",
            PluginPermission::ModifyUi => "Modify UI",
            PluginPermission::ShowNotification => "Show Notifications",
            PluginPermission::NetworkAccess => "Network Access",
            PluginPermission::RunCommand => "Run Commands",
            PluginPermission::AccessClipboard => "Access Clipboard",
            PluginPermission::AccessOtherPlugins => "Access Other Plugins",
        }
    }

    /// What granting it actually allows, for the permissions panel. Says
    /// plainly where the host refuses regardless of the manifest, so the user
    /// is not warned about a capability that does not exist.
    pub fn detail(self) -> &'static str {
        match self {
            PluginPermission::ReadFiles => {
                "Read files inside this plugin's own data folder. Nowhere else."
            }
            PluginPermission::WriteFiles => {
                "Write files inside this plugin's own data folder. Nowhere else."
            }
            PluginPermission::ExecuteQuery => {
                "Declared only. This build never gives plugins a database connection."
            }
            PluginPermission::ModifySchema => {
                "Declared only. This build never gives plugins a database connection."
            }
            PluginPermission::ReadMetadata => {
                "Declared only. This build never gives plugins a database connection."
            }
            PluginPermission::CreateTab => "Ask the host to open a plugin view.",
            PluginPermission::ModifyUi => {
                "Describe interface elements as data for the host to draw."
            }
            PluginPermission::ShowNotification => "Raise a toast notification.",
            PluginPermission::NetworkAccess => {
                "Make http/https requests to public hosts. Private and loopback addresses are refused."
            }
            PluginPermission::RunCommand => {
                "Declared only. This build never runs a subprocess for a plugin."
            }
            PluginPermission::AccessClipboard => "Read and write the system clipboard.",
            PluginPermission::AccessOtherPlugins => "Send messages to other installed plugins.",
        }
    }

    /// Permissions worth calling out before installing.
    pub fn is_sensitive(self) -> bool {
        matches!(
            self,
            PluginPermission::WriteFiles
                | PluginPermission::NetworkAccess
                | PluginPermission::RunCommand
                | PluginPermission::AccessClipboard
                | PluginPermission::ModifySchema
                | PluginPermission::ExecuteQuery
        )
    }
}

/// Comma-separated labels, matching the React `formatPermissions`.
pub fn permission_summary(permissions: &[PluginPermission]) -> String {
    if permissions.is_empty() {
        return "No special permissions required".to_string();
    }
    permissions
        .iter()
        .map(|p| p.label())
        .collect::<Vec<_>>()
        .join(", ")
}

/// An installed plugin: its manifest plus everything the host tracks about it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Plugin {
    pub manifest: PluginManifest,
    pub path: PathBuf,
    pub enabled: bool,
    pub loaded: bool,
    pub config: Option<serde_json::Value>,
    pub stats: PluginStats,
}

impl Plugin {
    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    pub fn has_permission(&self, permission: PluginPermission) -> bool {
        self.manifest.permissions.contains(&permission)
    }

    /// Enable or disable.
    ///
    /// Disabling also clears `loaded`: the React shell unloaded the runtime
    /// before disabling, and a plugin that is disabled but still marked
    /// loaded would keep its registered UI on screen.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.loaded = false;
        }
    }

    /// A disabled plugin is never executed, whatever its manifest says.
    pub fn may_execute(&self) -> bool {
        self.enabled && self.manifest.plugin_type == PluginType::JavaScript
    }

    /// Entry-point path, guaranteed to stay inside the plugin directory.
    pub fn main_path(&self) -> PluginResult<PathBuf> {
        if !is_safe_relative_path(&self.manifest.main) {
            return Err(PluginError::InvalidManifest(format!(
                "entry point '{}' escapes the plugin directory",
                self.manifest.main
            )));
        }
        Ok(self.path.join(&self.manifest.main))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginStats {
    pub install_date: String,
    #[serde(default)]
    pub last_used: Option<String>,
    #[serde(default)]
    pub execution_count: u64,
    #[serde(default)]
    pub error_count: u64,
    #[serde(default)]
    pub rating: Option<f32>,
    #[serde(default)]
    pub downloads: u64,
}

impl PluginStats {
    pub fn new(install_date: String) -> Self {
        Self {
            install_date,
            last_used: None,
            execution_count: 0,
            error_count: 0,
            rating: None,
            downloads: 1,
        }
    }
}

/// The `.state.json` a plugin directory carries: what the user chose, kept
/// apart from the manifest so an update to the plugin cannot re-enable it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginState {
    pub enabled: bool,
    #[serde(default)]
    pub loaded: bool,
}

impl Default for PluginState {
    fn default() -> Self {
        // A freshly installed plugin with no state file is enabled: the user
        // installing it is the consent. Corrupt or absent state falls back
        // here rather than erroring the whole scan.
        Self {
            enabled: true,
            loaded: false,
        }
    }
}

impl PluginState {
    /// Parse `.state.json`. Anything unreadable is treated as the default
    /// rather than as an error: a truncated state file must not make an
    /// installed plugin disappear from the list.
    ///
    /// `loaded` is never restored from disk. Nothing is loaded until this
    /// process loads it.
    pub fn parse(raw: &str) -> Self {
        match serde_json::from_str::<PluginState>(raw) {
            Ok(mut s) => {
                s.loaded = false;
                s
            }
            Err(_) => PluginState::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// A plugin id is used as a directory name and a storage key. Restrict it to
/// characters that cannot mean anything to a filesystem.
pub fn is_valid_plugin_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id != "."
        && id != ".."
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_')
        && !id.contains("..")
}

/// A path a plugin supplies that the host will join to a directory it owns.
pub fn is_safe_relative_path(path: &str) -> bool {
    if path.is_empty() || path.len() > 512 {
        return false;
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return false;
    }
    // Windows drive letters and UNC paths.
    if path.chars().nth(1) == Some(':') {
        return false;
    }
    let p = std::path::Path::new(path);
    p.components().all(|c| {
        matches!(
            c,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )
    })
}

impl PluginManifest {
    /// Parse and validate a `manifest.json`.
    pub fn parse(raw: &str) -> PluginResult<Self> {
        // A manifest is metadata; anything this big is not one.
        const MAX_MANIFEST_BYTES: usize = 256 * 1024;
        if raw.len() > MAX_MANIFEST_BYTES {
            return Err(PluginError::InvalidManifest(format!(
                "manifest is {} bytes, limit is {}",
                raw.len(),
                MAX_MANIFEST_BYTES
            )));
        }
        let manifest: PluginManifest =
            serde_json::from_str(raw).map_err(|e| PluginError::InvalidManifest(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> PluginResult<()> {
        if !is_valid_plugin_id(&self.id) {
            return Err(PluginError::InvalidManifest(format!(
                "'{}' is not a valid plugin id (letters, digits, '.', '-' and '_' only)",
                self.id
            )));
        }
        if self.name.trim().is_empty() {
            return Err(PluginError::InvalidManifest("name is empty".into()));
        }
        if self.version.trim().is_empty() {
            return Err(PluginError::InvalidManifest("version is empty".into()));
        }
        if !is_safe_relative_path(&self.main) {
            return Err(PluginError::InvalidManifest(format!(
                "entry point '{}' must be a relative path inside the plugin directory",
                self.main
            )));
        }
        if self.permissions.len() > PluginPermission::ALL.len() {
            return Err(PluginError::InvalidManifest(
                "permissions repeated in manifest".into(),
            ));
        }
        Ok(())
    }

    /// Sorted, de-duplicated permissions, for display.
    pub fn declared_permissions(&self) -> Vec<PluginPermission> {
        let mut v = self.permissions.clone();
        v.sort();
        v.dedup();
        v
    }

    /// Is this plugin compatible with `app_version`?
    pub fn check_compatibility(&self, app_version: &str) -> PluginResult<()> {
        if !self.min_version.trim().is_empty()
            && compare_versions(&self.min_version, app_version) == Ordering::Greater
        {
            return Err(PluginError::IncompatibleVersion {
                required: self.min_version.clone(),
                current: app_version.to_string(),
            });
        }
        if let Some(max) = &self.max_version {
            if !max.trim().is_empty() && compare_versions(app_version, max) == Ordering::Greater {
                return Err(PluginError::IncompatibleVersion {
                    required: format!("<= {}", max),
                    current: app_version.to_string(),
                });
            }
        }
        Ok(())
    }
}

/// Compare two dotted version strings numerically.
///
/// The Tauri implementation compared them as strings, which ordered "0.9.0"
/// after "0.13.0" and refused compatible plugins. Missing components count as
/// zero, so "1.0" == "1.0.0". Non-numeric components (pre-release tags)
/// compare as zero and then lexically, which is not full semver but is
/// deterministic and never panics.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let parse = |s: &str| -> Vec<(u64, String)> {
        s.trim()
            .trim_start_matches('v')
            .split('.')
            .map(|part| {
                let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
                let rest: String = part.chars().skip_while(|c| c.is_ascii_digit()).collect();
                (digits.parse::<u64>().unwrap_or(0), rest)
            })
            .collect()
    };
    let (va, vb) = (parse(a), parse(b));
    let len = va.len().max(vb.len());
    let zero = (0u64, String::new());
    for i in 0..len {
        let x = va.get(i).unwrap_or(&zero);
        let y = vb.get(i).unwrap_or(&zero);
        match x.0.cmp(&y.0) {
            Ordering::Equal => {}
            other => return other,
        }
        match x.1.cmp(&y.1) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bundled csv-exporter manifest, inlined. Tests never read plugin
    /// code or manifests off disk: fixtures live here where they cannot be
    /// swapped under the test.
    const CSV_EXPORTER: &str = r#"{
      "id": "com.dbhive.csv-exporter",
      "name": "Advanced CSV Exporter",
      "version": "1.0.0",
      "description": "Export query results to CSV",
      "author": { "name": "DB Hive Team", "email": "plugins@dbhive.com" },
      "category": "export",
      "main": "index.js",
      "pluginType": "javascript",
      "permissions": ["executeQuery", "writeFiles", "showNotification", "createTab"],
      "minVersion": "0.13.0",
      "icon": "icon.svg",
      "homepage": "https://github.com/dbhive/csv-exporter",
      "repository": "https://github.com/dbhive/csv-exporter",
      "license": "MIT",
      "keywords": ["csv", "export"],
      "configSchema": {
        "type": "object",
        "properties": { "delimiter": { "type": "string", "default": "," } }
      }
    }"#;

    fn manifest() -> PluginManifest {
        PluginManifest::parse(CSV_EXPORTER).expect("fixture should parse")
    }

    #[test]
    fn parses_the_shipped_manifest_format() {
        let m = manifest();
        assert_eq!(m.id, "com.dbhive.csv-exporter");
        assert_eq!(m.name, "Advanced CSV Exporter");
        assert_eq!(m.category, PluginCategory::Export);
        assert_eq!(m.plugin_type, PluginType::JavaScript);
        assert_eq!(m.main, "index.js");
        assert!(m.config_schema.is_some());
        assert_eq!(
            m.declared_permissions(),
            vec![
                PluginPermission::WriteFiles,
                PluginPermission::ExecuteQuery,
                PluginPermission::CreateTab,
                PluginPermission::ShowNotification,
            ]
        );
    }

    #[test]
    fn optional_fields_may_be_absent() {
        let raw = r#"{
          "id": "a.b",
          "name": "Minimal",
          "version": "1.0.0",
          "author": { "name": "n" },
          "category": "tool",
          "main": "main.js",
          "pluginType": "javascript"
        }"#;
        let m = PluginManifest::parse(raw).unwrap();
        assert!(m.permissions.is_empty());
        assert!(m.keywords.is_empty());
        assert_eq!(m.max_version, None);
        assert_eq!(m.description, "");
    }

    #[test]
    fn missing_required_field_is_rejected() {
        let raw = r#"{ "name": "No id", "version": "1.0.0" }"#;
        assert!(matches!(
            PluginManifest::parse(raw),
            Err(PluginError::InvalidManifest(_))
        ));
    }

    #[test]
    fn unknown_permission_fails_the_whole_manifest() {
        // Fail closed: a permission the host cannot name cannot be shown to
        // the user, so the plugin does not load at all.
        let raw = CSV_EXPORTER.replace("\"writeFiles\"", "\"rootAccess\"");
        assert!(matches!(
            PluginManifest::parse(&raw),
            Err(PluginError::InvalidManifest(_))
        ));
    }

    #[test]
    fn hostile_ids_are_rejected() {
        for bad in [
            "../../etc",
            "..",
            ".",
            "",
            "a/b",
            "a\\b",
            "a b",
            "com.x/../../y",
            "a:b",
        ] {
            assert!(!is_valid_plugin_id(bad), "{} should be rejected", bad);
        }
        for good in ["com.dbhive.csv-exporter", "a", "A_b-1.2"] {
            assert!(is_valid_plugin_id(good), "{} should be accepted", good);
        }
        assert!(!is_valid_plugin_id(&"a".repeat(MAX_ID_LEN + 1)));
    }

    #[test]
    fn entry_point_cannot_escape_the_plugin_directory() {
        for bad in [
            "../../../../etc/passwd",
            "/etc/passwd",
            "C:\\windows\\system32",
            "sub/../../out.js",
            "",
        ] {
            assert!(!is_safe_relative_path(bad), "{} should be rejected", bad);
        }
        for good in ["index.js", "./index.js", "dist/index.js"] {
            assert!(is_safe_relative_path(good), "{} should be accepted", good);
        }

        let raw = CSV_EXPORTER.replace("\"index.js\"", "\"../../../../etc/passwd\"");
        assert!(matches!(
            PluginManifest::parse(&raw),
            Err(PluginError::InvalidManifest(_))
        ));
    }

    #[test]
    fn oversized_manifest_is_refused_before_parsing() {
        let raw = format!(r#"{{"id":"a","pad":"{}"}}"#, "x".repeat(300 * 1024));
        assert!(matches!(
            PluginManifest::parse(&raw),
            Err(PluginError::InvalidManifest(_))
        ));
    }

    #[test]
    fn deeply_nested_manifest_json_errors_rather_than_overflowing() {
        // serde_json has a recursion limit; the point is that this returns an
        // error instead of taking the process down with it.
        let raw = format!("{}{}", "[".repeat(4000), "]".repeat(4000));
        assert!(PluginManifest::parse(&raw).is_err());
    }

    #[test]
    fn versions_compare_numerically_not_lexically() {
        assert_eq!(compare_versions("0.13.0", "0.9.0"), Ordering::Greater);
        assert_eq!(compare_versions("0.9.0", "0.13.0"), Ordering::Less);
        assert_eq!(compare_versions("1.0", "1.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("v1.2.3", "1.2.3"), Ordering::Equal);
        assert_eq!(compare_versions("2.0.0", "10.0.0"), Ordering::Less);
        assert_eq!(compare_versions("", ""), Ordering::Equal);
        // Non-numeric components are deterministic rather than meaningful:
        // the contract is only that they never panic and never compare
        // unequal to themselves.
        assert_eq!(
            compare_versions("not.a.version", "not.a.version"),
            Ordering::Equal
        );
        assert_eq!(compare_versions("1.0.0-beta", "1.0.0"), Ordering::Greater);
    }

    #[test]
    fn compatibility_uses_the_numeric_comparison() {
        let mut m = manifest();
        // minVersion 0.13.0 against app 0.21.0: fine. Lexically "0.13.0" >
        // "0.21.0" is false too, so use a case the old code got wrong.
        assert!(m.check_compatibility("0.21.0").is_ok());

        m.min_version = "0.9.0".into();
        assert!(
            m.check_compatibility("0.21.0").is_ok(),
            "0.21.0 satisfies a 0.9.0 minimum"
        );

        m.min_version = "1.0.0".into();
        assert!(matches!(
            m.check_compatibility("0.21.0"),
            Err(PluginError::IncompatibleVersion { .. })
        ));

        m.min_version = "0.1.0".into();
        m.max_version = Some("0.20.0".into());
        assert!(matches!(
            m.check_compatibility("0.21.0"),
            Err(PluginError::IncompatibleVersion { .. })
        ));
        m.max_version = Some("0.21.0".into());
        assert!(m.check_compatibility("0.21.0").is_ok());
    }

    #[test]
    fn disabling_also_unloads() {
        let mut p = Plugin {
            manifest: manifest(),
            path: PathBuf::from("/tmp/x"),
            enabled: true,
            loaded: true,
            config: None,
            stats: PluginStats::new("2026-01-01T00:00:00Z".into()),
        };
        assert!(p.may_execute());

        p.set_enabled(false);
        assert!(!p.enabled);
        assert!(!p.loaded, "disabling must unload the runtime");
        assert!(!p.may_execute(), "a disabled plugin never executes");

        p.set_enabled(true);
        assert!(p.enabled);
        assert!(!p.loaded, "enabling does not load by itself");
        assert!(p.may_execute());
    }

    #[test]
    fn wasm_plugins_never_execute() {
        let mut m = manifest();
        m.plugin_type = PluginType::WebAssembly;
        let p = Plugin {
            manifest: m,
            path: PathBuf::from("/tmp/x"),
            enabled: true,
            loaded: false,
            config: None,
            stats: PluginStats::new("2026-01-01T00:00:00Z".into()),
        };
        assert!(!p.may_execute());
    }

    #[test]
    fn state_file_parsing_fails_safe() {
        assert_eq!(
            PluginState::parse(r#"{"enabled": false, "loaded": true}"#),
            PluginState {
                enabled: false,
                loaded: false
            },
            "loaded is never restored from disk"
        );
        assert_eq!(
            PluginState::parse(r#"{"enabled": true}"#),
            PluginState {
                enabled: true,
                loaded: false
            }
        );
        assert_eq!(
            PluginState::parse("{ truncated"),
            PluginState::default(),
            "a corrupt state file must not hide the plugin"
        );
        assert!(PluginState::default().enabled);
    }

    #[test]
    fn permission_wire_names_match_the_shipped_manifests() {
        // `as_str` and the serde name must agree, or a permission would parse
        // under one spelling and be written back under another. `modifyUI` in
        // particular is not what rename_all = "camelCase" would produce.
        for p in PluginPermission::ALL {
            let encoded = serde_json::to_string(&p).unwrap();
            assert_eq!(
                encoded,
                format!("\"{}\"", p.as_str()),
                "{:?} encodes differently from as_str()",
                p
            );
            let decoded: PluginPermission = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, p);
        }
        assert_eq!(
            serde_json::from_str::<PluginPermission>("\"modifyUI\"").unwrap(),
            PluginPermission::ModifyUi
        );
    }

    #[test]
    fn permission_labels_cover_every_variant() {
        for p in PluginPermission::ALL {
            assert!(!p.label().is_empty());
            assert!(!p.detail().is_empty());
            assert!(!p.as_str().is_empty());
        }
        assert_eq!(
            permission_summary(&[PluginPermission::ReadFiles, PluginPermission::NetworkAccess]),
            "Read Files, Network Access"
        );
        assert_eq!(
            permission_summary(&[]),
            "No special permissions required"
        );
    }

    #[test]
    fn main_path_is_checked_at_use_not_only_at_parse() {
        let mut p = Plugin {
            manifest: manifest(),
            path: PathBuf::from("/plugins/x"),
            enabled: true,
            loaded: false,
            config: None,
            stats: PluginStats::new("2026-01-01T00:00:00Z".into()),
        };
        assert_eq!(p.main_path().unwrap(), PathBuf::from("/plugins/x/index.js"));

        // Even if a manifest reached us without going through parse().
        p.manifest.main = "../../../etc/passwd".into();
        assert!(p.main_path().is_err());
    }
}
