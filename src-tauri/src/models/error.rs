//! Error types for DB-Hive
//!
//! This module defines the custom error types used throughout the application.
//! The `DbError` enum represents all possible errors that can occur during
//! database operations and is serializable for transmission over Tauri's IPC.

use serde::Serializer;
use thiserror::Error;

/// Main error type for all database operations
///
/// This enum covers all possible error scenarios in DB-Hive, from connection
/// failures to query execution errors. It implements `Serialize` to enable
/// transmission to the frontend via Tauri's IPC mechanism.
#[derive(Debug, Error)]
pub enum DbError {
    /// Error occurred while establishing or maintaining a database connection
    #[error("Connection failed: {0}")]
    ConnectionError(String),

    /// Error occurred during query execution
    #[error("Query execution failed: {0}")]
    QueryError(String),

    /// Authentication or authorization error
    #[error("Authentication failed: {0}")]
    AuthError(String),

    /// Operation timed out
    #[error("Operation timed out: {0}")]
    TimeoutError(String),

    /// Invalid input provided by the user
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Requested resource was not found
    #[error("Not found: {0}")]
    NotFound(String),

    /// Internal error that doesn't fit other categories
    #[error("Internal error: {0}")]
    InternalError(String),
}

impl serde::Serialize for DbError {
    /// Serialize the error for transmission over Tauri's IPC
    ///
    /// Converts the error into a JSON object with `kind` and `message` fields:
    /// ```json
    /// {
    ///   "kind": "connection",
    ///   "message": "Connection failed: timeout"
    /// }
    /// ```
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;

        let mut state = serializer.serialize_struct("DbError", 2)?;

        // Determine error kind
        let kind = match self {
            DbError::ConnectionError(_) => "connection",
            DbError::QueryError(_) => "query",
            DbError::AuthError(_) => "auth",
            DbError::TimeoutError(_) => "timeout",
            DbError::InvalidInput(_) => "invalid_input",
            DbError::NotFound(_) => "not_found",
            DbError::InternalError(_) => "internal",
        };

        state.serialize_field("kind", kind)?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_serialization() {
        let error = DbError::ConnectionError("timeout".to_string());
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("\"kind\":\"connection\""));
        assert!(json.contains("\"message\":"));
    }

    #[test]
    fn test_error_display() {
        let error = DbError::QueryError("syntax error".to_string());
        assert_eq!(
            error.to_string(),
            "Query execution failed: syntax error"
        );
    }
}
