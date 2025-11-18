//! Application state management
//!
//! This module provides centralized state management for DB-Hive, including
//! active database connections, saved connection profiles, and query history.
//! All state is managed through a thread-safe `Mutex<AppState>` that is shared
//! across the Tauri application.

use std::collections::HashMap;
use std::sync::Arc;

use crate::drivers::DatabaseDriver;
use crate::models::ConnectionProfile;

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
#[derive(Default)]
pub struct AppState {
    /// Active database connections
    /// Key: Connection ID (UUID), Value: Database driver instance
    pub connections: HashMap<String, Arc<dyn DatabaseDriver>>,

    /// Saved connection profiles
    /// Key: Profile ID (UUID), Value: Connection profile
    pub connection_profiles: HashMap<String, ConnectionProfile>,

    // TODO: Define QueryRecord type and add query_history field
    // pub query_history: Vec<QueryRecord>,
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
