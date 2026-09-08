//! Credential storage using OS keyring
//!
//! This module provides secure credential storage using the operating system's
//! native keyring/keychain/credential manager:
//! - macOS: Keychain
//! - Windows: Credential Manager
//! - Linux: Secret Service API (libsecret)

use keyring::Entry;

use crate::models::DbError;

/// Service name for keyring entries
const SERVICE_NAME: &str = "com.dbhive.app";

/// Credential manager for secure password storage
pub struct CredentialManager;

impl CredentialManager {
    /// Save a password to the OS keyring
    ///
    /// # Arguments
    /// * `connection_id` - Unique identifier for the connection
    /// * `password` - Password to store
    ///
    /// # Returns
    /// * `Ok(())` on success
    /// * `Err(DbError)` on failure
    pub fn save_password(connection_id: &str, password: &str) -> Result<(), DbError> {
        let entry = Entry::new(SERVICE_NAME, connection_id)
            .map_err(|e| DbError::CredentialError(format!("Failed to create keyring entry: {}", e)))?;

        entry
            .set_password(password)
            .map_err(|e| DbError::CredentialError(format!("Failed to save password: {}", e)))?;

        Ok(())
    }

    /// Retrieve a password from the OS keyring
    ///
    /// # Arguments
    /// * `connection_id` - Unique identifier for the connection
    ///
    /// # Returns
    /// * `Ok(Some(password))` if password exists
    /// * `Ok(None)` if password not found
    /// * `Err(DbError)` on other errors
    pub fn get_password(connection_id: &str) -> Result<Option<String>, DbError> {
        let entry = Entry::new(SERVICE_NAME, connection_id)
            .map_err(|e| DbError::CredentialError(format!("Failed to create keyring entry: {}", e)))?;

        match entry.get_password() {
            Ok(password) => Ok(Some(password)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(DbError::CredentialError(format!(
                "Failed to retrieve password: {}",
                e
            ))),
        }
    }

    /// Delete a password from the OS keyring
    ///
    /// # Arguments
    /// * `connection_id` - Unique identifier for the connection
    ///
    /// # Returns
    /// * `Ok(())` on success or if entry doesn't exist
    /// * `Err(DbError)` on other errors
    pub fn delete_password(connection_id: &str) -> Result<(), DbError> {
        let entry = Entry::new(SERVICE_NAME, connection_id)
            .map_err(|e| DbError::CredentialError(format!("Failed to create keyring entry: {}", e)))?;

        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()), // Already deleted or never existed
            Err(e) => Err(DbError::CredentialError(format!(
                "Failed to delete password: {}",
                e
            ))),
        }
    }

    /// Check if a password exists in the keyring
    ///
    /// # Arguments
    /// * `connection_id` - Unique identifier for the connection
    ///
    /// # Returns
    /// * `Ok(true)` if password exists
    /// * `Ok(false)` if password doesn't exist
    /// * `Err(DbError)` on errors
    pub fn has_password(connection_id: &str) -> Result<bool, DbError> {
        match Self::get_password(connection_id)? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    /// Save an SSH password to the OS keyring
    ///
    /// # Arguments
    /// * `connection_id` - Unique identifier for the connection
    /// * `password` - SSH password to store
    ///
    /// # Returns
    /// * `Ok(())` on success
    /// * `Err(DbError)` on failure
    pub fn save_ssh_password(connection_id: &str, password: &str) -> Result<(), DbError> {
        let ssh_key = format!("{}-ssh", connection_id);
        Self::save_password(&ssh_key, password)
    }

    /// Retrieve an SSH password from the OS keyring
    ///
    /// # Arguments
    /// * `connection_id` - Unique identifier for the connection
    ///
    /// # Returns
    /// * `Ok(Some(password))` if password exists
    /// * `Ok(None)` if password not found
    /// * `Err(DbError)` on other errors
    pub fn get_ssh_password(connection_id: &str) -> Result<Option<String>, DbError> {
        let ssh_key = format!("{}-ssh", connection_id);
        Self::get_password(&ssh_key)
    }

    /// Delete an SSH password from the OS keyring
    ///
    /// # Arguments
    /// * `connection_id` - Unique identifier for the connection
    ///
    /// # Returns
    /// * `Ok(())` on success or if entry doesn't exist
    /// * `Err(DbError)` on other errors
    pub fn delete_ssh_password(connection_id: &str) -> Result<(), DbError> {
        let ssh_key = format!("{}-ssh", connection_id);
        Self::delete_password(&ssh_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_and_retrieve_password() {
        let test_id = "test_connection_save_retrieve";
        let test_password = "super_secret_password_123";

        // Clean up any existing entry
        let _ = CredentialManager::delete_password(test_id);

        // Save password
        assert!(CredentialManager::save_password(test_id, test_password).is_ok());

        // Retrieve password
        let retrieved = CredentialManager::get_password(test_id).unwrap();
        assert_eq!(retrieved, Some(test_password.to_string()));

        // Cleanup
        assert!(CredentialManager::delete_password(test_id).is_ok());
    }

    #[test]
    fn test_delete_password() {
        let test_id = "test_connection_delete";
        let test_password = "password_to_delete";

        // Clean up any existing entry
        let _ = CredentialManager::delete_password(test_id);

        // Save password
        CredentialManager::save_password(test_id, test_password).unwrap();

        // Verify it exists
        assert!(CredentialManager::has_password(test_id).unwrap());

        // Delete password
        assert!(CredentialManager::delete_password(test_id).is_ok());

        // Verify it's gone
        assert!(!CredentialManager::has_password(test_id).unwrap());
    }

    #[test]
    fn test_get_nonexistent_password() {
        let test_id = "test_connection_nonexistent";

        // Ensure it doesn't exist
        let _ = CredentialManager::delete_password(test_id);

        // Try to get nonexistent password
        let result = CredentialManager::get_password(test_id).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_has_password() {
        let test_id = "test_connection_has_password";
        let test_password = "test_password_exists";

        // Clean up
        let _ = CredentialManager::delete_password(test_id);

        // Should not exist initially
        assert!(!CredentialManager::has_password(test_id).unwrap());

        // Save password
        CredentialManager::save_password(test_id, test_password).unwrap();

        // Should exist now
        assert!(CredentialManager::has_password(test_id).unwrap());

        // Cleanup
        CredentialManager::delete_password(test_id).unwrap();
    }
}
