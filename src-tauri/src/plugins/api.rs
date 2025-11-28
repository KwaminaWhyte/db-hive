use super::{PluginContext, PluginError, PluginPermission, PluginResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tauri::{Emitter, Manager};

/// API exposed to plugins for interacting with DB-Hive
pub struct PluginApi {
    context: PluginContext,
}

impl PluginApi {
    pub fn new(context: PluginContext) -> Self {
        Self { context }
    }

    /// Check if the plugin has a specific permission
    fn check_permission(&self, permission: &PluginPermission) -> PluginResult<()> {
        if !self.context.permissions.contains(permission) {
            return Err(PluginError::PermissionDenied(permission.clone()));
        }
        Ok(())
    }

    // ========== Database API ==========

    /// Execute a SQL query (requires ExecuteQuery permission)
    pub async fn execute_query(&self, _query: &str, _connection_id: &str) -> PluginResult<QueryResult> {
        self.check_permission(&PluginPermission::ExecuteQuery)?;

        // Call the existing execute_query command through Tauri
        // This would integrate with the existing database infrastructure

        // For now, return a mock result
        Ok(QueryResult {
            columns: vec!["id".to_string(), "name".to_string()],
            rows: vec![
                vec![Value::Number(1.into()), Value::String("Test".to_string())],
            ],
            execution_time: 10,
            row_count: 1,
        })
    }

    /// Get database metadata (requires ReadMetadata permission)
    pub async fn get_metadata(&self, _connection_id: &str) -> PluginResult<DatabaseMetadata> {
        self.check_permission(&PluginPermission::ReadMetadata)?;

        // Mock metadata
        Ok(DatabaseMetadata {
            schemas: vec!["public".to_string()],
            tables: HashMap::new(),
        })
    }

    // ========== UI API ==========

    /// Create a new tab in the UI (requires CreateTab permission)
    pub async fn create_tab(&self, tab_config: TabConfig) -> PluginResult<String> {
        self.check_permission(&PluginPermission::CreateTab)?;

        let tab_id = format!("plugin-{}-{}", self.context.plugin_id, uuid::Uuid::new_v4());

        // Emit event to create tab in frontend
        self.context.app_handle.emit(
            "plugin-create-tab",
            CreateTabEvent {
                tab_id: tab_id.clone(),
                plugin_id: self.context.plugin_id.clone(),
                config: tab_config,
            },
        )?;

        Ok(tab_id)
    }

    /// Show a notification (requires ShowNotification permission)
    pub async fn show_notification(&self, notification: Notification) -> PluginResult<()> {
        self.check_permission(&PluginPermission::ShowNotification)?;

        self.context.app_handle.emit(
            "plugin-notification",
            NotificationEvent {
                plugin_id: self.context.plugin_id.clone(),
                notification,
            },
        )?;

        Ok(())
    }

    /// Register a custom UI component (requires ModifyUI permission)
    pub async fn register_ui_component(&self, component: UiComponent) -> PluginResult<()> {
        self.check_permission(&PluginPermission::ModifyUI)?;

        self.context.app_handle.emit(
            "plugin-register-ui",
            RegisterUiEvent {
                plugin_id: self.context.plugin_id.clone(),
                component,
            },
        )?;

        Ok(())
    }

    // ========== File System API ==========

    /// Read a file (requires ReadFiles permission)
    pub async fn read_file(&self, path: &str) -> PluginResult<String> {
        self.check_permission(&PluginPermission::ReadFiles)?;

        // Sandbox the path to plugin's data directory
        let safe_path = self.sandbox_path(path)?;

        let content = tokio::fs::read_to_string(safe_path).await?;
        Ok(content)
    }

    /// Write a file (requires WriteFiles permission)
    pub async fn write_file(&self, path: &str, content: &str) -> PluginResult<()> {
        self.check_permission(&PluginPermission::WriteFiles)?;

        // Sandbox the path to plugin's data directory
        let safe_path = self.sandbox_path(path)?;

        tokio::fs::write(safe_path, content).await?;
        Ok(())
    }

    /// Sandbox a file path to the plugin's data directory
    fn sandbox_path(&self, path: &str) -> PluginResult<std::path::PathBuf> {
        // Get plugin data directory
        let plugin_data_dir = self.context
            .app_handle
            .path()
            .app_data_dir()
            .map_err(|e| PluginError::Other(e.to_string()))?
            .join("plugin-data")
            .join(&self.context.plugin_id);

        // Ensure the path doesn't escape the sandbox
        let requested_path = std::path::Path::new(path);
        if requested_path.is_absolute() || path.contains("..") {
            return Err(PluginError::Other("Invalid file path".to_string()));
        }

        Ok(plugin_data_dir.join(path))
    }

    // ========== Network API ==========

    /// Make an HTTP request (requires NetworkAccess permission)
    pub async fn http_request(&self, _request: HttpRequest) -> PluginResult<HttpResponse> {
        self.check_permission(&PluginPermission::NetworkAccess)?;

        // Use reqwest or similar to make the actual request
        // For now, return a mock response
        Ok(HttpResponse {
            status: 200,
            headers: HashMap::new(),
            body: "Mock response".to_string(),
        })
    }

    // ========== System API ==========

    /// Access clipboard (requires AccessClipboard permission)
    pub async fn clipboard_read(&self) -> PluginResult<String> {
        self.check_permission(&PluginPermission::AccessClipboard)?;

        // Use arboard or similar for clipboard access
        Ok("Clipboard content".to_string())
    }

    pub async fn clipboard_write(&self, _content: &str) -> PluginResult<()> {
        self.check_permission(&PluginPermission::AccessClipboard)?;

        // Use arboard or similar for clipboard access
        Ok(())
    }

    // ========== Plugin Communication API ==========

    /// Send a message to another plugin (requires AccessOtherPlugins permission)
    pub async fn send_to_plugin(&self, target_plugin_id: &str, message: Value) -> PluginResult<()> {
        self.check_permission(&PluginPermission::AccessOtherPlugins)?;

        self.context.app_handle.emit(
            "plugin-message",
            PluginMessage {
                from: self.context.plugin_id.clone(),
                to: target_plugin_id.to_string(),
                message,
            },
        )?;

        Ok(())
    }

    /// Register a message handler
    pub async fn on_message<F>(&self, _handler: F) -> PluginResult<()>
    where
        F: Fn(PluginMessage) + Send + Sync + 'static,
    {
        // Register the handler with the plugin runtime
        // This would be implemented based on the runtime choice
        Ok(())
    }

    // ========== Storage API ==========

    /// Get plugin configuration
    pub fn get_config(&self) -> Option<&Value> {
        self.context.config.as_ref()
    }

    /// Store plugin data
    pub async fn store_data(&self, key: &str, value: Value) -> PluginResult<()> {
        let storage_path = self.sandbox_path(".storage.json")?;

        let mut storage: HashMap<String, Value> = if storage_path.exists() {
            let content = tokio::fs::read_to_string(&storage_path).await?;
            serde_json::from_str(&content)?
        } else {
            HashMap::new()
        };

        storage.insert(key.to_string(), value);

        let content = serde_json::to_string_pretty(&storage)?;
        tokio::fs::write(&storage_path, content).await?;

        Ok(())
    }

    /// Retrieve plugin data
    pub async fn get_data(&self, key: &str) -> PluginResult<Option<Value>> {
        let storage_path = self.sandbox_path(".storage.json")?;

        if !storage_path.exists() {
            return Ok(None);
        }

        let content = tokio::fs::read_to_string(&storage_path).await?;
        let storage: HashMap<String, Value> = serde_json::from_str(&content)?;

        Ok(storage.get(key).cloned())
    }
}

// ========== API Types ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub execution_time: u64,
    pub row_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseMetadata {
    pub schemas: Vec<String>,
    pub tables: HashMap<String, Vec<TableInfo>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableInfo {
    pub name: String,
    pub columns: Vec<ColumnInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabConfig {
    pub title: String,
    pub icon: Option<String>,
    pub content_type: TabContentType,
    pub closable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TabContentType {
    Html(String),
    Iframe(String),
    Custom(Value),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub title: String,
    pub message: String,
    pub notification_type: NotificationType,
    pub duration: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotificationType {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiComponent {
    pub id: String,
    pub location: UiLocation,
    pub component_type: UiComponentType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UiLocation {
    Toolbar,
    Sidebar,
    StatusBar,
    ContextMenu,
    Panel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UiComponentType {
    Button(ButtonComponent),
    MenuItem(MenuItemComponent),
    Panel(PanelComponent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButtonComponent {
    pub label: String,
    pub icon: Option<String>,
    pub tooltip: Option<String>,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuItemComponent {
    pub label: String,
    pub icon: Option<String>,
    pub shortcut: Option<String>,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelComponent {
    pub title: String,
    pub icon: Option<String>,
    pub resizable: bool,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    pub url: String,
    pub method: String,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMessage {
    pub from: String,
    pub to: String,
    pub message: Value,
}

// ========== Events ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTabEvent {
    pub tab_id: String,
    pub plugin_id: String,
    pub config: TabConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationEvent {
    pub plugin_id: String,
    pub notification: Notification,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterUiEvent {
    pub plugin_id: String,
    pub component: UiComponent,
}