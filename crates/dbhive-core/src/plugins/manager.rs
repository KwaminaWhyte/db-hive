//! Plugin lifecycle: what is installed, what is enabled, what is loaded.
//!
//! The Tauri version of this file held an `AppHandle`, resolved its own
//! directories from it, and announced every state change with
//! `app_handle.emit("plugin-event", ..)`. Here:
//!
//! * directories arrive as a [`PluginStore`], so a caller decides where
//!   plugins live (and a test can point it at a temporary directory);
//! * state changes go out on a `tokio::sync::broadcast` channel, which a
//!   shell can forward to whatever notification mechanism it has, or ignore;
//! * loading and unloading fold in what used to be a separate `PluginLoader`.
//!   It was a thin wrapper around `spawn_blocking` and a `HashSet`, and
//!   keeping the loaded set next to the plugin map removes a lock ordering
//!   nobody was enforcing.
//!
//! Boa's `Context` is not `Send`, so a plugin's JavaScript runs inside
//! `spawn_blocking` and its runtime is created and dropped inside that call.
//! Nothing here holds a plugin runtime between calls.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::sync::{broadcast, RwLock};

use super::error::{PluginError, PluginResult};
use super::manifest::{Plugin, PluginPermission};
use super::marketplace::MarketplacePlugin;
use super::runtime::{HostCall, PluginRuntimeSync};
use super::store::{PluginStore, ScanResult};
use super::ui::{self, PluginUi, UiEvent};
use super::PluginEvent;

/// The result of running plugin code: what it returned, plus everything it
/// asked the host to do while it ran.
#[derive(Debug, Clone)]
pub struct RunOutcome<T> {
    pub value: T,
    pub outbox: Vec<HostCall>,
}

pub struct PluginManager {
    store: PluginStore,
    plugins: Arc<RwLock<HashMap<String, Plugin>>>,
    loaded: Arc<RwLock<HashSet<String>>>,
    app_version: String,
    events: broadcast::Sender<PluginEvent>,
    /// Directories searched for a plugin shipped with the application.
    bundled_roots: Vec<PathBuf>,
}

impl PluginManager {
    pub fn new(store: PluginStore, app_version: impl Into<String>) -> Self {
        let (events, _) = broadcast::channel(64);
        Self {
            store,
            plugins: Arc::new(RwLock::new(HashMap::new())),
            loaded: Arc::new(RwLock::new(HashSet::new())),
            app_version: app_version.into(),
            events,
            bundled_roots: default_bundled_roots(),
        }
    }

    pub fn with_bundled_roots(mut self, roots: Vec<PathBuf>) -> Self {
        self.bundled_roots = roots;
        self
    }

    /// Subscribe to lifecycle events. Replaces the Tauri `plugin-event`
    /// emission; a shell that does not care simply never subscribes.
    pub fn subscribe(&self) -> broadcast::Receiver<PluginEvent> {
        self.events.subscribe()
    }

    fn announce(&self, event: PluginEvent) {
        // An error here only means nobody is listening.
        let _ = self.events.send(event);
    }

    pub fn store(&self) -> &PluginStore {
        &self.store
    }

    /// Create the directories and load what is installed.
    pub async fn initialize(&self) -> PluginResult<ScanResult> {
        self.store.ensure_dirs()?;
        self.rescan().await
    }

    /// Re-read the plugins directory, replacing the in-memory map.
    pub async fn rescan(&self) -> PluginResult<ScanResult> {
        let store = self.store.clone();
        let version = self.app_version.clone();
        let scan = tokio::task::spawn_blocking(move || store.scan(&version))
            .await
            .map_err(|e| PluginError::Other(format!("scan task failed: {}", e)))?;

        let mut map = self.plugins.write().await;
        map.clear();
        for p in &scan.plugins {
            map.insert(p.manifest.id.clone(), p.clone());
        }
        drop(map);

        // A plugin that is gone cannot still be loaded.
        let ids: HashSet<String> = scan.plugins.iter().map(|p| p.manifest.id.clone()).collect();
        self.loaded.write().await.retain(|id| ids.contains(id));

        for problem in &scan.problems {
            self.announce(PluginEvent::Error {
                plugin_id: String::new(),
                error: problem.clone(),
            });
        }
        Ok(scan)
    }

    pub async fn installed(&self) -> Vec<Plugin> {
        let map = self.plugins.read().await;
        let mut out: Vec<Plugin> = map.values().cloned().collect();
        out.sort_by(|a, b| {
            a.manifest
                .name
                .to_lowercase()
                .cmp(&b.manifest.name.to_lowercase())
        });
        out
    }

    pub async fn get(&self, plugin_id: &str) -> Option<Plugin> {
        self.plugins.read().await.get(plugin_id).cloned()
    }

    async fn require(&self, plugin_id: &str) -> PluginResult<Plugin> {
        self.get(plugin_id)
            .await
            .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))
    }

    /// Install from a marketplace entry.
    ///
    /// Only plugins bundled with the application can be installed: there is
    /// no marketplace server, and the Tauri shell's fallback wrote a fake
    /// "placeholder" plugin that looked installed but did nothing. An honest
    /// error is better than a plugin that silently is not the plugin the user
    /// asked for. When a real download exists it goes here, and its bytes
    /// must pass [`verify_artifact_hash`] before they reach disk.
    pub async fn install(&self, entry: &MarketplacePlugin) -> PluginResult<Plugin> {
        let id = entry.manifest.id.clone();
        if self.get(&id).await.is_some() {
            return Err(PluginError::AlreadyInstalled(id));
        }
        entry.manifest.validate()?;
        entry.manifest.check_compatibility(&self.app_version)?;

        let source = self
            .find_bundled(&id)
            .ok_or_else(|| PluginError::NotSupported(format!("downloading '{}'", id)))?;

        let store = self.store.clone();
        let version = self.app_version.clone();
        let id_for_task = id.clone();
        let plugin = tokio::task::spawn_blocking(move || -> PluginResult<Plugin> {
            let dir = store.install_from_dir(&id_for_task, &source)?;
            store.load_dir(&dir, &version)
        })
        .await
        .map_err(|e| PluginError::Other(format!("install task failed: {}", e)))??;

        self.plugins
            .write()
            .await
            .insert(id.clone(), plugin.clone());
        self.announce(PluginEvent::Installed {
            plugin_id: id,
            version: plugin.manifest.version.clone(),
        });
        Ok(plugin)
    }

    pub async fn uninstall(&self, plugin_id: &str) -> PluginResult<()> {
        let plugin = self.require(plugin_id).await?;

        let store = self.store.clone();
        let target = plugin.clone();
        tokio::task::spawn_blocking(move || store.uninstall(&target))
            .await
            .map_err(|e| PluginError::Other(format!("uninstall task failed: {}", e)))??;

        self.plugins.write().await.remove(plugin_id);
        self.loaded.write().await.remove(plugin_id);
        self.announce(PluginEvent::Uninstalled {
            plugin_id: plugin_id.to_string(),
        });
        Ok(())
    }

    /// Enable or disable.
    ///
    /// Disabling unloads first, which is the order the React shell used and
    /// the only safe one: a disabled plugin must not still be running.
    pub async fn set_enabled(&self, plugin_id: &str, enabled: bool) -> PluginResult<Plugin> {
        let mut plugin = self.require(plugin_id).await?;

        if !enabled {
            let _ = self.unload(plugin_id).await;
        }

        let store = self.store.clone();
        let mut owned = plugin.clone();
        let saved = tokio::task::spawn_blocking(move || -> PluginResult<Plugin> {
            store.set_enabled(&mut owned, enabled)?;
            Ok(owned)
        })
        .await
        .map_err(|e| PluginError::Other(format!("state task failed: {}", e)))??;

        plugin = saved;
        self.plugins
            .write()
            .await
            .insert(plugin_id.to_string(), plugin.clone());
        self.announce(if enabled {
            PluginEvent::Enabled {
                plugin_id: plugin_id.to_string(),
            }
        } else {
            PluginEvent::Disabled {
                plugin_id: plugin_id.to_string(),
            }
        });
        Ok(plugin)
    }

    pub async fn update_config(
        &self,
        plugin_id: &str,
        config: serde_json::Value,
    ) -> PluginResult<Plugin> {
        let plugin = self.require(plugin_id).await?;

        let store = self.store.clone();
        let mut owned = plugin.clone();
        let saved = tokio::task::spawn_blocking(move || -> PluginResult<Plugin> {
            store.save_config(&mut owned, config)?;
            Ok(owned)
        })
        .await
        .map_err(|e| PluginError::Other(format!("config task failed: {}", e)))??;

        self.plugins
            .write()
            .await
            .insert(plugin_id.to_string(), saved.clone());
        Ok(saved)
    }

    /// Is this permission both declared and currently effective?
    pub async fn check_permission(
        &self,
        plugin_id: &str,
        permission: PluginPermission,
    ) -> PluginResult<()> {
        let plugin = self.require(plugin_id).await?;
        if plugin.enabled && plugin.has_permission(permission) {
            Ok(())
        } else {
            Err(PluginError::PermissionDenied(permission.label().to_string()))
        }
    }

    // ---- Execution ------------------------------------------------------

    /// Run a plugin's `onLoad` and mark it loaded.
    pub async fn load(&self, plugin_id: &str) -> PluginResult<RunOutcome<serde_json::Value>> {
        let outcome = self
            .with_runtime(plugin_id, |rt| {
                let value = rt.call_on_load()?;
                Ok(value)
            })
            .await?;
        self.loaded.write().await.insert(plugin_id.to_string());
        Ok(outcome)
    }

    /// Forget a loaded plugin. There is no long-lived runtime to tear down;
    /// `onUnload` is called for the plugin's benefit, and its failure does
    /// not stop the unload.
    pub async fn unload(&self, plugin_id: &str) -> PluginResult<()> {
        if !self.loaded.write().await.remove(plugin_id) {
            return Err(PluginError::NotFound(plugin_id.to_string()));
        }
        let _ = self
            .with_runtime(plugin_id, |rt| rt.call_on_unload())
            .await;
        Ok(())
    }

    pub async fn is_loaded(&self, plugin_id: &str) -> bool {
        self.loaded.read().await.contains(plugin_id)
    }

    pub async fn loaded_ids(&self) -> Vec<String> {
        let mut v: Vec<String> = self.loaded.read().await.iter().cloned().collect();
        v.sort();
        v
    }

    /// Call an exported function by name with a JSON argument.
    pub async fn execute_function(
        &self,
        plugin_id: &str,
        function: &str,
        argument: serde_json::Value,
    ) -> PluginResult<RunOutcome<serde_json::Value>> {
        let function = function.to_string();
        self.with_runtime(plugin_id, move |rt| rt.call(&function, &argument))
            .await
    }

    /// Ask a plugin to describe its interface.
    pub async fn render_ui(&self, plugin_id: &str) -> PluginResult<RunOutcome<PluginUi>> {
        self.with_runtime(plugin_id, |rt| rt.render_ui()).await
    }

    /// Deliver a UI event.
    ///
    /// `current` is the descriptor the user is actually looking at: the event
    /// is resolved against it here, in Rust, before any plugin code runs, so
    /// an event for a control the plugin never declared is refused rather
    /// than becoming a call.
    pub async fn dispatch_ui_event(
        &self,
        plugin_id: &str,
        current: &PluginUi,
        event: &UiEvent,
    ) -> PluginResult<RunOutcome<Option<PluginUi>>> {
        let dispatch = ui::dispatch_for(current, event)
            .map_err(|e| PluginError::ExecutionError(e.to_string()))?;
        self.with_runtime(plugin_id, move |rt| rt.dispatch(&dispatch))
            .await
    }

    /// Read the plugin's code, build a runtime, run `body`, collect the
    /// outbox, record the execution. All of it on a blocking thread.
    async fn with_runtime<T, F>(&self, plugin_id: &str, body: F) -> PluginResult<RunOutcome<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut PluginRuntimeSync) -> PluginResult<T> + Send + 'static,
    {
        let plugin = self.require(plugin_id).await?;
        if !plugin.enabled {
            return Err(PluginError::ExecutionError(format!(
                "Plugin {} is disabled",
                plugin_id
            )));
        }

        let main_path = plugin.main_path()?;
        let data_dir = self.store.data_dir(&plugin.manifest.id)?;
        let store = self.store.clone();

        let (outcome, mut owned) = tokio::task::spawn_blocking(
            move || -> (PluginResult<RunOutcome<T>>, Plugin) {
                let result = (|| {
                    let code = std::fs::read_to_string(&main_path)?;
                    let mut rt = PluginRuntimeSync::new(&plugin, data_dir)?;
                    rt.initialize()?;
                    rt.execute(&code)?;
                    let value = body(&mut rt)?;
                    Ok(RunOutcome {
                        value,
                        outbox: rt.take_outbox(),
                    })
                })();
                (result, plugin)
            },
        )
        .await
        .map_err(|e| PluginError::Other(format!("plugin task failed: {}", e)))?;

        let ok = outcome.is_ok();
        let store_for_stats = store.clone();
        let recorded = tokio::task::spawn_blocking(move || {
            let _ = store_for_stats.record_run(&mut owned, ok);
            owned
        })
        .await
        .ok();

        if let Some(updated) = recorded {
            self.plugins
                .write()
                .await
                .insert(plugin_id.to_string(), updated);
        }

        if let Err(e) = &outcome {
            self.announce(PluginEvent::Error {
                plugin_id: plugin_id.to_string(),
                error: e.to_string(),
            });
        }
        outcome
    }

    // ---- Bundled sources ------------------------------------------------

    fn find_bundled(&self, plugin_id: &str) -> Option<PathBuf> {
        let folder = plugin_id.rsplit('.').next().unwrap_or(plugin_id);
        if folder.is_empty() || folder.contains(std::path::MAIN_SEPARATOR) {
            return None;
        }
        for root in &self.bundled_roots {
            let candidate = root.join(folder);
            if candidate.join("manifest.json").is_file() {
                return Some(candidate);
            }
        }
        None
    }
}

/// Where a bundled plugin might be, in order.
///
/// The Tauri version also searched a developer's hard-coded home directory.
/// That is gone.
pub fn default_bundled_roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("plugins"), PathBuf::from("../plugins")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.join("plugins"));
            // macOS bundle: Contents/MacOS/<exe> -> Contents/Resources/plugins
            roots.push(dir.join("../Resources/plugins"));
        }
    }
    roots
}

/// Verify a plugin artifact against the marketplace-provided SHA-256.
///
/// An empty hash means there is nothing to verify. A present one must be a
/// valid hex digest and must match exactly.
pub fn verify_artifact_hash(artifact: &[u8], expected_hash: &str) -> PluginResult<()> {
    let expected = expected_hash
        .trim()
        .trim_start_matches("sha256:")
        .to_ascii_lowercase();

    if expected.is_empty() {
        return Ok(());
    }
    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(PluginError::IntegrityCheckFailed(format!(
            "marketplace hash is not a valid SHA-256 hex digest: {}",
            expected_hash
        )));
    }

    let actual = hex::encode(Sha256::digest(artifact));
    if actual != expected {
        return Err(PluginError::IntegrityCheckFailed(format!(
            "SHA-256 mismatch: expected {}, got {}",
            expected, actual
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_hashes_are_verified() {
        let data = b"plugin artifact bytes";
        let good = hex::encode(Sha256::digest(data));

        assert!(verify_artifact_hash(data, &good).is_ok());
        assert!(verify_artifact_hash(data, &format!("sha256:{}", good)).is_ok());
        assert!(verify_artifact_hash(data, &good.to_uppercase()).is_ok());
        assert!(verify_artifact_hash(data, "").is_ok());

        assert!(verify_artifact_hash(data, "abc123def456").is_err());
        assert!(verify_artifact_hash(b"tampered", &good).is_err());
    }

    #[test]
    fn bundled_lookup_uses_the_last_id_segment() {
        let m = PluginManager::new(
            PluginStore::new(PathBuf::from("/nonexistent"), PathBuf::from("/nonexistent")),
            "0.21.0",
        )
        .with_bundled_roots(vec![PathBuf::from("/nonexistent")]);
        // Nothing on disk, so this only proves it does not panic on odd ids.
        assert!(m.find_bundled("com.dbhive.csv-exporter").is_none());
        assert!(m.find_bundled("").is_none());
    }
}
