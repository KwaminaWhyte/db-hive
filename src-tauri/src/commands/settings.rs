//! Settings commands
//!
//! Tauri commands for managing application settings including loading,
//! updating, and resetting settings to defaults.

use crate::models::{AppSettings, DbError};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

/// Get current application settings
///
/// Loads settings from persistent storage. If no settings exist,
/// returns default settings.
///
/// # Arguments
///
/// * `app` - Tauri application handle
///
/// # Returns
///
/// Current application settings or defaults if not found
#[tauri::command]
pub async fn get_settings(app: AppHandle) -> Result<AppSettings, DbError> {
    let store = app
        .store("settings.json")
        .map_err(|e| DbError::InternalError(format!("Failed to access settings store: {}", e)))?;

    // Try to load settings from store
    if let Some(settings_value) = store.get("settings") {
        // Deserialize settings
        let settings: AppSettings = serde_json::from_value(settings_value.clone())
            .map_err(|e| DbError::InternalError(format!("Failed to deserialize settings: {}", e)))?;

        Ok(settings)
    } else {
        // No settings found, return defaults
        Ok(AppSettings::default())
    }
}

/// Update application settings
///
/// Saves the provided settings to persistent storage.
///
/// # Arguments
///
/// * `app` - Tauri application handle
/// * `settings` - Updated settings to save
///
/// # Returns
///
/// Ok(()) if settings were saved successfully
#[tauri::command]
pub async fn update_settings(app: AppHandle, settings: AppSettings) -> Result<(), DbError> {
    let store = app
        .store("settings.json")
        .map_err(|e| DbError::InternalError(format!("Failed to access settings store: {}", e)))?;

    // Serialize settings
    let settings_value = serde_json::to_value(&settings)
        .map_err(|e| DbError::InternalError(format!("Failed to serialize settings: {}", e)))?;

    // Save to store
    store.set("settings", settings_value);

    // Persist to disk
    store
        .save()
        .map_err(|e| DbError::InternalError(format!("Failed to persist settings: {}", e)))?;

    Ok(())
}

/// Reset settings to defaults
///
/// Replaces current settings with default values and saves them.
///
/// # Arguments
///
/// * `app` - Tauri application handle
///
/// # Returns
///
/// The default settings that were saved
#[tauri::command]
pub async fn reset_settings(app: AppHandle) -> Result<AppSettings, DbError> {
    let defaults = AppSettings::default();
    update_settings(app, defaults.clone()).await?;
    Ok(defaults)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings_valid() {
        // Ensure default settings can be serialized/deserialized
        let settings = AppSettings::default();
        let json = serde_json::to_string(&settings).unwrap();
        let deserialized: AppSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(settings.general.language, deserialized.general.language);
        assert_eq!(settings.theme.accent_color, deserialized.theme.accent_color);
    }
}
