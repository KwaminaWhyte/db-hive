//! Data models for DB-Hive
//!
//! This module contains all the core data structures used throughout the application,
//! including connection profiles, metadata types, and error definitions.

pub mod connection;
pub mod error;
pub mod history;
pub mod metadata;

// Re-export commonly used types for convenience
pub use connection::{ConnectionProfile, ConnectionStatus, DbDriver};
pub use error::DbError;
pub use history::{QueryHistory, QuerySnippet};
pub use metadata::{ColumnInfo, DatabaseInfo, IndexInfo, SchemaInfo, TableInfo, TableSchema};
