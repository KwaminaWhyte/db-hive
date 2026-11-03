//! The host API surface plugins may reach, and its types.
//!
//! This is the Tauri shell's `plugins::api` with the event emitter removed.
//! Where it used to `app_handle.emit(...)` it now returns a value, because a
//! core crate has no window to emit into.
//!
//! Two of the old methods are gone rather than ported:
//!
//! * `execute_query` and `get_metadata` returned **mock rows**. A plugin that
//!   declares `executeQuery` today gets an explicit
//!   [`PluginError::NotSupported`] instead of two fake rows that look real.
//!   Wiring a real connection through here is a deliberate decision with
//!   consequences (a plugin would be able to read every row the user can),
//!   and it should be taken on purpose, not inherited from a placeholder.
//! * `register_ui_component` is replaced by the declarative descriptor in
//!   [`super::ui`], which the runtime parses and the host draws.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::{PluginError, PluginResult};
use super::manifest::PluginPermission;

/// Everything the API needs to know about the plugin it is acting for.
#[derive(Debug, Clone)]
pub struct PluginContext {
    pub plugin_id: String,
    pub permissions: Vec<PluginPermission>,
    pub config: Option<Value>,
    /// The plugin's own writable directory.
    pub data_dir: PathBuf,
}

pub struct PluginApi {
    context: PluginContext,
}

impl PluginApi {
    pub fn new(context: PluginContext) -> Self {
        Self { context }
    }

    pub fn context(&self) -> &PluginContext {
        &self.context
    }

    fn check(&self, permission: PluginPermission) -> PluginResult<()> {
        if self.context.permissions.contains(&permission) {
            Ok(())
        } else {
            Err(PluginError::PermissionDenied(
                permission.label().to_string(),
            ))
        }
    }

    // ---- Database -------------------------------------------------------

    /// Not available. See the module documentation.
    pub async fn execute_query(
        &self,
        _sql: &str,
        _connection_id: &str,
    ) -> PluginResult<QueryResult> {
        self.check(PluginPermission::ExecuteQuery)?;
        Err(PluginError::NotSupported(
            "database access from plugins".into(),
        ))
    }

    /// Not available. See the module documentation.
    pub async fn get_metadata(&self, _connection_id: &str) -> PluginResult<DatabaseMetadata> {
        self.check(PluginPermission::ReadMetadata)?;
        Err(PluginError::NotSupported(
            "database metadata from plugins".into(),
        ))
    }

    // ---- UI -------------------------------------------------------------

    /// Build a notification for the host to show.
    pub fn notification(&self, notification: Notification) -> PluginResult<Notification> {
        self.check(PluginPermission::ShowNotification)?;
        Ok(notification)
    }

    /// Validate a declarative interface a plugin wants shown.
    pub fn accept_ui(&self, raw: &str) -> PluginResult<super::ui::PluginUi> {
        self.check(PluginPermission::ModifyUi)?;
        super::ui::PluginUi::parse(raw).map_err(|e| PluginError::ExecutionError(e.to_string()))
    }

    // ---- Files ----------------------------------------------------------

    pub async fn read_file(&self, path: &str) -> PluginResult<String> {
        self.check(PluginPermission::ReadFiles)?;
        let safe = self.sandbox_path(path)?;
        Ok(tokio::fs::read_to_string(safe).await?)
    }

    pub async fn write_file(&self, path: &str, content: &str) -> PluginResult<()> {
        self.check(PluginPermission::WriteFiles)?;
        let safe = self.sandbox_path(path)?;
        if let Some(parent) = safe.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(safe, content).await?;
        Ok(())
    }

    /// Join a plugin-supplied path to its data directory, refusing anything
    /// that could leave it.
    fn sandbox_path(&self, path: &str) -> PluginResult<PathBuf> {
        if !super::manifest::is_safe_relative_path(path) {
            return Err(PluginError::Other("Invalid file path".to_string()));
        }
        Ok(self.context.data_dir.join(path))
    }

    // ---- Storage --------------------------------------------------------

    pub fn get_config(&self) -> Option<&Value> {
        self.context.config.as_ref()
    }

    pub async fn store_data(&self, key: &str, value: Value) -> PluginResult<()> {
        let path = self.sandbox_path(".storage.json")?;
        let mut storage: HashMap<String, Value> = match tokio::fs::read_to_string(&path).await {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => HashMap::new(),
        };
        storage.insert(key.to_string(), value);
        tokio::fs::write(&path, serde_json::to_string_pretty(&storage)?).await?;
        Ok(())
    }

    pub async fn get_data(&self, key: &str) -> PluginResult<Option<Value>> {
        let path = self.sandbox_path(".storage.json")?;
        let Ok(raw) = tokio::fs::read_to_string(&path).await else {
            return Ok(None);
        };
        let storage: HashMap<String, Value> = serde_json::from_str(&raw)?;
        Ok(storage.get(key).cloned())
    }
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub title: String,
    pub message: String,
    #[serde(default, rename = "notificationType")]
    pub kind: NotificationKind,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotificationKind {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

impl NotificationKind {
    /// Unknown values become `Info`: a plugin getting the type wrong should
    /// not lose the message.
    pub fn from_wire(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "success" => NotificationKind::Success,
            "warning" | "warn" => NotificationKind::Warning,
            "error" => NotificationKind::Error,
            _ => NotificationKind::Info,
        }
    }
}

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
pub struct PluginMessage {
    pub from: String,
    pub to: String,
    pub message: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api(permissions: Vec<PluginPermission>) -> PluginApi {
        PluginApi::new(PluginContext {
            plugin_id: "com.x.y".into(),
            permissions,
            config: None,
            data_dir: PathBuf::from("/tmp/dbhive-plugin-api-test"),
        })
    }

    #[test]
    fn undeclared_permissions_are_refused() {
        let a = api(vec![PluginPermission::ReadFiles]);
        assert!(a
            .notification(Notification {
                title: "t".into(),
                message: "m".into(),
                kind: NotificationKind::Info,
            })
            .is_err());
        assert!(a.accept_ui("{}").is_err());
    }

    #[test]
    fn declared_permissions_are_allowed() {
        let a = api(vec![
            PluginPermission::ShowNotification,
            PluginPermission::ModifyUi,
        ]);
        assert!(a
            .notification(Notification {
                title: "t".into(),
                message: "m".into(),
                kind: NotificationKind::Error,
            })
            .is_ok());
        assert!(a.accept_ui("{}").is_ok());
    }

    #[tokio::test]
    async fn database_access_is_refused_even_when_declared() {
        let a = api(vec![
            PluginPermission::ExecuteQuery,
            PluginPermission::ReadMetadata,
        ]);
        assert!(matches!(
            a.execute_query("SELECT 1", "conn").await,
            Err(PluginError::NotSupported(_))
        ));
        assert!(matches!(
            a.get_metadata("conn").await,
            Err(PluginError::NotSupported(_))
        ));
    }

    #[tokio::test]
    async fn file_access_cannot_leave_the_data_directory() {
        let a = api(vec![
            PluginPermission::ReadFiles,
            PluginPermission::WriteFiles,
        ]);
        assert!(a.read_file("../../etc/passwd").await.is_err());
        assert!(a.write_file("/etc/passwd", "x").await.is_err());
    }

    #[test]
    fn notification_kinds_parse_leniently() {
        assert_eq!(
            NotificationKind::from_wire("Success"),
            NotificationKind::Success
        );
        assert_eq!(
            NotificationKind::from_wire("warn"),
            NotificationKind::Warning
        );
        assert_eq!(
            NotificationKind::from_wire("nonsense"),
            NotificationKind::Info
        );
        assert_eq!(NotificationKind::from_wire(""), NotificationKind::Info);
    }
}
