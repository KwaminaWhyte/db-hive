//! Connection management commands
//!
//! This module provides Tauri commands for managing database connections and profiles.
//! It handles testing connections, creating/updating/deleting profiles, and establishing
//! active database connections.

use std::sync::{Arc, Mutex};
use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::drivers::{mongodb::MongoDbDriver, mysql::MysqlDriver, postgres::PostgresDriver, sqlite::SqliteDriver, sqlserver::SqlServerDriver, ConnectionOptions, DatabaseDriver};
use crate::models::{ConnectionProfile, ConnectionStatus, DbDriver, DbError};
use crate::state::AppState;

/// Test a database connection without saving it
///
/// This command attempts to establish a connection to the database using the
/// provided profile settings. It does not save the profile or maintain the
/// connection after testing. If SSH tunnel is configured, it will create a
/// temporary tunnel for the test and clean it up afterward.
///
/// # Arguments
///
/// * `profile` - Connection profile with database settings
/// * `password` - Password for database authentication (not stored in profile)
/// * `ssh_password` - Optional password for SSH authentication (when using password auth method)
/// * `state` - Application state (for SSH tunnel manager)
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
    ssh_password: Option<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<ConnectionStatus, DbError> {
    // Check if SSH tunnel is configured
    let (actual_host, actual_port, temp_tunnel_id) = if let Some(ssh_config) = &profile.ssh_tunnel {
        // Create temporary SSH tunnel for testing
        let temp_id = format!("test-{}", Uuid::new_v4());

        let ssh_auth_password = match ssh_config.auth_method {
            crate::models::connection::SshAuthMethod::Password => ssh_password.clone(),
            crate::models::connection::SshAuthMethod::PrivateKey => None,
        };

        let local_port = {
            let tunnel_manager = {
                let state_guard = state.lock().unwrap();
                state_guard.ssh_tunnel_manager.clone()
            };

            tunnel_manager
                .create_tunnel(
                    temp_id.clone(),
                    ssh_config,
                    ssh_auth_password,
                    profile.host.clone(),
                    profile.port,
                )
                .await?
        };

        // Connect to localhost:local_port instead of the original host:port
        ("127.0.0.1".to_string(), local_port, Some(temp_id))
    } else {
        // No SSH tunnel, use direct connection
        (profile.host.clone(), profile.port, None)
    };

    // Build connection options from profile
    let opts = ConnectionOptions {
        host: actual_host,
        port: actual_port,
        username: profile.username.clone(),
        password: Some(password),
        database: profile.database.clone(),
        timeout: Some(30),
    };

    // Test connection based on driver type
    let result = match profile.driver {
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
        DbDriver::MySql => {
            let driver = MysqlDriver::connect(opts).await?;
            driver.test_connection().await?;
            Ok(ConnectionStatus::Connected)
        }
        DbDriver::MongoDb => {
            let driver = MongoDbDriver::connect(opts).await?;
            driver.test_connection().await?;
            Ok(ConnectionStatus::Connected)
        }
        DbDriver::SqlServer => {
            let driver = SqlServerDriver::connect(opts).await?;
            driver.test_connection().await?;
            Ok(ConnectionStatus::Connected)
        }
    };

    // Clean up temporary SSH tunnel if it was created
    if let Some(tunnel_id) = temp_tunnel_id {
        let tunnel_manager = {
            let state_guard = state.lock().unwrap();
            state_guard.ssh_tunnel_manager.clone()
        };

        if tunnel_manager.has_tunnel(&tunnel_id).await {
            let _ = tunnel_manager.close_tunnel(&tunnel_id).await;
        }
    }

    result
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

    // Delete password from OS keyring
    crate::credentials::CredentialManager::delete_password(&profile_id)?;

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
    // Try to get password from OS keyring first
    match crate::credentials::CredentialManager::get_password(&profile_id)? {
        Some(password) => Ok(Some(password)),
        None => {
            // Fallback to in-memory store for migration purposes
            let state = state.lock().unwrap();
            Ok(state.connection_passwords.get(&profile_id).cloned())
        }
    }
}

/// Save password for a connection profile
///
/// This command saves a password for a profile to the OS keyring.
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
/// Passwords are now stored securely in the OS keyring:
/// - macOS: Keychain
/// - Windows: Credential Manager
/// - Linux: Secret Service API (libsecret)
#[tauri::command]
pub fn save_password(
    profile_id: String,
    password: String,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<(), DbError> {
    // Save to OS keyring
    crate::credentials::CredentialManager::save_password(&profile_id, &password)?;

    // Remove password from in-memory store (we now use keyring)
    let mut state = state.lock().unwrap();
    state.connection_passwords.remove(&profile_id);

    // Save the updated passwords (without this one) to the store file
    // This ensures we remove any existing plaintext password
    state.save_passwords_to_store(&app)?;

    Ok(())
}

/// Save SSH password to OS keyring
///
/// This command securely stores an SSH password in the OS keyring for later retrieval.
/// The password is associated with a connection profile ID.
///
/// # Arguments
///
/// * `profile_id` - Connection profile ID
/// * `ssh_password` - SSH password to store
///
/// # Returns
///
/// Returns Ok(()) if successful
///
/// # Security Note
///
/// SSH passwords are stored securely in the OS keyring with a "-ssh" suffix:
/// - macOS: Keychain
/// - Windows: Credential Manager
/// - Linux: Secret Service API (libsecret)
#[tauri::command]
pub fn save_ssh_password(
    profile_id: String,
    ssh_password: String,
) -> Result<(), DbError> {
    // Save to OS keyring
    crate::credentials::CredentialManager::save_ssh_password(&profile_id, &ssh_password)?;
    Ok(())
}

/// Get saved SSH password for a profile
///
/// This command retrieves an SSH password from the OS keyring.
///
/// # Arguments
///
/// * `profile_id` - Connection profile ID
///
/// # Returns
///
/// Returns `Some(password)` if found, `None` if not found
#[tauri::command]
pub fn get_ssh_password(profile_id: String) -> Result<Option<String>, DbError> {
    crate::credentials::CredentialManager::get_ssh_password(&profile_id)
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
/// * `password` - Password for database authentication
/// * `ssh_password` - Optional password for SSH authentication (when using password auth method)
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
    ssh_password: Option<String>,
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

    // Check if SSH tunnel is configured
    let (actual_host, actual_port) = if let Some(ssh_config) = &profile.ssh_tunnel {
        // Create SSH tunnel
        let tunnel_manager = {
            let state_guard = state.lock().unwrap();
            state_guard.ssh_tunnel_manager.clone()
        };

        // Use SSH password parameter for password auth, none for private key auth
        let ssh_auth_password = match ssh_config.auth_method {
            crate::models::connection::SshAuthMethod::Password => ssh_password.clone(),
            crate::models::connection::SshAuthMethod::PrivateKey => None,
        };

        let local_port = tunnel_manager
            .create_tunnel(
                profile_id.clone(),
                ssh_config,
                ssh_auth_password,
                profile.host.clone(),
                profile.port,
            )
            .await?;

        // Connect to localhost:local_port instead of the original host:port
        ("127.0.0.1".to_string(), local_port)
    } else {
        // No SSH tunnel, use direct connection
        (profile.host.clone(), profile.port)
    };

    // Build connection options from profile
    let opts = ConnectionOptions {
        host: actual_host,
        port: actual_port,
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
            let driver = MysqlDriver::connect(opts).await?;
            Arc::new(driver)
        }
        DbDriver::MongoDb => {
            let driver = MongoDbDriver::connect(opts).await?;
            Arc::new(driver)
        }
        DbDriver::SqlServer => {
            let driver = SqlServerDriver::connect(opts).await?;
            Arc::new(driver)
        }
    };

    // Store connection and password in state
    {
        let mut state = state.lock().unwrap();
        state.add_connection(profile_id.clone(), connection);
        state.connection_passwords.insert(profile_id.clone(), password.clone());

        // Save password to persistent storage
        state.save_passwords_to_store(&app)?;
    }

    // Also save password to OS keyring for next time
    // This ensures password is saved even if the initial save from frontend failed
    if !password.is_empty() {
        if let Err(e) = crate::credentials::CredentialManager::save_password(&profile_id, &password) {
            eprintln!("Warning: Failed to save password to keyring: {}", e);
            // Don't fail the connection if keyring save fails
        }
    }

    // Save SSH password to keyring if provided
    if let Some(ref ssh_pwd) = ssh_password {
        if !ssh_pwd.is_empty() {
            if let Err(e) = crate::credentials::CredentialManager::save_ssh_password(&profile_id, ssh_pwd) {
                eprintln!("Warning: Failed to save SSH password to keyring: {}", e);
                // Don't fail the connection if keyring save fails
            }
        }
    }

    // Record successful connection (update metadata)
    if let Err(e) = record_connection(profile_id.clone(), state, app.clone()) {
        eprintln!("Warning: Failed to record connection metadata: {}", e);
        // Don't fail the connection if metadata recording fails
    }

    Ok(profile_id)
}

/// Disconnect from a database
///
/// This command closes an active database connection and removes it from
/// the application state. If an SSH tunnel was created for this connection,
/// it will also be closed.
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

    // Close SSH tunnel if one exists
    {
        let tunnel_manager = {
            let state_guard = state.lock().unwrap();
            state_guard.ssh_tunnel_manager.clone()
        };

        if tunnel_manager.has_tunnel(&connection_id).await {
            tunnel_manager.close_tunnel(&connection_id).await?;
        }
    }

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
        DbDriver::MySql => {
            let driver = MysqlDriver::connect(opts).await?;
            Arc::new(driver)
        }
        DbDriver::MongoDb => {
            let driver = MongoDbDriver::connect(opts).await?;
            Arc::new(driver)
        }
        _ => {
            return Err(DbError::InternalError(
                "Database switching only supported for PostgreSQL, MySQL, SQLite, and MongoDB currently".to_string(),
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

/// Record a successful connection (update metadata)
///
/// This command updates connection metadata after a successful connection,
/// including incrementing the connection count and updating the last connected timestamp.
///
/// # Arguments
///
/// * `profile_id` - ID of the profile that was connected
/// * `state` - Application state
/// * `app` - Application handle
///
/// # Returns
///
/// Returns `Ok(())` if successful
#[tauri::command]
pub fn record_connection(
    profile_id: String,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<(), DbError> {
    let mut state_guard = state.lock().unwrap();

    // Find the profile and update metadata
    if let Some(profile) = state_guard.get_profile_mut(&profile_id) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        profile.last_connected_at = Some(now);
        profile.connection_count += 1;
        profile.updated_at = now;

        // Save updated profiles
        drop(state_guard); // Release lock before saving
        let state_guard = state.lock().unwrap();
        state_guard.save_profiles_to_store(&app)?;

        Ok(())
    } else {
        Err(DbError::NotFound(format!("Profile with ID {} not found", profile_id)))
    }
}

/// Toggle favorite status for a connection profile
///
/// # Arguments
///
/// * `profile_id` - ID of the profile to toggle
/// * `state` - Application state
/// * `app` - Application handle
///
/// # Returns
///
/// Returns the new favorite status (true if now favorite, false if unfavorited)
#[tauri::command]
pub fn toggle_favorite(
    profile_id: String,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<bool, DbError> {
    let mut state_guard = state.lock().unwrap();

    if let Some(profile) = state_guard.get_profile_mut(&profile_id) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        profile.is_favorite = !profile.is_favorite;
        profile.updated_at = now;

        let new_status = profile.is_favorite;

        // Save updated profiles
        drop(state_guard);
        let state_guard = state.lock().unwrap();
        state_guard.save_profiles_to_store(&app)?;

        Ok(new_status)
    } else {
        Err(DbError::NotFound(format!("Profile with ID {} not found", profile_id)))
    }
}

/// Update connection folder
///
/// Moves a connection to a different folder or removes it from a folder.
///
/// # Arguments
///
/// * `profile_id` - ID of the profile to update
/// * `folder` - New folder name (None to remove from folder)
/// * `state` - Application state
/// * `app` - Application handle
///
/// # Returns
///
/// Returns `Ok(())` if successful
#[tauri::command]
pub fn update_connection_folder(
    profile_id: String,
    folder: Option<String>,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<(), DbError> {
    let mut state_guard = state.lock().unwrap();

    if let Some(profile) = state_guard.get_profile_mut(&profile_id) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        profile.folder = folder;
        profile.updated_at = now;

        // Save updated profiles
        drop(state_guard);
        let state_guard = state.lock().unwrap();
        state_guard.save_profiles_to_store(&app)?;

        Ok(())
    } else {
        Err(DbError::NotFound(format!("Profile with ID {} not found", profile_id)))
    }
}

/// Get connection statistics
///
/// Calculates and returns statistics about all saved connection profiles.
///
/// # Arguments
///
/// * `state` - Application state
///
/// # Returns
///
/// Returns connection statistics including total count, favorite count, etc.
#[tauri::command]
pub fn get_connection_stats(
    state: State<'_, Mutex<AppState>>,
) -> Result<serde_json::Value, DbError> {
    let state_guard = state.lock().unwrap();
    let profiles = state_guard.list_profiles();

    let total_connections = profiles.len();
    let favorite_count = profiles.iter().filter(|p| p.is_favorite).count();
    let recent_count = profiles.iter().filter(|p| p.last_connected_at.is_some()).count();

    // Get unique folders
    let folders: std::collections::HashSet<String> = profiles
        .iter()
        .filter_map(|p| p.folder.clone())
        .collect();
    let folder_count = folders.len();

    // Find most used connection
    let most_used_connection = profiles
        .iter()
        .max_by_key(|p| p.connection_count)
        .cloned();

    Ok(serde_json::json!({
        "totalConnections": total_connections,
        "favoriteCount": favorite_count,
        "recentCount": recent_count,
        "folderCount": folder_count,
        "mostUsedConnection": most_used_connection,
    }))
}

/// Get recent connections
///
/// Returns the most recently used connections, sorted by last_connected_at.
///
/// # Arguments
///
/// * `limit` - Maximum number of connections to return
/// * `state` - Application state
///
/// # Returns
///
/// Returns a list of recently used connection profiles
#[tauri::command]
pub fn get_recent_connections(
    limit: usize,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<crate::models::ConnectionProfile>, DbError> {
    let state_guard = state.lock().unwrap();
    let mut profiles: Vec<crate::models::ConnectionProfile> = state_guard
        .list_profiles()
        .into_iter()
        .filter(|p| p.last_connected_at.is_some())
        .cloned()
        .collect();

    // Sort by last_connected_at (most recent first)
    profiles.sort_by(|a, b| {
        b.last_connected_at
            .unwrap_or(0)
            .cmp(&a.last_connected_at.unwrap_or(0))
    });

    // Take only the requested number
    profiles.truncate(limit);

    Ok(profiles)
}

/// Duplicate a connection profile
///
/// Creates a copy of an existing connection profile with a new ID and name.
///
/// # Arguments
///
/// * `profile_id` - ID of the profile to duplicate
/// * `state` - Application state
/// * `app` - Application handle
///
/// # Returns
///
/// Returns the ID of the newly created profile
#[tauri::command]
pub fn duplicate_connection(
    profile_id: String,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<String, DbError> {
    let mut state_guard = state.lock().unwrap();

    // Get the profile to duplicate
    let original = state_guard
        .get_profile(&profile_id)
        .ok_or_else(|| DbError::NotFound(format!("Profile with ID {} not found", profile_id)))?
        .clone();

    // Create a new profile with duplicated settings
    let new_id = uuid::Uuid::new_v4().to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let mut new_profile = original;
    new_profile.id = new_id.clone();
    new_profile.name = format!("{} (Copy)", new_profile.name);
    new_profile.last_connected_at = None;
    new_profile.connection_count = 0;
    new_profile.created_at = now;
    new_profile.updated_at = now;

    // Add the new profile
    state_guard.add_profile(new_profile);

    // Save profiles
    drop(state_guard);
    let state_guard = state.lock().unwrap();
    state_guard.save_profiles_to_store(&app)?;

    Ok(new_id)
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
