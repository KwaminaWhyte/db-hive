//! Application state management
//!
//! This module provides centralized state management for DB-Hive, including
//! active database connections, saved connection profiles, and query history.
//! All state is managed through a thread-safe `Mutex<AppState>` that is shared
//! across the Tauri application.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::drivers::DatabaseDriver;
use crate::models::{
    ColumnInfo, ConnectionProfile, DatabaseInfo, DbError, QueryHistory, QuerySnippet, SchemaInfo,
    TableInfo,
};
use crate::ssh::SshTunnelManager;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

/// Metadata cache entry for a database connection
///
/// Caches schema metadata to improve autocomplete performance
#[derive(Debug, Clone)]
pub struct MetadataCache {
    /// List of databases
    pub databases: Vec<DatabaseInfo>,

    /// Map of database name to schemas
    pub schemas: HashMap<String, Vec<SchemaInfo>>,

    /// Map of schema name to tables
    pub tables: HashMap<String, Vec<TableInfo>>,

    /// Map of "schema.table" to columns
    pub columns: HashMap<String, Vec<ColumnInfo>>,

    /// When the cache was last updated
    pub last_updated: SystemTime,
}

impl MetadataCache {
    /// Create a new empty metadata cache
    pub fn new() -> Self {
        Self {
            databases: Vec::new(),
            schemas: HashMap::new(),
            tables: HashMap::new(),
            columns: HashMap::new(),
            last_updated: SystemTime::now(),
        }
    }

    /// Check if the cache is stale (older than 5 minutes)
    pub fn is_stale(&self) -> bool {
        if let Ok(elapsed) = self.last_updated.elapsed() {
            elapsed > Duration::from_secs(300) // 5 minutes
        } else {
            true
        }
    }

    /// Update the timestamp
    pub fn touch(&mut self) {
        self.last_updated = SystemTime::now();
    }
}

/// Application state
///
/// Central state container for the entire application. This is wrapped in a `Mutex`
/// and managed by Tauri's state management system to provide thread-safe access
/// across all commands.
///
/// # State Components
///
/// - **connections**: Active database connections mapped by connection ID
/// - **connection_profiles**: Saved connection profiles for quick reconnection
/// - **query_history**: Historical record of executed queries (placeholder for now)
///
/// # Usage
///
/// ```rust,ignore
/// use tauri::State;
/// use std::sync::Mutex;
///
/// #[tauri::command]
/// fn my_command(state: State<'_, Mutex<AppState>>) -> Result<(), String> {
///     let mut state = state.lock().unwrap();
///     state.add_profile(profile);
///     Ok(())
/// }
/// ```
pub struct AppState {
    /// Active database connections
    /// Key: Connection ID (UUID), Value: Database driver instance
    pub connections: HashMap<String, Arc<dyn DatabaseDriver>>,

    /// Saved connection profiles
    /// Key: Profile ID (UUID), Value: Connection profile
    pub connection_profiles: HashMap<String, ConnectionProfile>,

    /// Temporary password storage for active connections (not persisted)
    /// Key: Connection ID (UUID), Value: Password
    /// These are cleared when connections are disconnected
    pub connection_passwords: HashMap<String, String>,

    /// Query execution history
    /// Stores records of all executed queries with metadata
    pub query_history: Vec<QueryHistory>,

    /// Saved query snippets
    /// Key: Snippet ID (UUID), Value: Query snippet
    pub query_snippets: HashMap<String, QuerySnippet>,

    /// SSH tunnel manager for managing active SSH tunnels
    pub ssh_tunnel_manager: SshTunnelManager,

    /// Metadata cache for each connection
    /// Key: Connection ID (UUID), Value: Metadata cache
    pub metadata_cache: HashMap<String, MetadataCache>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            connections: HashMap::new(),
            connection_profiles: HashMap::new(),
            connection_passwords: HashMap::new(),
            query_history: Vec::new(),
            query_snippets: HashMap::new(),
            ssh_tunnel_manager: SshTunnelManager::new(),
            metadata_cache: HashMap::new(),
        }
    }
}

impl AppState {
    /// Create a new empty application state
    ///
    /// # Returns
    ///
    /// A new `AppState` instance with empty collections
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
            connection_profiles: HashMap::new(),
            connection_passwords: HashMap::new(),
            query_history: Vec::new(),
            query_snippets: HashMap::new(),
            ssh_tunnel_manager: SshTunnelManager::new(),
            metadata_cache: HashMap::new(),
        }
    }

    // ========================================================================
    // Connection Management
    // ========================================================================

    /// Add an active database connection
    ///
    /// # Arguments
    ///
    /// * `id` - Unique connection identifier (typically a UUID)
    /// * `connection` - Database driver instance
    ///
    /// # Notes
    ///
    /// If a connection with the same ID already exists, it will be replaced.
    pub fn add_connection(&mut self, id: String, connection: Arc<dyn DatabaseDriver>) {
        self.connections.insert(id, connection);
    }

    /// Remove an active database connection
    ///
    /// # Arguments
    ///
    /// * `id` - Connection identifier to remove
    ///
    /// # Returns
    ///
    /// The removed connection if it existed, `None` otherwise
    pub fn remove_connection(&mut self, id: &str) -> Option<Arc<dyn DatabaseDriver>> {
        self.connections.remove(id)
    }

    /// Get a reference to an active database connection
    ///
    /// # Arguments
    ///
    /// * `id` - Connection identifier to retrieve
    ///
    /// # Returns
    ///
    /// Reference to the connection if it exists, `None` otherwise
    pub fn get_connection(&self, id: &str) -> Option<&Arc<dyn DatabaseDriver>> {
        self.connections.get(id)
    }

    /// Check if a connection is active
    ///
    /// # Arguments
    ///
    /// * `id` - Connection identifier to check
    ///
    /// # Returns
    ///
    /// `true` if the connection exists and is active, `false` otherwise
    pub fn has_connection(&self, id: &str) -> bool {
        self.connections.contains_key(id)
    }

    /// Get the count of active connections
    ///
    /// # Returns
    ///
    /// Number of active database connections
    pub fn active_connection_count(&self) -> usize {
        self.connections.len()
    }

    // ========================================================================
    // Connection Profile Management
    // ========================================================================

    /// Add a connection profile
    ///
    /// # Arguments
    ///
    /// * `profile` - Connection profile to save
    ///
    /// # Notes
    ///
    /// If a profile with the same ID already exists, it will be replaced.
    pub fn add_profile(&mut self, profile: ConnectionProfile) {
        self.connection_profiles.insert(profile.id.clone(), profile);
    }

    /// Remove a connection profile
    ///
    /// # Arguments
    ///
    /// * `id` - Profile identifier to remove
    ///
    /// # Returns
    ///
    /// The removed profile if it existed, `None` otherwise
    pub fn remove_profile(&mut self, id: &str) -> Option<ConnectionProfile> {
        self.connection_profiles.remove(id)
    }

    /// Get a reference to a connection profile
    ///
    /// # Arguments
    ///
    /// * `id` - Profile identifier to retrieve
    ///
    /// # Returns
    ///
    /// Reference to the profile if it exists, `None` otherwise
    pub fn get_profile(&self, id: &str) -> Option<&ConnectionProfile> {
        self.connection_profiles.get(id)
    }

    /// Get a mutable reference to a connection profile
    ///
    /// # Arguments
    ///
    /// * `id` - Profile identifier to retrieve
    ///
    /// # Returns
    ///
    /// Mutable reference to the profile if it exists, `None` otherwise
    pub fn get_profile_mut(&mut self, id: &str) -> Option<&mut ConnectionProfile> {
        self.connection_profiles.get_mut(id)
    }

    /// List all connection profiles
    ///
    /// # Returns
    ///
    /// Vector of references to all saved connection profiles
    pub fn list_profiles(&self) -> Vec<&ConnectionProfile> {
        self.connection_profiles.values().collect()
    }

    /// Get the count of saved profiles
    ///
    /// # Returns
    ///
    /// Number of saved connection profiles
    pub fn profile_count(&self) -> usize {
        self.connection_profiles.len()
    }

    /// Find profiles by folder
    ///
    /// # Arguments
    ///
    /// * `folder` - Folder name to filter by
    ///
    /// # Returns
    ///
    /// Vector of references to profiles in the specified folder
    pub fn profiles_in_folder(&self, folder: &str) -> Vec<&ConnectionProfile> {
        self.connection_profiles
            .values()
            .filter(|profile| {
                profile
                    .folder
                    .as_ref()
                    .map(|f| f == folder)
                    .unwrap_or(false)
            })
            .collect()
    }

    // ========================================================================
    // Persistent Storage
    // ========================================================================

    /// Load connection profiles from persistent storage
    ///
    /// # Arguments
    ///
    /// * `app` - Tauri application handle
    ///
    /// # Returns
    ///
    /// Number of profiles loaded
    pub fn load_profiles_from_store(&mut self, app: &AppHandle) -> Result<usize, DbError> {
        let store = app
            .store("profiles.json")
            .map_err(|e| DbError::InternalError(format!("Failed to access store: {}", e)))?;

        // Get profiles from store
        if let Some(profiles_value) = store.get("profiles") {
            // Deserialize profiles
            let profiles: Vec<ConnectionProfile> =
                serde_json::from_value(profiles_value.clone()).map_err(|e| {
                    DbError::InternalError(format!("Failed to deserialize profiles: {}", e))
                })?;

            // Add each profile to the in-memory map
            let count = profiles.len();
            for profile in profiles {
                self.connection_profiles
                    .insert(profile.id.clone(), profile);
            }

            Ok(count)
        } else {
            // No profiles in store yet
            Ok(0)
        }
    }

    /// Save connection profiles to persistent storage
    ///
    /// # Arguments
    ///
    /// * `app` - Tauri application handle
    ///
    /// # Returns
    ///
    /// Ok(()) if successful
    pub fn save_profiles_to_store(&self, app: &AppHandle) -> Result<(), DbError> {
        let store = app
            .store("profiles.json")
            .map_err(|e| DbError::InternalError(format!("Failed to access store: {}", e)))?;

        // Convert profiles map to vector
        let profiles: Vec<&ConnectionProfile> = self.connection_profiles.values().collect();

        // Serialize and save to store
        let profiles_value = serde_json::to_value(&profiles)
            .map_err(|e| DbError::InternalError(format!("Failed to serialize profiles: {}", e)))?;

        // Set profiles in store (returns ())
        store.set("profiles", profiles_value);

        // Save the store to disk
        store
            .save()
            .map_err(|e| DbError::InternalError(format!("Failed to persist store: {}", e)))?;

        Ok(())
    }

    /// Load saved passwords from persistent storage
    ///
    /// # Arguments
    ///
    /// * `app` - Tauri application handle
    ///
    /// # Returns
    ///
    /// Number of passwords loaded
    ///
    /// # Security Note
    ///
    /// This is a temporary solution. Passwords are stored in plaintext
    /// in the store file. In the future, this should be replaced with
    /// OS keyring storage for better security.
    pub fn load_passwords_from_store(&mut self, app: &AppHandle) -> Result<usize, DbError> {
        let store = app
            .store("passwords.json")
            .map_err(|e| DbError::InternalError(format!("Failed to access store: {}", e)))?;

        // Get passwords from store
        if let Some(passwords_value) = store.get("passwords") {
            // Deserialize passwords map
            let passwords: HashMap<String, String> =
                serde_json::from_value(passwords_value.clone()).map_err(|e| {
                    DbError::InternalError(format!("Failed to deserialize passwords: {}", e))
                })?;

            let count = passwords.len();
            self.connection_passwords = passwords;

            Ok(count)
        } else {
            // No passwords in store yet
            Ok(0)
        }
    }

    /// Save connection passwords to persistent storage
    ///
    /// # Arguments
    ///
    /// * `app` - Tauri application handle
    ///
    /// # Returns
    ///
    /// Ok(()) if successful
    ///
    /// # Security Note
    ///
    /// This is a temporary solution. Passwords are stored in plaintext
    /// in the store file. In the future, this should be replaced with
    /// OS keyring storage for better security.
    pub fn save_passwords_to_store(&self, app: &AppHandle) -> Result<(), DbError> {
        let store = app
            .store("passwords.json")
            .map_err(|e| DbError::InternalError(format!("Failed to access store: {}", e)))?;

        // Serialize and save passwords
        let passwords_value = serde_json::to_value(&self.connection_passwords)
            .map_err(|e| DbError::InternalError(format!("Failed to serialize passwords: {}", e)))?;

        // Set passwords in store
        store.set("passwords", passwords_value);

        // Save the store to disk
        store
            .save()
            .map_err(|e| DbError::InternalError(format!("Failed to persist store: {}", e)))?;

        Ok(())
    }

    // ========================================================================
    // Query History Management
    // ========================================================================

    /// Add a query history entry
    pub fn add_history(&mut self, history: QueryHistory) {
        self.query_history.push(history);
    }

    /// Get all query history
    pub fn get_all_history(&self) -> Vec<QueryHistory> {
        self.query_history.clone()
    }

    /// Get query history for a specific connection
    pub fn get_history_by_connection(&self, connection_id: &str) -> Vec<QueryHistory> {
        self.query_history
            .iter()
            .filter(|h| h.connection_id == connection_id)
            .cloned()
            .collect()
    }

    /// Clear all query history
    pub fn clear_all_history(&mut self) -> usize {
        let count = self.query_history.len();
        self.query_history.clear();
        count
    }

    /// Clear query history for a specific connection
    pub fn clear_history_by_connection(&mut self, connection_id: &str) -> usize {
        let original_len = self.query_history.len();
        self.query_history
            .retain(|h| h.connection_id != connection_id);
        original_len - self.query_history.len()
    }

    /// Load query history from persistent storage
    pub fn load_history_from_store(&mut self, app: &AppHandle) -> Result<usize, DbError> {
        let store = app
            .store("history.json")
            .map_err(|e| DbError::InternalError(format!("Failed to access store: {}", e)))?;

        if let Some(history_value) = store.get("history") {
            let history: Vec<QueryHistory> =
                serde_json::from_value(history_value.clone()).map_err(|e| {
                    DbError::InternalError(format!("Failed to deserialize history: {}", e))
                })?;

            let count = history.len();
            self.query_history = history;
            Ok(count)
        } else {
            Ok(0)
        }
    }

    /// Save query history to persistent storage
    pub fn save_history_to_store(&self, app: &AppHandle) -> Result<(), DbError> {
        let store = app
            .store("history.json")
            .map_err(|e| DbError::InternalError(format!("Failed to access store: {}", e)))?;

        let history_value = serde_json::to_value(&self.query_history)
            .map_err(|e| DbError::InternalError(format!("Failed to serialize history: {}", e)))?;

        store.set("history", history_value);

        store
            .save()
            .map_err(|e| DbError::InternalError(format!("Failed to persist store: {}", e)))?;

        Ok(())
    }

    // ========================================================================
    // Query Snippet Management
    // ========================================================================

    /// Add or update a query snippet
    pub fn add_snippet(&mut self, snippet: QuerySnippet) {
        self.query_snippets.insert(snippet.id.clone(), snippet);
    }

    /// Remove a query snippet
    pub fn remove_snippet(&mut self, id: &str) -> Option<QuerySnippet> {
        self.query_snippets.remove(id)
    }

    /// Get a query snippet by ID
    pub fn get_snippet(&self, id: &str) -> Option<&QuerySnippet> {
        self.query_snippets.get(id)
    }

    /// Get all query snippets
    pub fn get_all_snippets(&self) -> Vec<QuerySnippet> {
        self.query_snippets.values().cloned().collect()
    }

    /// Get snippets filtered by tag
    pub fn get_snippets_by_tag(&self, tag: &str) -> Vec<QuerySnippet> {
        self.query_snippets
            .values()
            .filter(|s| {
                s.tags
                    .as_ref()
                    .map(|tags| tags.contains(&tag.to_string()))
                    .unwrap_or(false)
            })
            .cloned()
            .collect()
    }

    /// Load query snippets from persistent storage
    pub fn load_snippets_from_store(&mut self, app: &AppHandle) -> Result<usize, DbError> {
        let store = app
            .store("snippets.json")
            .map_err(|e| DbError::InternalError(format!("Failed to access store: {}", e)))?;

        if let Some(snippets_value) = store.get("snippets") {
            let snippets: Vec<QuerySnippet> =
                serde_json::from_value(snippets_value.clone()).map_err(|e| {
                    DbError::InternalError(format!("Failed to deserialize snippets: {}", e))
                })?;

            let count = snippets.len();
            for snippet in snippets {
                self.query_snippets.insert(snippet.id.clone(), snippet);
            }
            Ok(count)
        } else {
            Ok(0)
        }
    }

    /// Save query snippets to persistent storage
    pub fn save_snippets_to_store(&self, app: &AppHandle) -> Result<(), DbError> {
        let store = app
            .store("snippets.json")
            .map_err(|e| DbError::InternalError(format!("Failed to access store: {}", e)))?;

        let snippets: Vec<&QuerySnippet> = self.query_snippets.values().collect();
        let snippets_value = serde_json::to_value(&snippets)
            .map_err(|e| DbError::InternalError(format!("Failed to serialize snippets: {}", e)))?;

        store.set("snippets", snippets_value);

        store
            .save()
            .map_err(|e| DbError::InternalError(format!("Failed to persist store: {}", e)))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{DbDriver, SslMode};

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

    #[test]
    fn test_app_state_new() {
        let state = AppState::new();
        assert_eq!(state.connections.len(), 0);
        assert_eq!(state.connection_profiles.len(), 0);
    }

    #[test]
    fn test_app_state_default() {
        let state = AppState::default();
        assert_eq!(state.connections.len(), 0);
        assert_eq!(state.connection_profiles.len(), 0);
    }

    #[test]
    fn test_add_and_get_profile() {
        let mut state = AppState::new();
        let profile = create_test_profile("test-1", "Test DB");

        state.add_profile(profile.clone());
        assert_eq!(state.profile_count(), 1);

        let retrieved = state.get_profile("test-1");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "Test DB");
    }

    #[test]
    fn test_remove_profile() {
        let mut state = AppState::new();
        let profile = create_test_profile("test-1", "Test DB");

        state.add_profile(profile);
        assert_eq!(state.profile_count(), 1);

        let removed = state.remove_profile("test-1");
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().name, "Test DB");
        assert_eq!(state.profile_count(), 0);

        // Try to remove again
        let removed_again = state.remove_profile("test-1");
        assert!(removed_again.is_none());
    }

    #[test]
    fn test_list_profiles() {
        let mut state = AppState::new();
        state.add_profile(create_test_profile("test-1", "DB 1"));
        state.add_profile(create_test_profile("test-2", "DB 2"));
        state.add_profile(create_test_profile("test-3", "DB 3"));

        let profiles = state.list_profiles();
        assert_eq!(profiles.len(), 3);
    }

    #[test]
    fn test_profile_replace() {
        let mut state = AppState::new();
        let mut profile1 = create_test_profile("test-1", "Original Name");
        state.add_profile(profile1.clone());

        profile1.name = "Updated Name".to_string();
        state.add_profile(profile1);

        assert_eq!(state.profile_count(), 1);
        let retrieved = state.get_profile("test-1").unwrap();
        assert_eq!(retrieved.name, "Updated Name");
    }

    #[test]
    fn test_profiles_in_folder() {
        let mut state = AppState::new();

        let mut profile1 = create_test_profile("test-1", "DB 1");
        profile1.folder = Some("Production".to_string());

        let mut profile2 = create_test_profile("test-2", "DB 2");
        profile2.folder = Some("Production".to_string());

        let mut profile3 = create_test_profile("test-3", "DB 3");
        profile3.folder = Some("Development".to_string());

        let profile4 = create_test_profile("test-4", "DB 4");

        state.add_profile(profile1);
        state.add_profile(profile2);
        state.add_profile(profile3);
        state.add_profile(profile4);

        let prod_profiles = state.profiles_in_folder("Production");
        assert_eq!(prod_profiles.len(), 2);

        let dev_profiles = state.profiles_in_folder("Development");
        assert_eq!(dev_profiles.len(), 1);

        let nonexistent = state.profiles_in_folder("Nonexistent");
        assert_eq!(nonexistent.len(), 0);
    }

    // Note: Connection tests are commented out until we have a mock DatabaseDriver
    // implementation for testing. The connection management methods are tested
    // indirectly through integration tests with real database drivers.
    //
    // #[test]
    // fn test_connection_management() {
    //     // Requires a mock DatabaseDriver implementation
    // }

    #[test]
    fn test_get_profile_mut() {
        let mut state = AppState::new();
        let profile = create_test_profile("test-1", "Original Name");
        state.add_profile(profile);

        {
            let profile_mut = state.get_profile_mut("test-1").unwrap();
            profile_mut.name = "Modified Name".to_string();
        }

        let retrieved = state.get_profile("test-1").unwrap();
        assert_eq!(retrieved.name, "Modified Name");
    }
}
