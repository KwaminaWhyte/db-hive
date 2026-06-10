use super::{
    MarketplacePlugin, Plugin, PluginContext, PluginError, PluginEvent, PluginManifest,
    PluginPermission, PluginResult, PluginStats,
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::fs;
use tokio::sync::RwLock;

/// Manages all installed plugins and their lifecycle
pub struct PluginManager {
    /// Application handle for Tauri integration
    app_handle: AppHandle,

    /// Directory where plugins are stored
    plugins_dir: PathBuf,

    /// Map of plugin ID to plugin instance
    plugins: Arc<RwLock<HashMap<String, Plugin>>>,

    /// Current DB Hive version for compatibility checks
    app_version: String,
}

impl PluginManager {
    pub fn new(app_handle: AppHandle) -> Self {
        let plugins_dir = app_handle
            .path()
            .app_data_dir()
            .expect("Failed to get app data dir")
            .join("plugins");

        Self {
            app_handle,
            plugins_dir,
            plugins: Arc::new(RwLock::new(HashMap::new())),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Initialize the plugin manager and load all installed plugins
    pub async fn initialize(&self) -> PluginResult<()> {
        // Create plugins directory if it doesn't exist
        fs::create_dir_all(&self.plugins_dir).await?;

        // Load all installed plugins
        self.load_all_plugins().await?;

        Ok(())
    }

    /// Load all plugins from the plugins directory
    async fn load_all_plugins(&self) -> PluginResult<()> {
        let mut entries = fs::read_dir(&self.plugins_dir).await?;
        let mut plugins = self.plugins.write().await;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                match self.load_plugin_from_dir(&path).await {
                    Ok(plugin) => {
                        plugins.insert(plugin.manifest.id.clone(), plugin);
                    }
                    Err(e) => {
                        eprintln!("Failed to load plugin from {:?}: {}", path, e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Load a single plugin from a directory
    async fn load_plugin_from_dir(&self, path: &Path) -> PluginResult<Plugin> {
        let manifest_path = path.join("manifest.json");
        let manifest_content = fs::read_to_string(&manifest_path).await?;
        let manifest: PluginManifest = serde_json::from_str(&manifest_content)?;

        // Check version compatibility
        self.check_version_compatibility(&manifest)?;

        // Load plugin configuration if it exists
        let config_path = path.join("config.json");
        let config = if config_path.exists() {
            let config_content = fs::read_to_string(&config_path).await?;
            Some(serde_json::from_str(&config_content)?)
        } else {
            None
        };

        // Load plugin stats
        let stats_path = path.join(".stats.json");
        let stats = if stats_path.exists() {
            let stats_content = fs::read_to_string(&stats_path).await?;
            serde_json::from_str(&stats_content)?
        } else {
            PluginStats {
                install_date: chrono::Utc::now().to_rfc3339(),
                last_used: None,
                execution_count: 0,
                error_count: 0,
                rating: None,
                downloads: 1,
            }
        };

        Ok(Plugin {
            manifest,
            path: path.to_path_buf(),
            enabled: true,
            loaded: false,
            config,
            stats,
        })
    }

    /// Check if a plugin is compatible with the current app version
    fn check_version_compatibility(&self, manifest: &PluginManifest) -> PluginResult<()> {
        // Simple version check - in production, use semver crate
        if manifest.min_version > self.app_version {
            return Err(PluginError::IncompatibleVersion {
                required: manifest.min_version.clone(),
                current: self.app_version.clone(),
            });
        }

        if let Some(max_version) = &manifest.max_version {
            if &self.app_version > max_version {
                return Err(PluginError::IncompatibleVersion {
                    required: format!("<= {}", max_version),
                    current: self.app_version.clone(),
                });
            }
        }

        Ok(())
    }

    /// Install a plugin from a marketplace entry
    pub async fn install_plugin(&self, marketplace_plugin: &MarketplacePlugin) -> PluginResult<()> {
        let plugins = self.plugins.read().await;

        // Check if already installed
        if plugins.contains_key(&marketplace_plugin.manifest.id) {
            return Err(PluginError::AlreadyInstalled(
                marketplace_plugin.manifest.id.clone(),
            ));
        }
        drop(plugins);

        // Check version compatibility
        self.check_version_compatibility(&marketplace_plugin.manifest)?;

        // Create plugin directory
        let plugin_dir = self.plugins_dir.join(&marketplace_plugin.manifest.id);
        fs::create_dir_all(&plugin_dir).await?;

        // Try to find bundled plugin source
        let bundled_source = self.find_bundled_plugin(&marketplace_plugin.manifest.id).await;

        if let Some(source_dir) = bundled_source {
            // Copy files from bundled plugin
            self.copy_plugin_files(&source_dir, &plugin_dir).await?;
            println!(
                "[PluginManager] Installed plugin {} from bundled source",
                marketplace_plugin.manifest.id
            );
        } else {
            // TODO: Implement actual download from marketplace_plugin.download_url
            // For now, create a placeholder that indicates the plugin needs real code.
            // Any artifact installed via this path (downloaded or placeholder) must
            // pass the SHA-256 integrity check against marketplace_plugin.hash
            // before it is persisted.
            let artifact = format!(
                "// Plugin '{}' - placeholder\n// Real implementation should be downloaded from: {}\n\n__plugin_exports__ = {{\n  onLoad: function() {{\n    console.log('Plugin {} loaded (placeholder)');\n    return {{ success: true, message: 'Placeholder plugin' }};\n  }},\n  onUnload: function() {{\n    return {{ success: true }};\n  }}\n}};\n",
                marketplace_plugin.manifest.name,
                marketplace_plugin.download_url,
                marketplace_plugin.manifest.name
            );

            if let Err(e) =
                Self::verify_artifact_hash(artifact.as_bytes(), &marketplace_plugin.hash)
            {
                let _ = fs::remove_dir_all(&plugin_dir).await;
                return Err(e);
            }

            // Create manifest file
            let manifest_path = plugin_dir.join("manifest.json");
            let manifest_content = serde_json::to_string_pretty(&marketplace_plugin.manifest)?;
            fs::write(&manifest_path, manifest_content).await?;

            let main_path = plugin_dir.join(&marketplace_plugin.manifest.main);
            fs::write(&main_path, artifact).await?;
            println!(
                "[PluginManager] Installed plugin {} with placeholder (no bundled source found)",
                marketplace_plugin.manifest.id
            );
        }

        // Load the plugin
        let plugin = self.load_plugin_from_dir(&plugin_dir).await?;

        let mut plugins = self.plugins.write().await;
        plugins.insert(plugin.manifest.id.clone(), plugin.clone());

        // Emit installation event
        self.app_handle.emit(
            "plugin-event",
            PluginEvent::Installed {
                plugin_id: plugin.manifest.id.clone(),
                version: plugin.manifest.version.clone(),
            },
        )?;

        Ok(())
    }

    /// Find bundled plugin source directory
    async fn find_bundled_plugin(&self, plugin_id: &str) -> Option<PathBuf> {
        // Check common bundled plugins locations
        let possible_paths = vec![
            // Development: plugins/ directory in project root (from src-tauri, go up one level)
            PathBuf::from("../plugins"),
            // Also try absolute path for development
            PathBuf::from("/home/kwamina/Desktop/others/db-hive/plugins"),
            // Current directory (just in case)
            PathBuf::from("plugins"),
            // Resource directory (for packaged app)
            self.app_handle
                .path()
                .resource_dir()
                .ok()
                .map(|p| p.join("plugins"))
                .unwrap_or_default(),
        ];

        // Map plugin IDs to directory names (for bundled plugins with different folder names)
        let folder_name = match plugin_id {
            "com.dbhive.csv-exporter" => "csv-exporter",
            "com.dbhive.dark-theme-plus" => "dark-theme-plus",
            "com.dbhive.sql-formatter" => "sql-formatter",
            _ => {
                // Try using the last part of the ID as folder name
                plugin_id.split('.').last().unwrap_or(plugin_id)
            }
        };

        for base_path in possible_paths {
            let plugin_path = base_path.join(folder_name);
            if plugin_path.exists() && plugin_path.is_dir() {
                // Verify it has a manifest or main file
                if plugin_path.join("manifest.json").exists()
                    || plugin_path.join("index.js").exists()
                {
                    println!(
                        "[PluginManager] Found bundled plugin at: {:?}",
                        plugin_path
                    );
                    return Some(plugin_path);
                }
            }
        }

        None
    }

    /// Copy plugin files from source to destination
    async fn copy_plugin_files(&self, source: &Path, dest: &Path) -> PluginResult<()> {
        let mut entries = fs::read_dir(source).await?;

        while let Some(entry) = entries.next_entry().await? {
            let entry_path = entry.path();
            let file_name = entry.file_name();
            let dest_path = dest.join(&file_name);

            if entry_path.is_file() {
                let content = fs::read(&entry_path).await?;
                fs::write(&dest_path, content).await?;
                println!("[PluginManager] Copied: {:?}", file_name);
            } else if entry_path.is_dir() {
                // Recursively copy subdirectories
                fs::create_dir_all(&dest_path).await?;
                Box::pin(self.copy_plugin_files(&entry_path, &dest_path)).await?;
            }
        }

        Ok(())
    }

    /// Uninstall a plugin
    pub async fn uninstall_plugin(&self, plugin_id: &str) -> PluginResult<()> {
        let mut plugins = self.plugins.write().await;

        let plugin = plugins
            .remove(plugin_id)
            .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))?;

        // Remove plugin directory
        fs::remove_dir_all(&plugin.path).await?;

        // Emit uninstallation event
        self.app_handle.emit(
            "plugin-event",
            PluginEvent::Uninstalled {
                plugin_id: plugin_id.to_string(),
            },
        )?;

        Ok(())
    }

    /// Enable a plugin
    pub async fn enable_plugin(&self, plugin_id: &str) -> PluginResult<()> {
        let mut plugins = self.plugins.write().await;

        let plugin = plugins
            .get_mut(plugin_id)
            .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))?;

        plugin.enabled = true;

        // Save state
        self.save_plugin_state(plugin).await?;

        // Emit event
        self.app_handle.emit(
            "plugin-event",
            PluginEvent::Enabled {
                plugin_id: plugin_id.to_string(),
            },
        )?;

        Ok(())
    }

    /// Disable a plugin
    pub async fn disable_plugin(&self, plugin_id: &str) -> PluginResult<()> {
        let mut plugins = self.plugins.write().await;

        let plugin = plugins
            .get_mut(plugin_id)
            .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))?;

        plugin.enabled = false;
        plugin.loaded = false;

        // Save state
        self.save_plugin_state(plugin).await?;

        // Emit event
        self.app_handle.emit(
            "plugin-event",
            PluginEvent::Disabled {
                plugin_id: plugin_id.to_string(),
            },
        )?;

        Ok(())
    }

    /// Get all installed plugins
    pub async fn get_installed_plugins(&self) -> Vec<Plugin> {
        let plugins = self.plugins.read().await;
        plugins.values().cloned().collect()
    }

    /// Get a specific plugin by ID
    pub async fn get_plugin(&self, plugin_id: &str) -> Option<Plugin> {
        let plugins = self.plugins.read().await;
        plugins.get(plugin_id).cloned()
    }

    /// Update plugin configuration
    pub async fn update_plugin_config(
        &self,
        plugin_id: &str,
        config: serde_json::Value,
    ) -> PluginResult<()> {
        let mut plugins = self.plugins.write().await;

        let plugin = plugins
            .get_mut(plugin_id)
            .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))?;

        plugin.config = Some(config);

        // Save configuration
        let config_path = plugin.path.join("config.json");
        let config_content = serde_json::to_string_pretty(&plugin.config)?;
        fs::write(&config_path, config_content).await?;

        Ok(())
    }

    /// Check that a plugin is enabled and has declared a permission in its manifest
    pub async fn check_permission(
        &self,
        plugin_id: &str,
        permission: &PluginPermission,
    ) -> PluginResult<()> {
        let plugins = self.plugins.read().await;
        let plugin = plugins
            .get(plugin_id)
            .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))?;

        if !plugin.enabled || !plugin.manifest.permissions.contains(permission) {
            return Err(PluginError::PermissionDenied(permission.clone()));
        }

        Ok(())
    }

    /// Verify a plugin artifact's SHA-256 against the marketplace-provided hash.
    /// An empty hash means there is nothing to verify; a present hash must be a
    /// valid SHA-256 hex digest and must match the artifact exactly.
    fn verify_artifact_hash(artifact: &[u8], expected_hash: &str) -> PluginResult<()> {
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

    /// Save plugin state to disk
    async fn save_plugin_state(&self, plugin: &Plugin) -> PluginResult<()> {
        let state_path = plugin.path.join(".state.json");
        let state = serde_json::json!({
            "enabled": plugin.enabled,
            "loaded": plugin.loaded,
        });
        let state_content = serde_json::to_string_pretty(&state)?;
        fs::write(&state_path, state_content).await?;

        // Save stats
        let stats_path = plugin.path.join(".stats.json");
        let stats_content = serde_json::to_string_pretty(&plugin.stats)?;
        fs::write(&stats_path, stats_content).await?;

        Ok(())
    }

    /// Create a plugin context for execution
    pub fn create_context(&self, plugin_id: String, plugin: &Plugin) -> PluginContext {
        PluginContext {
            app_handle: self.app_handle.clone(),
            plugin_id,
            permissions: plugin.manifest.permissions.clone(),
            config: plugin.config.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_artifact_hash() {
        let data = b"plugin artifact bytes";
        let good = hex::encode(Sha256::digest(data));

        assert!(PluginManager::verify_artifact_hash(data, &good).is_ok());
        assert!(PluginManager::verify_artifact_hash(data, &format!("sha256:{}", good)).is_ok());
        assert!(PluginManager::verify_artifact_hash(data, &good.to_uppercase()).is_ok());

        // Empty hash means nothing to verify
        assert!(PluginManager::verify_artifact_hash(data, "").is_ok());

        // Malformed hash refused
        assert!(PluginManager::verify_artifact_hash(data, "abc123def456").is_err());

        // Tampered artifact refused
        assert!(PluginManager::verify_artifact_hash(b"tampered", &good).is_err());
    }
}