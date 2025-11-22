//! Data models for DB-Hive
//!
//! This module contains all the core data structures used throughout the application,
//! including connection profiles, metadata types, and error definitions.

pub mod activity;
pub mod connection;
pub mod error;
pub mod history;
pub mod metadata;
pub mod settings;

// Re-export commonly used types for convenience
pub use activity::{
    ActivityStats, ExportFormat, QueryLog, QueryLogFilter, QueryLogResponse, QueryLogSort,
    QueryLogSortField, QueryStatus, QueryType, SortDirection,
};
pub use connection::{ConnectionProfile, ConnectionStatus, DbDriver, SslMode};
pub use error::DbError;
pub use history::{QueryHistory, QuerySnippet};
pub use metadata::{ColumnInfo, DatabaseInfo, ForeignKeyInfo, IndexInfo, SchemaInfo, TableInfo, TableSchema};
pub use settings::AppSettings;
