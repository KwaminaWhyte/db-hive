use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::AppHandle;

pub mod api;
pub mod loader;
pub mod manager;
pub mod runtime;
pub mod sandbox;

pub use api::PluginApi;
pub use manager::PluginManager;
pub use runtime::PluginRuntimeSync;

/// Plugin manifest that defines a plugin's metadata and requirements
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    /// Unique identifier for the plugin (e.g., "com.example.myplugin")
    pub id: String,

    /// Human-readable name of the plugin
    pub name: String,

    /// Plugin version following semver (e.g., "1.0.0")
    pub version: String,

    /// Brief description of what the plugin does
    pub description: String,

    /// Author information
    pub author: PluginAuthor,

    /// Plugin category for marketplace organization
    pub category: PluginCategory,

    /// Main entry point file (relative to plugin directory)
    pub main: String,

    /// Plugin type (JavaScript or WebAssembly)
    pub plugin_type: PluginType,

    /// Required permissions for the plugin
    pub permissions: Vec<PluginPermission>,

    /// Minimum DB-Hive version required
    pub min_version: String,

    /// Maximum DB-Hive version supported (optional)
    pub max_version: Option<String>,

    /// Plugin icon URL or path
    pub icon: Option<String>,

    /// Homepage URL
    pub homepage: Option<String>,

    /// Repository URL
    pub repository: Option<String>,

    /// License identifier (e.g., "MIT", "Apache-2.0")
    pub license: String,

    /// Keywords for search
    pub keywords: Vec<String>,

    /// Plugin dependencies (other plugin IDs)
    pub dependencies: Option<HashMap<String, String>>,

    /// Configuration schema for user settings
    pub config_schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginAuthor {
    pub name: String,
    pub email: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PluginCategory {
    Driver,      // Database drivers
    Theme,       // UI themes
    Tool,        // Utility tools
    Export,      // Export formats
    Import,      // Import formats
    Formatter,   // Code formatters
    Analyzer,    // Query analyzers
    Visualizer,  // Data visualizers
    Extension,   // General extensions
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PluginType {
    JavaScript,  // JS/TS plugins
    WebAssembly, // WASM plugins
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum PluginPermission {
    // File system permissions
    ReadFiles,
    WriteFiles,

    // Database permissions
    ExecuteQuery,
    ModifySchema,
    ReadMetadata,

    // UI permissions
    CreateTab,
    ModifyUI,
    ShowNotification,

    // Network permissions
    NetworkAccess,

    // System permissions
    RunCommand,
    AccessClipboard,

    // Plugin permissions
    AccessOtherPlugins,
}

/// Represents an installed and loaded plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plugin {
    pub manifest: PluginManifest,
    pub path: PathBuf,
    pub enabled: bool,
    pub loaded: bool,
    pub config: Option<serde_json::Value>,
    pub stats: PluginStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStats {
    pub install_date: String,
    pub last_used: Option<String>,
    pub execution_count: u64,
    pub error_count: u64,
    pub rating: Option<f32>,
    pub downloads: u64,
}

/// Plugin marketplace metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplacePlugin {
    pub manifest: PluginManifest,
    pub stats: MarketplaceStats,
    pub screenshots: Vec<String>,
    pub changelog: Option<String>,
    pub readme: Option<String>,
    pub download_url: String,
    pub size: u64,
    pub hash: String,
    pub signature: Option<String>,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceStats {
    pub downloads: u64,
    pub stars: u64,
    pub rating: f32,
    pub reviews: u64,
    pub updated_at: String,
    pub created_at: String,
}

/// Plugin execution context
#[derive(Debug, Clone)]
pub struct PluginContext {
    pub app_handle: AppHandle,
    pub plugin_id: String,
    pub permissions: Vec<PluginPermission>,
    pub config: Option<serde_json::Value>,
}

/// Result of plugin operations
pub type PluginResult<T> = Result<T, PluginError>;

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("Plugin not found: {0}")]
    NotFound(String),

    #[error("Plugin already installed: {0}")]
    AlreadyInstalled(String),

    #[error("Invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("Permission denied: {0:?}")]
    PermissionDenied(PluginPermission),

    #[error("Plugin execution error: {0}")]
    ExecutionError(String),

    #[error("Incompatible version: requires {required}, current is {current}")]
    IncompatibleVersion { required: String, current: String },

    #[error("Missing dependency: {0}")]
    MissingDependency(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Other error: {0}")]
    Other(String),
}

impl From<tauri::Error> for PluginError {
    fn from(err: tauri::Error) -> Self {
        PluginError::Other(err.to_string())
    }
}

/// Plugin events that can be emitted
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum PluginEvent {
    Installed {
        plugin_id: String,
        version: String,
    },
    Uninstalled {
        plugin_id: String,
    },
    Enabled {
        plugin_id: String,
    },
    Disabled {
        plugin_id: String,
    },
    Updated {
        plugin_id: String,
        old_version: String,
        new_version: String,
    },
    Error {
        plugin_id: String,
        error: String,
    },
}