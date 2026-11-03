//! The plugin directory on disk.
//!
//! One directory per plugin, exactly the layout the Tauri shell used, so an
//! existing installation is picked up unchanged:
//!
//! ```text
//! <app data>/plugins/<plugin id>/manifest.json   the plugin's own metadata
//!                               /<main>          its code
//!                               /config.json     user settings
//!                               /.state.json     enabled/disabled
//!                               /.stats.json     execution and error counts
//! <app data>/plugin-data/<plugin id>/            the only place it may write
//! ```
//!
//! Everything here is synchronous `std::fs` and no more than a few small
//! files per plugin, which keeps it usable from anywhere. The async manager
//! wraps these calls in `spawn_blocking`; a UI must do the same rather than
//! calling them on a frame.
//!
//! Two things this deliberately does not do: it never executes anything, and
//! it never trusts a path that came out of a manifest. Scanning a directory
//! full of hostile plugins is safe.

#![allow(dead_code)] // shared by both shells; neither uses every accessor.

use std::fs;
use std::path::{Path, PathBuf};

use super::error::{PluginError, PluginResult};
use super::manifest::{is_valid_plugin_id, Plugin, PluginManifest, PluginState, PluginStats};

/// Caps on an install, so a "plugin" cannot be a decompression bomb or a
/// directory tree deep enough to exhaust the stack while copying.
pub const MAX_INSTALL_FILES: usize = 512;
pub const MAX_INSTALL_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_INSTALL_DEPTH: usize = 8;

/// What one scan of the plugins directory found.
///
/// `problems` exists because a single unreadable plugin must not hide the
/// others: the scan reports what it could not load and carries on, and the UI
/// shows that rather than silently listing fewer plugins.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ScanResult {
    pub plugins: Vec<Plugin>,
    pub problems: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PluginStore {
    root: PathBuf,
    data_root: PathBuf,
}

impl PluginStore {
    /// `root` holds installed plugins, `data_root` their writable data.
    pub fn new(root: PathBuf, data_root: PathBuf) -> Self {
        Self { root, data_root }
    }

    /// The conventional layout under an application data directory.
    pub fn under_app_dir(app_data_dir: &Path) -> Self {
        Self::new(
            app_data_dir.join("plugins"),
            app_data_dir.join("plugin-data"),
        )
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    pub fn ensure_dirs(&self) -> PluginResult<()> {
        fs::create_dir_all(&self.root)?;
        fs::create_dir_all(&self.data_root)?;
        Ok(())
    }

    /// Directory for a plugin id, refusing anything that is not a plain id.
    /// This is the only place an id becomes a path.
    pub fn plugin_dir(&self, id: &str) -> PluginResult<PathBuf> {
        if !is_valid_plugin_id(id) {
            return Err(PluginError::InvalidManifest(format!(
                "'{}' is not a valid plugin id",
                id
            )));
        }
        Ok(self.root.join(id))
    }

    /// Writable data directory for a plugin, created on demand.
    pub fn data_dir(&self, id: &str) -> PluginResult<PathBuf> {
        if !is_valid_plugin_id(id) {
            return Err(PluginError::InvalidManifest(format!(
                "'{}' is not a valid plugin id",
                id
            )));
        }
        let dir = self.data_root.join(id);
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Load every plugin in the root directory.
    pub fn scan(&self, app_version: &str) -> ScanResult {
        let mut out = ScanResult::default();

        let entries = match fs::read_dir(&self.root) {
            Ok(e) => e,
            // A missing directory is a first run, not a failure.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return out,
            Err(e) => {
                out.problems.push(format!("{}: {}", self.root.display(), e));
                return out;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            match self.load_dir(&path, app_version) {
                Ok(p) => out.plugins.push(p),
                Err(e) => out.problems.push(format!(
                    "{}: {}",
                    path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string()),
                    e
                )),
            }
        }

        out.plugins.sort_by(|a, b| {
            a.manifest
                .name
                .to_lowercase()
                .cmp(&b.manifest.name.to_lowercase())
        });
        out
    }

    /// Load one plugin directory: manifest, config, state, stats.
    pub fn load_dir(&self, dir: &Path, app_version: &str) -> PluginResult<Plugin> {
        let raw = fs::read_to_string(dir.join("manifest.json"))?;
        let manifest = PluginManifest::parse(&raw)?;

        // The directory name is what the id has to be, or an update could
        // shadow another plugin.
        if let Some(name) = dir.file_name().and_then(|n| n.to_str()) {
            if name != manifest.id {
                return Err(PluginError::InvalidManifest(format!(
                    "manifest id '{}' does not match its directory '{}'",
                    manifest.id, name
                )));
            }
        }

        manifest.check_compatibility(app_version)?;

        let config = read_json(&dir.join("config.json"));
        let state = match fs::read_to_string(dir.join(".state.json")) {
            Ok(raw) => PluginState::parse(&raw),
            Err(_) => PluginState::default(),
        };
        let stats = read_json(&dir.join(".stats.json"))
            .and_then(|v| serde_json::from_value::<PluginStats>(v).ok())
            .unwrap_or_else(|| PluginStats::new(now_rfc3339()));

        Ok(Plugin {
            manifest,
            path: dir.to_path_buf(),
            enabled: state.enabled,
            loaded: false,
            config,
            stats,
        })
    }

    /// Enable or disable, persisting the choice.
    ///
    /// Disabling clears `loaded` (see `Plugin::set_enabled`); the caller is
    /// responsible for actually dropping the runtime.
    pub fn set_enabled(&self, plugin: &mut Plugin, enabled: bool) -> PluginResult<()> {
        plugin.set_enabled(enabled);
        self.write_state(plugin)
    }

    pub fn write_state(&self, plugin: &Plugin) -> PluginResult<()> {
        self.guard_inside_root(&plugin.path)?;
        let state = PluginState {
            enabled: plugin.enabled,
            loaded: plugin.loaded,
        };
        write_json(&plugin.path.join(".state.json"), &state)
    }

    pub fn save_config(&self, plugin: &mut Plugin, config: serde_json::Value) -> PluginResult<()> {
        self.guard_inside_root(&plugin.path)?;
        write_json(&plugin.path.join("config.json"), &config)?;
        plugin.config = Some(config);
        Ok(())
    }

    /// Record one execution. Errors are counted separately so the detail
    /// panel can show a plugin that runs but keeps failing.
    pub fn record_run(&self, plugin: &mut Plugin, ok: bool) -> PluginResult<()> {
        self.guard_inside_root(&plugin.path)?;
        plugin.stats.execution_count = plugin.stats.execution_count.saturating_add(1);
        if !ok {
            plugin.stats.error_count = plugin.stats.error_count.saturating_add(1);
        }
        plugin.stats.last_used = Some(now_rfc3339());
        write_json(&plugin.path.join(".stats.json"), &plugin.stats)
    }

    /// Delete a plugin directory and its data directory.
    pub fn uninstall(&self, plugin: &Plugin) -> PluginResult<()> {
        // Never remove_dir_all a path that did not come from this store.
        self.guard_inside_root(&plugin.path)?;
        fs::remove_dir_all(&plugin.path)?;

        // Best effort: a plugin with no data directory is not an error.
        if is_valid_plugin_id(&plugin.manifest.id) {
            let _ = fs::remove_dir_all(self.data_root.join(&plugin.manifest.id));
        }
        Ok(())
    }

    /// Copy a local plugin source directory into the store.
    ///
    /// Used for the plugins bundled with the application. Symlinks are
    /// skipped rather than followed: a link in the source would otherwise
    /// copy a file from outside it into a directory the plugin can read.
    pub fn install_from_dir(&self, id: &str, source: &Path) -> PluginResult<PathBuf> {
        let dest = self.plugin_dir(id)?;
        if dest.exists() {
            return Err(PluginError::AlreadyInstalled(id.to_string()));
        }
        if !source.join("manifest.json").is_file() {
            return Err(PluginError::InvalidManifest(format!(
                "{} has no manifest.json",
                source.display()
            )));
        }

        fs::create_dir_all(&dest)?;
        let mut budget = Budget::default();
        if let Err(e) = copy_tree(source, &dest, 0, &mut budget) {
            // Do not leave a half-copied plugin behind.
            let _ = fs::remove_dir_all(&dest);
            return Err(e);
        }
        Ok(dest)
    }

    /// Write a manifest and an entry point for a plugin with no bundled
    /// source. The Tauri shell did this so that "install" produced something
    /// inspectable rather than nothing at all.
    pub fn install_placeholder(
        &self,
        manifest: &PluginManifest,
        note: &str,
    ) -> PluginResult<PathBuf> {
        manifest.validate()?;
        let dest = self.plugin_dir(&manifest.id)?;
        if dest.exists() {
            return Err(PluginError::AlreadyInstalled(manifest.id.clone()));
        }
        fs::create_dir_all(&dest)?;
        write_json(&dest.join("manifest.json"), manifest)?;
        fs::write(
            dest.join(&manifest.main),
            placeholder_source(manifest, note),
        )?;
        Ok(dest)
    }

    /// Refuse to touch anything outside the store, whatever produced the path.
    fn guard_inside_root(&self, path: &Path) -> PluginResult<()> {
        if path.starts_with(&self.root) && path != self.root {
            Ok(())
        } else {
            Err(PluginError::Other(format!(
                "{} is not inside the plugin directory",
                path.display()
            )))
        }
    }
}

/// Source for a placeholder install: valid JavaScript that does nothing and
/// says so, including through the declarative UI.
fn placeholder_source(manifest: &PluginManifest, note: &str) -> String {
    format!(
        "// {name} - placeholder. {note}\n\
         __plugin_exports__ = {{\n\
         \x20 onLoad: function () {{ return {{ success: true, message: 'placeholder' }}; }},\n\
         \x20 onUnload: function () {{ return {{ success: true }}; }},\n\
         \x20 renderUi: function () {{\n\
         \x20   return {{ panels: [{{ title: '{name}', items: [\n\
         \x20     {{ type: 'text', style: 'warning', text: 'This plugin was installed without its code.' }}\n\
         \x20   ] }}] }};\n\
         \x20 }}\n\
         }};\n",
        name = manifest.name.replace('\'', ""),
        note = note.replace('\n', " ")
    )
}

#[derive(Default)]
struct Budget {
    files: usize,
    bytes: u64,
}

fn copy_tree(src: &Path, dest: &Path, depth: usize, budget: &mut Budget) -> PluginResult<()> {
    if depth > MAX_INSTALL_DEPTH {
        return Err(PluginError::Other(format!(
            "plugin source nests deeper than {} directories",
            MAX_INSTALL_DEPTH
        )));
    }

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();

        // symlink_metadata does not follow the link, which is the point.
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            continue;
        }

        let target = dest.join(entry.file_name());
        if meta.is_dir() {
            fs::create_dir_all(&target)?;
            copy_tree(&path, &target, depth + 1, budget)?;
        } else if meta.is_file() {
            budget.files += 1;
            budget.bytes = budget.bytes.saturating_add(meta.len());
            if budget.files > MAX_INSTALL_FILES {
                return Err(PluginError::Other(format!(
                    "plugin source has more than {} files",
                    MAX_INSTALL_FILES
                )));
            }
            if budget.bytes > MAX_INSTALL_BYTES {
                return Err(PluginError::Other(format!(
                    "plugin source is larger than {} bytes",
                    MAX_INSTALL_BYTES
                )));
            }
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> PluginResult<()> {
    let body = serde_json::to_string_pretty(value)?;
    fs::write(path, body)?;
    Ok(())
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixtures are written by the test itself. Nothing here reads a plugin
    /// that was already on the machine, and nothing executes any of it.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "dbhive-plugin-store-{}-{}-{:?}",
                std::process::id(),
                tag,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .subsec_nanos()
            ));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn manifest_json(id: &str, name: &str) -> String {
        format!(
            r#"{{
              "id": "{id}",
              "name": "{name}",
              "version": "1.0.0",
              "author": {{ "name": "T" }},
              "category": "tool",
              "main": "index.js",
              "pluginType": "javascript",
              "permissions": ["showNotification"],
              "minVersion": "0.1.0",
              "license": "MIT"
            }}"#
        )
    }

    fn install(store: &PluginStore, id: &str, name: &str) -> PathBuf {
        let dir = store.plugin_dir(id).unwrap();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manifest.json"), manifest_json(id, name)).unwrap();
        fs::write(dir.join("index.js"), "// not executed by any test\n").unwrap();
        dir
    }

    fn store_in(tmp: &TempDir) -> PluginStore {
        let s = PluginStore::under_app_dir(tmp.path());
        s.ensure_dirs().unwrap();
        s
    }

    #[test]
    fn a_missing_plugins_directory_is_an_empty_scan_not_an_error() {
        let tmp = TempDir::new("missing");
        let store = PluginStore::under_app_dir(&tmp.path().join("nothing-here"));
        let scan = store.scan("0.21.0");
        assert!(scan.plugins.is_empty());
        assert!(scan.problems.is_empty());
    }

    #[test]
    fn scan_loads_plugins_sorted_by_name() {
        let tmp = TempDir::new("scan");
        let store = store_in(&tmp);
        install(&store, "com.b", "Beta");
        install(&store, "com.a", "Alpha");

        let scan = store.scan("0.21.0");
        assert!(scan.problems.is_empty(), "{:?}", scan.problems);
        let names: Vec<&str> = scan
            .plugins
            .iter()
            .map(|p| p.manifest.name.as_str())
            .collect();
        assert_eq!(names, vec!["Alpha", "Beta"]);
        assert!(
            scan.plugins.iter().all(|p| p.enabled),
            "new plugins start enabled"
        );
        assert!(
            scan.plugins.iter().all(|p| !p.loaded),
            "nothing is loaded by a scan"
        );
    }

    #[test]
    fn one_broken_plugin_does_not_hide_the_others() {
        let tmp = TempDir::new("broken");
        let store = store_in(&tmp);
        install(&store, "com.good", "Good");

        let bad = store.root().join("com.bad");
        fs::create_dir_all(&bad).unwrap();
        fs::write(bad.join("manifest.json"), "{ truncated").unwrap();

        let missing = store.root().join("com.missing");
        fs::create_dir_all(&missing).unwrap();

        let scan = store.scan("0.21.0");
        assert_eq!(scan.plugins.len(), 1);
        assert_eq!(scan.plugins[0].manifest.id, "com.good");
        assert_eq!(
            scan.problems.len(),
            2,
            "both failures are reported: {:?}",
            scan.problems
        );
        assert!(scan.problems.iter().any(|p| p.starts_with("com.bad")));
    }

    #[test]
    fn a_manifest_that_claims_another_id_is_rejected() {
        let tmp = TempDir::new("idmismatch");
        let store = store_in(&tmp);
        let dir = store.plugin_dir("com.a").unwrap();
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("manifest.json"),
            manifest_json("com.b", "Impostor"),
        )
        .unwrap();

        let scan = store.scan("0.21.0");
        assert!(scan.plugins.is_empty());
        assert!(scan.problems[0].contains("does not match"));
    }

    #[test]
    fn incompatible_plugins_are_reported_not_loaded() {
        let tmp = TempDir::new("incompatible");
        let store = store_in(&tmp);
        let dir = install(&store, "com.future", "Future");
        let raw = manifest_json("com.future", "Future").replace("0.1.0", "9.0.0");
        fs::write(dir.join("manifest.json"), raw).unwrap();

        let scan = store.scan("0.21.0");
        assert!(scan.plugins.is_empty());
        assert!(scan.problems[0].contains("Incompatible version"));
    }

    #[test]
    fn enable_state_survives_a_rescan_and_disabling_unloads() {
        let tmp = TempDir::new("state");
        let store = store_in(&tmp);
        install(&store, "com.a", "Alpha");

        let mut plugin = store.scan("0.21.0").plugins.remove(0);
        plugin.loaded = true;
        store.set_enabled(&mut plugin, false).unwrap();
        assert!(!plugin.loaded, "disabling unloads");

        let rescanned = store.scan("0.21.0").plugins.remove(0);
        assert!(!rescanned.enabled, "the choice persisted");

        let mut p = rescanned;
        store.set_enabled(&mut p, true).unwrap();
        assert!(store.scan("0.21.0").plugins[0].enabled);
    }

    #[test]
    fn config_is_saved_and_reloaded() {
        let tmp = TempDir::new("config");
        let store = store_in(&tmp);
        install(&store, "com.a", "Alpha");

        let mut plugin = store.scan("0.21.0").plugins.remove(0);
        assert!(plugin.config.is_none());
        store
            .save_config(&mut plugin, serde_json::json!({ "delimiter": ";" }))
            .unwrap();

        let reloaded = store.scan("0.21.0").plugins.remove(0);
        assert_eq!(
            reloaded.config,
            Some(serde_json::json!({ "delimiter": ";" }))
        );
    }

    #[test]
    fn execution_counts_are_recorded() {
        let tmp = TempDir::new("stats");
        let store = store_in(&tmp);
        install(&store, "com.a", "Alpha");

        let mut plugin = store.scan("0.21.0").plugins.remove(0);
        store.record_run(&mut plugin, true).unwrap();
        store.record_run(&mut plugin, false).unwrap();

        let reloaded = store.scan("0.21.0").plugins.remove(0);
        assert_eq!(reloaded.stats.execution_count, 2);
        assert_eq!(reloaded.stats.error_count, 1);
        assert!(reloaded.stats.last_used.is_some());
    }

    #[test]
    fn uninstall_removes_the_plugin_and_its_data() {
        let tmp = TempDir::new("uninstall");
        let store = store_in(&tmp);
        install(&store, "com.a", "Alpha");
        let data = store.data_dir("com.a").unwrap();
        fs::write(data.join("scratch.txt"), "x").unwrap();

        let plugin = store.scan("0.21.0").plugins.remove(0);
        store.uninstall(&plugin).unwrap();

        assert!(store.scan("0.21.0").plugins.is_empty());
        assert!(!data.exists());
    }

    #[test]
    fn uninstall_refuses_a_path_outside_the_store() {
        let tmp = TempDir::new("escape");
        let store = store_in(&tmp);
        install(&store, "com.a", "Alpha");
        let mut plugin = store.scan("0.21.0").plugins.remove(0);

        let elsewhere = tmp.path().join("precious");
        fs::create_dir_all(&elsewhere).unwrap();
        plugin.path = elsewhere.clone();

        assert!(store.uninstall(&plugin).is_err());
        assert!(elsewhere.exists(), "nothing outside the store is deleted");
    }

    #[test]
    fn ids_that_are_not_ids_never_become_paths() {
        let tmp = TempDir::new("ids");
        let store = store_in(&tmp);
        for bad in ["../../etc", "a/b", "", ".."] {
            assert!(store.plugin_dir(bad).is_err(), "{:?}", bad);
            assert!(store.data_dir(bad).is_err(), "{:?}", bad);
        }
    }

    #[test]
    fn install_from_dir_copies_the_tree_and_refuses_a_reinstall() {
        let tmp = TempDir::new("install");
        let store = store_in(&tmp);

        let src = tmp.path().join("src");
        fs::create_dir_all(src.join("lib")).unwrap();
        fs::write(src.join("manifest.json"), manifest_json("com.a", "Alpha")).unwrap();
        fs::write(src.join("index.js"), "// inert\n").unwrap();
        fs::write(src.join("lib").join("helper.js"), "// inert\n").unwrap();

        let dest = store.install_from_dir("com.a", &src).unwrap();
        assert!(dest.join("index.js").is_file());
        assert!(dest.join("lib").join("helper.js").is_file());
        assert_eq!(store.scan("0.21.0").plugins.len(), 1);

        assert!(matches!(
            store.install_from_dir("com.a", &src),
            Err(PluginError::AlreadyInstalled(_))
        ));
    }

    #[test]
    fn install_from_dir_needs_a_manifest() {
        let tmp = TempDir::new("nomanifest");
        let store = store_in(&tmp);
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("index.js"), "// inert\n").unwrap();

        assert!(matches!(
            store.install_from_dir("com.a", &src),
            Err(PluginError::InvalidManifest(_))
        ));
        assert!(
            !store.plugin_dir("com.a").unwrap().exists(),
            "a failed install leaves nothing behind"
        );
    }

    #[test]
    fn placeholder_installs_are_valid_and_declare_themselves() {
        let tmp = TempDir::new("placeholder");
        let store = store_in(&tmp);
        let manifest = PluginManifest::parse(&manifest_json("com.a", "Alpha")).unwrap();

        store
            .install_placeholder(&manifest, "no bundled source")
            .unwrap();

        let scan = store.scan("0.21.0");
        assert_eq!(scan.plugins.len(), 1);
        let body = fs::read_to_string(scan.plugins[0].main_path().unwrap()).unwrap();
        assert!(
            body.contains("renderUi"),
            "the placeholder describes itself"
        );
    }
}
