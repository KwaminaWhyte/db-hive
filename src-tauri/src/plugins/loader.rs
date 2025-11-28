//! Plugin loader - loads and executes plugins using the JavaScript runtime

use super::{Plugin, PluginError, PluginResult, PluginType};
use serde_json::Value as JsonValue;
use std::collections::HashSet;
use std::sync::Arc;
use tauri::AppHandle;
use tokio::fs;
use tokio::sync::RwLock;

/// Manages plugin loading and execution
/// Note: Due to boa_engine's Context not being Send/Sync, we execute plugins on-demand
/// rather than keeping long-running runtimes.
pub struct PluginLoader {
    /// Set of loaded plugin IDs (for tracking which plugins have been initialized)
    loaded_plugins: Arc<RwLock<HashSet<String>>>,
    /// App handle for creating contexts
    app_handle: AppHandle,
}

impl PluginLoader {
    /// Create a new plugin loader
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            loaded_plugins: Arc::new(RwLock::new(HashSet::new())),
            app_handle,
        }
    }

    /// Load and execute a plugin's initialization
    pub async fn load_plugin(&self, plugin: &Plugin) -> PluginResult<()> {
        match plugin.manifest.plugin_type {
            PluginType::JavaScript => self.load_js_plugin(plugin).await,
            PluginType::WebAssembly => self.load_wasm_plugin(plugin).await,
        }
    }

    /// Load a JavaScript plugin
    async fn load_js_plugin(&self, plugin: &Plugin) -> PluginResult<()> {
        let main_path = plugin.path.join(&plugin.manifest.main);

        if !main_path.exists() {
            return Err(PluginError::Other(format!(
                "Plugin main file not found: {:?}",
                main_path
            )));
        }

        // Read the plugin code
        let code = fs::read_to_string(&main_path).await?;

        // Execute in a blocking task since boa_engine is not Send
        let plugin_id = plugin.manifest.id.clone();
        let plugin_clone = plugin.clone();
        let app_handle = self.app_handle.clone();

        let result = tokio::task::spawn_blocking(move || {
            // Create runtime and execute
            let mut runtime = super::runtime::PluginRuntimeSync::new(&plugin_clone, &app_handle)?;
            runtime.initialize()?;
            runtime.execute(&code)?;
            runtime.call_on_load()
        })
        .await
        .map_err(|e| PluginError::ExecutionError(format!("Task join error: {}", e)))??;

        println!(
            "[PluginLoader] Plugin {} loaded: {:?}",
            plugin_id, result
        );

        // Mark as loaded
        let mut loaded = self.loaded_plugins.write().await;
        loaded.insert(plugin_id);

        Ok(())
    }

    /// Load a WebAssembly plugin (not yet implemented)
    async fn load_wasm_plugin(&self, plugin: &Plugin) -> PluginResult<()> {
        let main_path = plugin.path.join(&plugin.manifest.main);

        if !main_path.exists() {
            return Err(PluginError::Other(format!(
                "Plugin main file not found: {:?}",
                main_path
            )));
        }

        // WASM support is not yet implemented
        Err(PluginError::Other(
            "WebAssembly plugins are not yet supported".to_string(),
        ))
    }

    /// Unload a plugin
    pub async fn unload_plugin(&self, plugin_id: &str) -> PluginResult<()> {
        let mut loaded = self.loaded_plugins.write().await;

        if loaded.remove(plugin_id) {
            println!("[PluginLoader] Plugin {} unloaded", plugin_id);
            Ok(())
        } else {
            Err(PluginError::NotFound(plugin_id.to_string()))
        }
    }

    /// Check if a plugin is loaded
    pub async fn is_loaded(&self, plugin_id: &str) -> bool {
        let loaded = self.loaded_plugins.read().await;
        loaded.contains(plugin_id)
    }

    /// Execute a function on a plugin
    pub async fn execute_function(
        &self,
        plugin: &Plugin,
        function_name: &str,
        _args: Vec<JsonValue>,
    ) -> PluginResult<JsonValue> {
        let main_path = plugin.path.join(&plugin.manifest.main);

        if !main_path.exists() {
            return Err(PluginError::Other(format!(
                "Plugin main file not found: {:?}",
                main_path
            )));
        }

        let code = fs::read_to_string(&main_path).await?;
        let plugin_clone = plugin.clone();
        let app_handle = self.app_handle.clone();
        let func_name = function_name.to_string();

        tokio::task::spawn_blocking(move || {
            let mut runtime = super::runtime::PluginRuntimeSync::new(&plugin_clone, &app_handle)?;
            runtime.initialize()?;
            runtime.execute(&code)?;
            runtime.call_function(&func_name)
        })
        .await
        .map_err(|e| PluginError::ExecutionError(format!("Task join error: {}", e)))?
    }

    /// Get the list of loaded plugins
    pub async fn get_loaded_plugins(&self) -> Vec<String> {
        let loaded = self.loaded_plugins.read().await;
        loaded.iter().cloned().collect()
    }
}

// Make PluginLoader Send + Sync
unsafe impl Send for PluginLoader {}
unsafe impl Sync for PluginLoader {}
