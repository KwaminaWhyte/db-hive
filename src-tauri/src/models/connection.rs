//! Connection profile and related types
//!
//! This module defines the data structures for managing database connections,
//! including connection profiles, driver types, SSL configuration, and SSH tunneling.

use serde::{Deserialize, Serialize};

/// Supported database drivers
///
/// Represents the different types of databases that DB-Hive can connect to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DbDriver {
    /// PostgreSQL database
    Postgres,
    /// MySQL or MariaDB database
    MySql,
    /// SQLite database (file-based)
    Sqlite,
    /// MongoDB (NoSQL document database)
    MongoDb,
    /// Microsoft SQL Server
    SqlServer,
}

/// SSL/TLS connection mode
///
/// Defines how SSL/TLS should be handled for database connections.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SslMode {
    /// Do not use SSL/TLS
    Disable,
    /// Prefer SSL/TLS but allow unencrypted connections
    Prefer,
    /// Require SSL/TLS (connection fails if not available)
    Require,
}

impl Default for SslMode {
    fn default() -> Self {
        SslMode::Prefer
    }
}

/// SSH authentication method
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SshAuthMethod {
    /// Password-based authentication
    Password,
    /// Public/private key authentication
    PrivateKey,
}

/// SSH tunnel configuration
///
/// Configuration for establishing an SSH tunnel to access a remote database.
/// This is commonly used for databases that are not directly accessible
/// over the network but can be reached through an SSH bastion host.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshConfig {
    /// SSH server hostname or IP address
    pub host: String,

    /// SSH server port (typically 22)
    pub port: u16,

    /// SSH username
    pub username: String,

    /// Authentication method (password or private key)
    pub auth_method: SshAuthMethod,

    /// Path to the private key file (only used with PrivateKey auth)
    pub private_key_path: Option<String>,

    /// Passphrase for encrypted private keys (optional)
    pub key_passphrase_keyring_key: Option<String>,

    /// Local port to bind the tunnel to (0 = auto-assign)
    pub local_port: u16,
}

/// Connection profile
///
/// Represents a saved database connection with all necessary configuration.
/// Passwords are not stored directly but referenced via a keyring key for security.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfile {
    /// Unique identifier for this connection profile
    pub id: String,

    /// User-friendly name for this connection
    pub name: String,

    /// Database driver type
    pub driver: DbDriver,

    /// Database server hostname or IP address
    /// For SQLite, this is the file path
    pub host: String,

    /// Database server port
    pub port: u16,

    /// Username for database authentication
    pub username: String,

    /// Key to retrieve the password from the OS keyring
    /// If None, no password is required (e.g., SQLite)
    pub password_keyring_key: Option<String>,

    /// Default database/schema to connect to
    pub database: Option<String>,

    /// SSL/TLS mode for the connection
    pub ssl_mode: SslMode,

    /// Optional SSH tunnel configuration for accessing remote databases
    pub ssh_tunnel: Option<SshConfig>,

    /// Optional folder/group for organizing connections in the UI
    pub folder: Option<String>,
}

impl ConnectionProfile {
    /// Create a new connection profile with required fields
    ///
    /// # Arguments
    ///
    /// * `id` - Unique identifier (typically a UUID)
    /// * `name` - User-friendly connection name
    /// * `driver` - Database driver type
    /// * `host` - Database server host or file path
    /// * `port` - Database server port
    /// * `username` - Database username
    pub fn new(
        id: String,
        name: String,
        driver: DbDriver,
        host: String,
        port: u16,
        username: String,
    ) -> Self {
        Self {
            id,
            name,
            driver,
            host,
            port,
            username,
            password_keyring_key: None,
            database: None,
            ssl_mode: SslMode::default(),
            ssh_tunnel: None,
            folder: None,
        }
    }

    /// Get the default port for a given database driver
    pub fn default_port_for_driver(driver: &DbDriver) -> u16 {
        match driver {
            DbDriver::Postgres => 5432,
            DbDriver::MySql => 3306,
            DbDriver::Sqlite => 0, // File-based, no port
            DbDriver::MongoDb => 27017,
            DbDriver::SqlServer => 1433,
        }
    }
}

/// Connection status
///
/// Represents the current state of a database connection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConnectionStatus {
    /// Connection is established and active
    Connected,

    /// Connection is not established
    Disconnected,

    /// Connection is in an error state
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_profile_creation() {
        let profile = ConnectionProfile::new(
            "test-id".to_string(),
            "Test DB".to_string(),
            DbDriver::Postgres,
            "localhost".to_string(),
            5432,
            "postgres".to_string(),
        );

        assert_eq!(profile.id, "test-id");
        assert_eq!(profile.name, "Test DB");
        assert_eq!(profile.driver, DbDriver::Postgres);
        assert_eq!(profile.ssl_mode, SslMode::Prefer);
    }

    #[test]
    fn test_default_ports() {
        assert_eq!(ConnectionProfile::default_port_for_driver(&DbDriver::Postgres), 5432);
        assert_eq!(ConnectionProfile::default_port_for_driver(&DbDriver::MySql), 3306);
        assert_eq!(ConnectionProfile::default_port_for_driver(&DbDriver::MongoDb), 27017);
        assert_eq!(ConnectionProfile::default_port_for_driver(&DbDriver::SqlServer), 1433);
        assert_eq!(ConnectionProfile::default_port_for_driver(&DbDriver::Sqlite), 0);
    }

    #[test]
    fn test_ssl_mode_default() {
        assert_eq!(SslMode::default(), SslMode::Prefer);
    }

    #[test]
    fn test_serialization() {
        let profile = ConnectionProfile::new(
            "test-id".to_string(),
            "Test DB".to_string(),
            DbDriver::Postgres,
            "localhost".to_string(),
            5432,
            "postgres".to_string(),
        );

        let json = serde_json::to_string(&profile).unwrap();
        let deserialized: ConnectionProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(profile.id, deserialized.id);
    }
}
