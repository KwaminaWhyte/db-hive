use crate::plugins::{loader::PluginLoader, MarketplacePlugin, Plugin, PluginManager};
use serde_json::Value;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

/// Get all installed plugins
#[tauri::command]
pub async fn get_installed_plugins(
    manager: State<'_, Arc<Mutex<PluginManager>>>,
) -> Result<Vec<Plugin>, String> {
    let manager = manager.lock().await;
    Ok(manager.get_installed_plugins().await)
}

/// Get a specific plugin by ID
#[tauri::command]
pub async fn get_plugin(
    plugin_id: String,
    manager: State<'_, Arc<Mutex<PluginManager>>>,
) -> Result<Option<Plugin>, String> {
    let manager = manager.lock().await;
    Ok(manager.get_plugin(&plugin_id).await)
}

/// Install a plugin from marketplace
#[tauri::command]
pub async fn install_plugin(
    marketplace_plugin: MarketplacePlugin,
    manager: State<'_, Arc<Mutex<PluginManager>>>,
) -> Result<(), String> {
    let manager = manager.lock().await;
    manager
        .install_plugin(&marketplace_plugin)
        .await
        .map_err(|e| e.to_string())
}

/// Uninstall a plugin
#[tauri::command]
pub async fn uninstall_plugin(
    plugin_id: String,
    manager: State<'_, Arc<Mutex<PluginManager>>>,
) -> Result<(), String> {
    let manager = manager.lock().await;
    manager
        .uninstall_plugin(&plugin_id)
        .await
        .map_err(|e| e.to_string())
}

/// Enable a plugin
#[tauri::command]
pub async fn enable_plugin(
    plugin_id: String,
    manager: State<'_, Arc<Mutex<PluginManager>>>,
) -> Result<(), String> {
    let manager = manager.lock().await;
    manager
        .enable_plugin(&plugin_id)
        .await
        .map_err(|e| e.to_string())
}

/// Disable a plugin
#[tauri::command]
pub async fn disable_plugin(
    plugin_id: String,
    manager: State<'_, Arc<Mutex<PluginManager>>>,
) -> Result<(), String> {
    let manager = manager.lock().await;
    manager
        .disable_plugin(&plugin_id)
        .await
        .map_err(|e| e.to_string())
}

/// Update plugin configuration
#[tauri::command]
pub async fn update_plugin_config(
    plugin_id: String,
    config: Value,
    manager: State<'_, Arc<Mutex<PluginManager>>>,
) -> Result<(), String> {
    let manager = manager.lock().await;
    manager
        .update_plugin_config(&plugin_id, config)
        .await
        .map_err(|e| e.to_string())
}

/// Get marketplace plugins (mock data for now)
#[tauri::command]
pub async fn get_marketplace_plugins(
    category: Option<String>,
    search: Option<String>,
) -> Result<Vec<MarketplacePlugin>, String> {
    use crate::plugins::{
        MarketplaceStats, PluginAuthor, PluginCategory, PluginManifest, PluginType,
    };

    // Mock marketplace plugins for demonstration
    let mut plugins = vec![
        MarketplacePlugin {
            manifest: PluginManifest {
                id: "com.dbhive.csv-exporter".to_string(),
                name: "Advanced CSV Exporter".to_string(),
                version: "1.0.0".to_string(),
                description: "Export query results to CSV with advanced formatting options".to_string(),
                author: PluginAuthor {
                    name: "DB-Hive Team".to_string(),
                    email: Some("plugins@dbhive.com".to_string()),
                    url: None,
                },
                category: PluginCategory::Export,
                main: "index.js".to_string(),
                plugin_type: PluginType::JavaScript,
                permissions: vec![
                    crate::plugins::PluginPermission::ExecuteQuery,
                    crate::plugins::PluginPermission::WriteFiles,
                ],
                min_version: "0.13.0".to_string(),
                max_version: None,
                icon: Some("csv-icon.svg".to_string()),
                homepage: None,
                repository: None,
                license: "MIT".to_string(),
                keywords: vec!["csv".to_string(), "export".to_string(), "data".to_string()],
                dependencies: None,
                config_schema: None,
            },
            stats: MarketplaceStats {
                downloads: 15234,
                stars: 342,
                rating: 4.8,
                reviews: 89,
                updated_at: "2025-11-20T10:00:00Z".to_string(),
                created_at: "2025-10-01T10:00:00Z".to_string(),
            },
            screenshots: vec![],
            changelog: Some("v1.0.0: Initial release with advanced CSV formatting".to_string()),
            readme: None,
            download_url: "https://plugins.dbhive.com/csv-exporter-1.0.0.zip".to_string(),
            size: 45678,
            hash: "abc123def456".to_string(),
            signature: None,
            verified: true,
        },
        MarketplacePlugin {
            manifest: PluginManifest {
                id: "com.dbhive.dark-theme-plus".to_string(),
                name: "Dark Theme Plus".to_string(),
                version: "2.1.0".to_string(),
                description: "Enhanced dark theme with customizable accent colors".to_string(),
                author: PluginAuthor {
                    name: "Theme Designer".to_string(),
                    email: None,
                    url: Some("https://themes.example.com".to_string()),
                },
                category: PluginCategory::Theme,
                main: "theme.css".to_string(),
                plugin_type: PluginType::JavaScript,
                permissions: vec![crate::plugins::PluginPermission::ModifyUI],
                min_version: "0.12.0".to_string(),
                max_version: None,
                icon: Some("theme-icon.svg".to_string()),
                homepage: None,
                repository: None,
                license: "MIT".to_string(),
                keywords: vec!["theme".to_string(), "dark".to_string(), "ui".to_string()],
                dependencies: None,
                config_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "accentColor": {
                            "type": "string",
                            "default": "#3b82f6"
                        }
                    }
                })),
            },
            stats: MarketplaceStats {
                downloads: 28456,
                stars: 567,
                rating: 4.9,
                reviews: 234,
                updated_at: "2025-11-15T10:00:00Z".to_string(),
                created_at: "2025-09-01T10:00:00Z".to_string(),
            },
            screenshots: vec!["screenshot1.png".to_string(), "screenshot2.png".to_string()],
            changelog: Some("v2.1.0: Added 10 new accent colors".to_string()),
            readme: None,
            download_url: "https://plugins.dbhive.com/dark-theme-plus-2.1.0.zip".to_string(),
            size: 23456,
            hash: "xyz789ghi012".to_string(),
            signature: None,
            verified: true,
        },
        MarketplacePlugin {
            manifest: PluginManifest {
                id: "com.dbhive.sql-formatter".to_string(),
                name: "SQL Formatter Pro".to_string(),
                version: "1.5.2".to_string(),
                description: "Format and beautify SQL queries with multiple style options".to_string(),
                author: PluginAuthor {
                    name: "SQL Tools Inc".to_string(),
                    email: Some("support@sqltools.com".to_string()),
                    url: None,
                },
                category: PluginCategory::Formatter,
                main: "formatter.js".to_string(),
                plugin_type: PluginType::JavaScript,
                permissions: vec![crate::plugins::PluginPermission::ModifyUI],
                min_version: "0.11.0".to_string(),
                max_version: None,
                icon: Some("formatter-icon.svg".to_string()),
                homepage: Some("https://sqltools.com".to_string()),
                repository: Some("https://github.com/sqltools/formatter".to_string()),
                license: "Apache-2.0".to_string(),
                keywords: vec!["sql".to_string(), "formatter".to_string(), "beautify".to_string()],
                dependencies: None,
                config_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "indentSize": {
                            "type": "number",
                            "default": 2
                        },
                        "uppercase": {
                            "type": "boolean",
                            "default": true
                        }
                    }
                })),
            },
            stats: MarketplaceStats {
                downloads: 45678,
                stars: 890,
                rating: 4.7,
                reviews: 456,
                updated_at: "2025-11-22T10:00:00Z".to_string(),
                created_at: "2025-08-15T10:00:00Z".to_string(),
            },
            screenshots: vec![],
            changelog: Some("v1.5.2: Fixed formatting for complex JOIN queries".to_string()),
            readme: Some("# SQL Formatter Pro\n\nFormat your SQL queries with style!".to_string()),
            download_url: "https://plugins.dbhive.com/sql-formatter-1.5.2.zip".to_string(),
            size: 67890,
            hash: "qwe345rty678".to_string(),
            signature: None,
            verified: true,
        },
    ];

    // Filter by category if specified
    if let Some(category) = category {
        plugins.retain(|p| format!("{:?}", p.manifest.category).to_lowercase() == category.to_lowercase());
    }

    // Filter by search term if specified
    if let Some(search) = search {
        let search_lower = search.to_lowercase();
        plugins.retain(|p| {
            p.manifest.name.to_lowercase().contains(&search_lower)
                || p.manifest.description.to_lowercase().contains(&search_lower)
                || p.manifest.keywords.iter().any(|k| k.to_lowercase().contains(&search_lower))
        });
    }

    Ok(plugins)
}

/// Load a plugin (execute its code)
#[tauri::command]
pub async fn load_plugin(
    plugin_id: String,
    manager: State<'_, Arc<Mutex<PluginManager>>>,
    loader: State<'_, Arc<Mutex<PluginLoader>>>,
) -> Result<(), String> {
    let manager = manager.lock().await;
    let plugin = manager
        .get_plugin(&plugin_id)
        .await
        .ok_or_else(|| format!("Plugin not found: {}", plugin_id))?;

    if !plugin.enabled {
        return Err(format!("Plugin {} is disabled", plugin_id));
    }

    let loader = loader.lock().await;
    loader.load_plugin(&plugin).await.map_err(|e| e.to_string())
}

/// Unload a plugin
#[tauri::command]
pub async fn unload_plugin_runtime(
    plugin_id: String,
    loader: State<'_, Arc<Mutex<PluginLoader>>>,
) -> Result<(), String> {
    let loader = loader.lock().await;
    loader
        .unload_plugin(&plugin_id)
        .await
        .map_err(|e| e.to_string())
}

/// Execute a plugin function
#[tauri::command]
pub async fn execute_plugin_function(
    plugin_id: String,
    function_name: String,
    args: Value,
    manager: State<'_, Arc<Mutex<PluginManager>>>,
    loader: State<'_, Arc<Mutex<PluginLoader>>>,
) -> Result<Value, String> {
    let args_vec = if args.is_array() {
        args.as_array().unwrap().clone()
    } else if args.is_null() {
        vec![]
    } else {
        vec![args]
    };

    let manager = manager.lock().await;
    let plugin = manager
        .get_plugin(&plugin_id)
        .await
        .ok_or_else(|| format!("Plugin not found: {}", plugin_id))?;

    let loader = loader.lock().await;
    loader
        .execute_function(&plugin, &function_name, args_vec)
        .await
        .map_err(|e| e.to_string())
}

/// Get loaded plugins
#[tauri::command]
pub async fn get_loaded_plugins(
    loader: State<'_, Arc<Mutex<PluginLoader>>>,
) -> Result<Vec<String>, String> {
    let loader = loader.lock().await;
    Ok(loader.get_loaded_plugins().await)
}

/// Check if a plugin is loaded
#[tauri::command]
pub async fn is_plugin_loaded(
    plugin_id: String,
    loader: State<'_, Arc<Mutex<PluginLoader>>>,
) -> Result<bool, String> {
    let loader = loader.lock().await;
    Ok(loader.is_loaded(&plugin_id).await)
}