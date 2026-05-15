//! Backup and restore data models for DB-Hive
//!
//! This module defines the types used for database backup/restore operations,
//! including backup entries, options, and progress reporting.

use serde::{Deserialize, Serialize};

/// Represents a completed (or failed) backup entry on disk
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    /// Unique identifier (UUID) for this backup record
    pub id: String,

    /// ID of the connection profile this backup was made from
    pub connection_id: String,

    /// Human-readable name of the connection at the time of backup
    pub connection_name: String,

    /// Database driver string, e.g. "Postgres", "MySql", "Sqlite"
    pub driver: String,

    /// Absolute path to the backup file on disk
    pub file_path: String,

    /// File name component of `file_path`
    pub file_name: String,

    /// Size of the backup file in bytes
    pub size_bytes: u64,

    /// Unix timestamp (seconds) when the backup was created
    pub created_at: i64,

    /// Completion status of the backup
    pub status: BackupStatus,

    /// Optional user-supplied note describing this backup
    pub note: Option<String>,
}

/// Status of a backup operation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum BackupStatus {
    /// Backup completed successfully
    Completed,

    /// Backup failed; the inner string contains the error message
    Failed(String),
}

/// Options controlling how a backup is created
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupOptions {
    /// Whether to include row data (INSERT statements / data dump)
    pub include_data: bool,

    /// Whether to include schema definitions (CREATE TABLE, etc.)
    pub include_schema: bool,

    /// Specific tables to back up; empty means all tables
    pub tables: Vec<String>,

    /// Optional note to attach to the backup entry
    pub note: Option<String>,

    /// Directory where the backup file should be written.
    /// `None` means use the default app data backup directory.
    pub output_dir: Option<String>,
}

/// Options controlling how a backup is restored into a database
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOptions {
    /// Absolute path to the backup file to restore
    pub file_path: String,

    /// If true, drop and recreate the target database before restoring.
    /// For Postgres this runs `dropdb` + `createdb`;
    /// for MySQL it issues `DROP DATABASE` + `CREATE DATABASE`.
    pub drop_existing: bool,
}

/// Progress event sent over a Tauri channel during a long backup/restore
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupProgress {
    /// Current stage label, e.g. "connecting", "dumping", "compressing", "done"
    pub stage: String,

    /// Human-readable status message for the current stage
    pub message: String,

    /// True when the operation has finished (either successfully or with error)
    pub done: bool,

    /// Set to a non-None value when the operation terminated with an error
    pub error: Option<String>,
}
