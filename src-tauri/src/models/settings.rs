//! Application settings model
//!
//! Defines the structure for application-wide settings including
//! general preferences, theme configuration, query execution options,
//! and keyboard shortcuts.

use serde::{Deserialize, Serialize};

/// Application settings
///
/// Central configuration for all user preferences and application behavior.
/// Settings are persisted to disk using Tauri Store and loaded on application startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// General application settings
    pub general: GeneralSettings,

    /// Theme and appearance settings
    pub theme: ThemeSettings,

    /// Query execution settings
    pub query: QuerySettings,

    /// Keyboard shortcuts configuration
    pub shortcuts: ShortcutsSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            general: GeneralSettings::default(),
            theme: ThemeSettings::default(),
            query: QuerySettings::default(),
            shortcuts: ShortcutsSettings::default(),
        }
    }
}

/// General application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralSettings {
    /// Language code (e.g., "en", "es", "fr")
    pub language: String,

    /// Default database to connect to on startup (connection profile ID)
    pub default_database: Option<String>,

    /// Behavior when application starts
    pub startup_behavior: StartupBehavior,

    /// Enable auto-save for connection profiles
    pub auto_save_connections: bool,

    /// Enable telemetry and crash reporting
    pub enable_telemetry: bool,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            language: "en".to_string(),
            default_database: None,
            startup_behavior: StartupBehavior::ShowConnectionList,
            auto_save_connections: true,
            enable_telemetry: false,
        }
    }
}

/// Application startup behavior
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StartupBehavior {
    /// Show connection list (default)
    ShowConnectionList,

    /// Open the last used connection
    OpenLastConnection,

    /// Open a specific connection (uses default_database)
    OpenDefaultConnection,

    /// Show query editor with no connection
    ShowQueryEditor,
}

/// Theme and appearance settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeSettings {
    /// Theme mode (light, dark, system)
    pub mode: ThemeMode,

    /// Accent color (hex color code, e.g., "#f59e0b" for amber)
    pub accent_color: String,

    /// Font size for SQL editor (in pixels)
    pub editor_font_size: u32,

    /// Font family for SQL editor
    pub editor_font_family: String,

    /// Enable line numbers in SQL editor
    pub editor_line_numbers: bool,

    /// Enable minimap in SQL editor
    pub editor_minimap: bool,

    /// Word wrap in SQL editor
    pub editor_word_wrap: bool,
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self {
            mode: ThemeMode::System,
            accent_color: "#f59e0b".to_string(), // Amber/Honey gold
            editor_font_size: 14,
            editor_font_family: "Monaco, 'Courier New', monospace".to_string(),
            editor_line_numbers: true,
            editor_minimap: false,
            editor_word_wrap: false,
        }
    }
}

/// Theme mode options
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    /// Light theme
    Light,

    /// Dark theme
    Dark,

    /// Follow system preference
    System,
}

/// Query execution settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuerySettings {
    /// Query execution timeout in seconds (0 = no timeout)
    pub timeout_seconds: u32,

    /// Maximum number of rows to fetch per query
    pub max_rows: u32,

    /// Enable auto-commit (applies changes immediately)
    pub auto_commit: bool,

    /// Confirm before running DELETE or DROP statements
    pub confirm_destructive: bool,

    /// Save query to history automatically
    pub auto_save_history: bool,

    /// Maximum number of history entries to keep
    pub max_history_entries: u32,

    /// Format SQL automatically before execution
    pub auto_format_sql: bool,
}

impl Default for QuerySettings {
    fn default() -> Self {
        Self {
            timeout_seconds: 30,
            max_rows: 1000,
            auto_commit: false,
            confirm_destructive: true,
            auto_save_history: true,
            max_history_entries: 500,
            auto_format_sql: false,
        }
    }
}

/// Keyboard shortcuts configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutsSettings {
    /// Execute query (default: Ctrl/Cmd+Enter)
    pub execute_query: String,

    /// Clear editor (default: Ctrl/Cmd+K)
    pub clear_editor: String,

    /// Open new tab (default: Ctrl/Cmd+T)
    pub new_tab: String,

    /// Close tab (default: Ctrl/Cmd+W)
    pub close_tab: String,

    /// Save snippet (default: Ctrl/Cmd+S)
    pub save_snippet: String,

    /// Open settings (default: Ctrl/Cmd+,)
    pub open_settings: String,

    /// Toggle sidebar (default: Ctrl/Cmd+B)
    pub toggle_sidebar: String,

    /// Search (default: Ctrl/Cmd+F)
    pub search: String,

    /// Format SQL (default: Ctrl/Cmd+Shift+F)
    pub format_sql: String,

    /// Show keyboard shortcuts (default: ?)
    pub show_shortcuts: String,
}

impl Default for ShortcutsSettings {
    fn default() -> Self {
        Self {
            execute_query: "Ctrl+Enter".to_string(),
            clear_editor: "Ctrl+K".to_string(),
            new_tab: "Ctrl+T".to_string(),
            close_tab: "Ctrl+W".to_string(),
            save_snippet: "Ctrl+S".to_string(),
            open_settings: "Ctrl+,".to_string(),
            toggle_sidebar: "Ctrl+B".to_string(),
            search: "Ctrl+F".to_string(),
            format_sql: "Ctrl+Shift+F".to_string(),
            show_shortcuts: "?".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_settings_default() {
        let settings = AppSettings::default();
        assert_eq!(settings.general.language, "en");
        assert_eq!(settings.theme.mode, ThemeMode::System);
        assert_eq!(settings.query.timeout_seconds, 30);
        assert_eq!(settings.shortcuts.execute_query, "Ctrl+Enter");
    }

    #[test]
    fn test_general_settings_default() {
        let general = GeneralSettings::default();
        assert_eq!(general.language, "en");
        assert_eq!(general.startup_behavior, StartupBehavior::ShowConnectionList);
        assert!(general.auto_save_connections);
        assert!(!general.enable_telemetry);
    }

    #[test]
    fn test_theme_settings_default() {
        let theme = ThemeSettings::default();
        assert_eq!(theme.mode, ThemeMode::System);
        assert_eq!(theme.accent_color, "#f59e0b");
        assert_eq!(theme.editor_font_size, 14);
        assert!(theme.editor_line_numbers);
        assert!(!theme.editor_minimap);
    }

    #[test]
    fn test_query_settings_default() {
        let query = QuerySettings::default();
        assert_eq!(query.timeout_seconds, 30);
        assert_eq!(query.max_rows, 1000);
        assert!(!query.auto_commit);
        assert!(query.confirm_destructive);
        assert!(query.auto_save_history);
    }

    #[test]
    fn test_shortcuts_settings_default() {
        let shortcuts = ShortcutsSettings::default();
        assert_eq!(shortcuts.execute_query, "Ctrl+Enter");
        assert_eq!(shortcuts.clear_editor, "Ctrl+K");
        assert_eq!(shortcuts.save_snippet, "Ctrl+S");
    }

    #[test]
    fn test_serialization() {
        let settings = AppSettings::default();
        let json = serde_json::to_string(&settings).unwrap();
        let deserialized: AppSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(settings.general.language, deserialized.general.language);
        assert_eq!(settings.theme.mode, deserialized.theme.mode);
        assert_eq!(settings.query.timeout_seconds, deserialized.query.timeout_seconds);
    }
}
