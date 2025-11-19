//! Connection management commands
//!
//! This module provides Tauri commands for managing database connections and profiles.
//! It handles testing connections, creating/updating/deleting profiles, and establishing
//! active database connections.

use std::sync::{Arc, Mutex};
use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::drivers::{postgres::PostgresDriver, sqlite::SqliteDriver, ConnectionOptions, DatabaseDriver};
use crate::models::{ConnectionProfile, ConnectionStatus, DbDriver, DbError};
use crate::state::AppState;

/// Test a database connection without saving it
///
/// This command attempts to establish a connection to the database using the
/// provided profile settings. It does not save the profile or maintain the
/// connection after testing.
///
/// # Arguments
///
/// * `profile` - Connection profile with database settings
/// * `password` - Password for authentication (not stored in profile)
///
/// # Returns
///
/// Returns `ConnectionStatus::Connected` if successful, or an error if the
/// connection fails.
///
/// # Notes
///
/// Currently only PostgreSQL is supported. Other database drivers will return
/// an error indicating they are not yet implemented.
#[tauri::command]
pub async fn test_connection_command(
    profile: ConnectionProfile,
    password: String,
) -> Result<ConnectionStatus, DbError> {
    // Build connection options from profile
    let opts = ConnectionOptions {
        host: profile.host.clone(),
        port: profile.port,
        username: profile.username.clone(),
        password: Some(password),
        database: profile.database.clone(),
        timeout: Some(30),
    };

    // Test connection based on driver type
    match profile.driver {
        DbDriver::Postgres => {
            let driver = PostgresDriver::connect(opts).await?;
            driver.test_connection().await?;
            Ok(ConnectionStatus::Connected)
        }
        DbDriver::Sqlite => {
            let driver = SqliteDriver::connect(opts).await?;
            driver.test_connection().await?;
            Ok(ConnectionStatus::Connected)
        }
        DbDriver::MySql => Err(DbError::InternalError(
            "MySQL driver not yet implemented".to_string(),
        )),
        DbDriver::MongoDb => Err(DbError::InternalError(
            "MongoDB driver not yet implemented".to_string(),
        )),
        DbDriver::SqlServer => Err(DbError::InternalError(
            "SQL Server driver not yet implemented".to_string(),
        )),
    }
}

/// Create a new connection profile
///
/// This command saves a connection profile to the application state for later use.
/// If the profile ID is empty, a new UUID will be generated.
///
/// # Arguments
///
/// * `profile` - Connection profile to save
/// * `state` - Application state
///
/// # Returns
///
/// Returns the profile ID (either the provided ID or a newly generated UUID)
///
/// # Notes
///
/// Passwords should be stored in the OS keyring separately and referenced
/// via `password_keyring_key` in the profile.
#[tauri::command]
pub fn create_connection_profile(
    mut profile: ConnectionProfile,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<String, DbError> {
    // Generate UUID if ID is empty
    if profile.id.is_empty() {
        profile.id = Uuid::new_v4().to_string();
    }

    let profile_id = profile.id.clone();

    // Check if profile already exists and add to state
    let mut state = state.lock().unwrap();
    if state.get_profile(&profile_id).is_some() {
        return Err(DbError::InvalidInput(format!(
            "Profile with ID {} already exists",
            profile_id
        )));
    }

    // Add profile to state
    state.add_profile(profile);

    // Save profiles to persistent storage
    state.save_profiles_to_store(&app)?;

    Ok(profile_id)
}

/// Update an existing connection profile
///
/// This command updates a connection profile in the application state.
/// The profile ID cannot be changed.
///
/// # Arguments
///
/// * `profile` - Updated connection profile
/// * `state` - Application state
///
/// # Returns
///
/// Returns `Ok(())` if successful, or an error if the profile doesn't exist
#[tauri::command]
pub fn update_connection_profile(
    profile: ConnectionProfile,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<(), DbError> {
    let mut state = state.lock().unwrap();

    // Check if profile exists
    if state.get_profile(&profile.id).is_none() {
        return Err(DbError::NotFound(format!(
            "Profile with ID {} not found",
            profile.id
        )));
    }

    // Update profile
    state.add_profile(profile);

    // Save profiles to persistent storage
    state.save_profiles_to_store(&app)?;

    Ok(())
}

/// Delete a connection profile
///
/// This command deletes a connection profile from the application state.
/// If there is an active connection using this profile, it will be closed first.
///
/// # Arguments
///
/// * `profile_id` - ID of the profile to delete
/// * `state` - Application state
///
/// # Returns
///
/// Returns `Ok(())` if successful, or an error if the profile doesn't exist
#[tauri::command]
pub async fn delete_connection_profile(
    profile_id: String,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<(), DbError> {
    // Check if profile exists and get connection if present
    let connection = {
        let mut state_guard = state.lock().unwrap();

        // Check if profile exists
        if state_guard.get_profile(&profile_id).is_none() {
            return Err(DbError::NotFound(format!(
                "Profile with ID {} not found",
                profile_id
            )));
        }

        // Remove active connection if it exists (we'll close it after releasing the lock)
        state_guard.remove_connection(&profile_id)
    };

    // Close connection outside of the lock
    if let Some(conn) = connection {
        conn.close().await?;
    }

    // Remove profile and password, then save to store
    {
        let mut state_guard = state.lock().unwrap();
        state_guard.remove_profile(&profile_id);
        state_guard.connection_passwords.remove(&profile_id);

        // Save profiles and passwords to persistent storage
        state_guard.save_profiles_to_store(&app)?;
        state_guard.save_passwords_to_store(&app)?;
    }

    Ok(())
}

/// List all saved connection profiles
///
/// This command retrieves all connection profiles from the application state.
///
/// # Arguments
///
/// * `state` - Application state
///
/// # Returns
///
/// Returns a vector of all saved connection profiles
#[tauri::command]
pub fn list_connection_profiles(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<ConnectionProfile>, DbError> {
    let state = state.lock().unwrap();
    let profiles = state.list_profiles();

    // Convert from Vec<&ConnectionProfile> to Vec<ConnectionProfile>
    Ok(profiles.into_iter().cloned().collect())
}

/// Get saved password for a connection profile
///
/// This command retrieves the saved password for a profile if one exists.
///
/// # Arguments
///
/// * `profile_id` - ID of the profile
/// * `state` - Application state
///
/// # Returns
///
/// Returns the saved password if it exists, otherwise None
///
/// # Security Note
///
/// Passwords are currently stored in plaintext in the persistent store.
/// This is a temporary solution and should be replaced with OS keyring storage.
#[tauri::command]
pub fn get_saved_password(
    profile_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Option<String>, DbError> {
    let state = state.lock().unwrap();
    Ok(state.connection_passwords.get(&profile_id).cloned())
}

/// Save password for a connection profile
///
/// This command saves a password for a profile to the persistent store.
///
/// # Arguments
///
/// * `profile_id` - ID of the profile
/// * `password` - Password to save
/// * `state` - Application state
/// * `app` - Application handle
///
/// # Returns
///
/// Returns Ok(()) if successful
///
/// # Security Note
///
/// Passwords are currently stored in plaintext in the persistent store.
/// This is a temporary solution and should be replaced with OS keyring storage.
#[tauri::command]
pub fn save_password(
    profile_id: String,
    password: String,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<(), DbError> {
    let mut state = state.lock().unwrap();
    state.connection_passwords.insert(profile_id, password);
    state.save_passwords_to_store(&app)?;
    Ok(())
}

/// Connect to a database using a saved profile
///
/// This command establishes an active database connection using the credentials
/// from a saved profile. The connection is stored in the application state and
/// can be used for subsequent operations.
///
/// # Arguments
///
/// * `profile_id` - ID of the profile to use for connection
/// * `password` - Password for authentication
/// * `state` - Application state
///
/// # Returns
///
/// Returns the connection ID (same as profile ID) if successful
///
/// # Notes
///
/// Currently only PostgreSQL is supported. If a connection already exists for
/// this profile, it will be replaced.
#[tauri::command]
pub async fn connect_to_database(
    profile_id: String,
    password: String,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<String, DbError> {
    // Get the profile from state
    let profile = {
        let state = state.lock().unwrap();
        state
            .get_profile(&profile_id)
            .ok_or_else(|| {
                DbError::NotFound(format!("Profile with ID {} not found", profile_id))
            })?
            .clone()
    };

    // Build connection options from profile
    let opts = ConnectionOptions {
        host: profile.host.clone(),
        port: profile.port,
        username: profile.username.clone(),
        password: Some(password.clone()),
        database: profile.database.clone(),
        timeout: Some(30),
    };

    // Connect based on driver type
    let connection: Arc<dyn DatabaseDriver> = match profile.driver {
        DbDriver::Postgres => {
            let driver = PostgresDriver::connect(opts).await?;
            Arc::new(driver)
        }
        DbDriver::Sqlite => {
            let driver = SqliteDriver::connect(opts).await?;
            Arc::new(driver)
        }
        DbDriver::MySql => {
            return Err(DbError::InternalError(
                "MySQL driver not yet implemented".to_string(),
            ))
        }
        DbDriver::MongoDb => {
            return Err(DbError::InternalError(
                "MongoDB driver not yet implemented".to_string(),
            ))
        }
        DbDriver::SqlServer => {
            return Err(DbError::InternalError(
                "SQL Server driver not yet implemented".to_string(),
            ))
        }
    };

    // Store connection and password in state
    {
        let mut state = state.lock().unwrap();
        state.add_connection(profile_id.clone(), connection);
        state.connection_passwords.insert(profile_id.clone(), password);

        // Save password to persistent storage
        state.save_passwords_to_store(&app)?;
    }

    Ok(profile_id)
}

/// Disconnect from a database
///
/// This command closes an active database connection and removes it from
/// the application state.
///
/// # Arguments
///
/// * `connection_id` - ID of the connection to close
/// * `state` - Application state
///
/// # Returns
///
/// Returns `Ok(())` if successful, or an error if the connection doesn't exist
#[tauri::command]
pub async fn disconnect_from_database(
    connection_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), DbError> {
    // Remove connection from state (but keep password for reconnection)
    let connection = {
        let mut state = state.lock().unwrap();
        // Note: We no longer clear connection_passwords here to allow easy reconnection
        state
            .remove_connection(&connection_id)
            .ok_or_else(|| {
                DbError::NotFound(format!("Connection with ID {} not found", connection_id))
            })?
    };

    // Close the connection
    connection.close().await?;

    Ok(())
}

/// Switch to a different database using the same connection credentials
///
/// This command creates a new connection to a different database on the same server,
/// reusing the credentials from the existing connection. This is useful when no
/// specific database was configured in the profile and the user wants to browse
/// different databases.
///
/// # Arguments
///
/// * `connection_id` - ID of the current connection
/// * `new_database` - Name of the database to switch to
/// * `state` - Application state
///
/// # Returns
///
/// Returns the connection ID if successful
///
/// # Notes
///
/// This maintains the same connection ID but creates a new underlying connection
/// to the different database.
#[tauri::command]
pub async fn switch_database(
    connection_id: String,
    new_database: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, DbError> {
    // Get the profile and password from state
    let (profile, password) = {
        let state_guard = state.lock().unwrap();

        // Get the connection to ensure it exists
        let _connection = state_guard
            .get_connection(&connection_id)
            .ok_or_else(|| {
                DbError::NotFound(format!("Connection with ID {} not found", connection_id))
            })?;

        // Get the profile
        let profile = state_guard
            .get_profile(&connection_id)
            .ok_or_else(|| {
                DbError::NotFound(format!("Profile for connection {} not found", connection_id))
            })?
            .clone();

        // Get the stored password
        let password = state_guard
            .connection_passwords
            .get(&connection_id)
            .ok_or_else(|| {
                DbError::AuthError("Password not found for connection".to_string())
            })?
            .clone();

        (profile, password)
    };

    // Build connection options with the new database
    let opts = ConnectionOptions {
        host: profile.host.clone(),
        port: profile.port,
        username: profile.username.clone(),
        password: Some(password.clone()),
        database: Some(new_database.clone()),
        timeout: Some(30),
    };

    // Connect to the new database based on driver type
    let new_connection: Arc<dyn DatabaseDriver> = match profile.driver {
        DbDriver::Postgres => {
            let driver = PostgresDriver::connect(opts).await?;
            Arc::new(driver)
        }
        DbDriver::Sqlite => {
            let driver = SqliteDriver::connect(opts).await?;
            Arc::new(driver)
        }
        _ => {
            return Err(DbError::InternalError(
                "Database switching only supported for PostgreSQL and SQLite currently".to_string(),
            ))
        }
    };

    // Close the old connection
    let old_connection = {
        let mut state_guard = state.lock().unwrap();
        state_guard.remove_connection(&connection_id)
    };

    if let Some(old_connection) = old_connection {
        // Try to close but don't fail if it errors
        let _ = old_connection.close().await;
    }

    // Store the new connection with the same ID
    {
        let mut state_guard = state.lock().unwrap();
        state_guard.add_connection(connection_id.clone(), new_connection);
        // Password is already stored, no need to update it
    }

    Ok(connection_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SslMode;

    fn create_test_profile(id: &str, name: &str) -> ConnectionProfile {
        ConnectionProfile::new(
            id.to_string(),
            name.to_string(),
            DbDriver::Postgres,
            "localhost".to_string(),
            5432,
            "postgres".to_string(),
        )
    }

    // Helper functions for testing that work directly with Mutex<AppState>
    fn test_create_profile(
        profile: &mut ConnectionProfile,
        state: &Mutex<AppState>,
    ) -> Result<String, DbError> {
        // Generate UUID if ID is empty
        if profile.id.is_empty() {
            profile.id = Uuid::new_v4().to_string();
        }

        let profile_id = profile.id.clone();

        // Check if profile already exists
        let mut state = state.lock().unwrap();
        if state.get_profile(&profile_id).is_some() {
            return Err(DbError::InvalidInput(format!(
                "Profile with ID {} already exists",
                profile_id
            )));
        }

        // Add profile to state
        state.add_profile(profile.clone());

        Ok(profile_id)
    }

    fn test_update_profile(
        profile: &ConnectionProfile,
        state: &Mutex<AppState>,
    ) -> Result<(), DbError> {
        let mut state = state.lock().unwrap();

        // Check if profile exists
        if state.get_profile(&profile.id).is_none() {
            return Err(DbError::NotFound(format!(
                "Profile with ID {} not found",
                profile.id
            )));
        }

        // Update profile
        state.add_profile(profile.clone());

        Ok(())
    }

    fn test_list_profiles(state: &Mutex<AppState>) -> Result<Vec<ConnectionProfile>, DbError> {
        let state = state.lock().unwrap();
        let profiles = state.list_profiles();
        Ok(profiles.into_iter().cloned().collect())
    }

    #[test]
    fn test_create_connection_profile_generates_uuid() {
        let state = Mutex::new(AppState::default());
        let mut profile = create_test_profile("", "Test DB");

        let result = test_create_profile(&mut profile, &state);

        assert!(result.is_ok());
        let profile_id = result.unwrap();
        assert!(!profile_id.is_empty());
        assert_ne!(profile_id, "");

        // Verify it was saved with the generated ID
        let state = state.lock().unwrap();
        assert!(state.get_profile(&profile_id).is_some());
    }

    #[test]
    fn test_create_connection_profile_with_id() {
        let state = Mutex::new(AppState::default());
        let mut profile = create_test_profile("test-123", "Test DB");

        let result = test_create_profile(&mut profile, &state);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test-123");

        let state = state.lock().unwrap();
        assert!(state.get_profile("test-123").is_some());
    }

    #[test]
    fn test_create_duplicate_profile_fails() {
        let state = Mutex::new(AppState::default());
        let mut profile = create_test_profile("test-123", "Test DB");

        // Create first profile
        let result1 = test_create_profile(&mut profile, &state);
        assert!(result1.is_ok());

        // Try to create duplicate
        let mut profile2 = profile.clone();
        let result2 = test_create_profile(&mut profile2, &state);
        assert!(result2.is_err());
        assert!(matches!(result2.unwrap_err(), DbError::InvalidInput(_)));
    }

    #[test]
    fn test_update_connection_profile() {
        let state = Mutex::new(AppState::default());
        let mut profile = create_test_profile("test-123", "Original Name");

        // Create profile
        test_create_profile(&mut profile, &state).unwrap();

        // Update profile
        profile.name = "Updated Name".to_string();
        let result = test_update_profile(&profile, &state);

        assert!(result.is_ok());

        let state = state.lock().unwrap();
        let saved_profile = state.get_profile("test-123").unwrap();
        assert_eq!(saved_profile.name, "Updated Name");
    }

    #[test]
    fn test_update_nonexistent_profile_fails() {
        let state = Mutex::new(AppState::default());
        let profile = create_test_profile("nonexistent", "Test");

        let result = test_update_profile(&profile, &state);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DbError::NotFound(_)));
    }

    #[test]
    fn test_list_connection_profiles() {
        let state = Mutex::new(AppState::default());

        // Create multiple profiles
        let mut profile1 = create_test_profile("test-1", "DB 1");
        let mut profile2 = create_test_profile("test-2", "DB 2");
        let mut profile3 = create_test_profile("test-3", "DB 3");

        test_create_profile(&mut profile1, &state).unwrap();
        test_create_profile(&mut profile2, &state).unwrap();
        test_create_profile(&mut profile3, &state).unwrap();

        // List profiles
        let result = test_list_profiles(&state);

        assert!(result.is_ok());
        let profiles = result.unwrap();
        assert_eq!(profiles.len(), 3);
    }
}
