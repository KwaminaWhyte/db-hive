//! Query history and snippet models
//!
//! This module defines data structures for storing and managing query execution
//! history and saved query snippets. These provide users with the ability to
//! review past queries and save frequently-used SQL for quick access.

use serde::{Deserialize, Serialize};

/// Query history record
///
/// Represents a single executed query with metadata about the execution.
/// History records are automatically created when queries are executed
/// and stored persistently for later review.
///
/// # Fields
///
/// - **id**: Unique identifier (UUID)
/// - **connection_id**: ID of the connection where query was executed
/// - **connection_name**: Human-readable name of the connection
/// - **database**: Database name where query was executed
/// - **query**: The SQL query text that was executed
/// - **executed_at**: ISO 8601 timestamp of when query was executed
/// - **execution_time_ms**: How long the query took to execute (in milliseconds)
/// - **row_count**: Number of rows returned/affected (if available)
/// - **success**: Whether the query executed successfully
/// - **error_message**: Error message if query failed (None if successful)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryHistory {
    /// Unique identifier for this history entry
    pub id: String,

    /// Connection ID where query was executed
    pub connection_id: String,

    /// Human-readable connection name
    pub connection_name: String,

    /// Database name where query was executed
    pub database: String,

    /// The SQL query text
    pub query: String,

    /// ISO 8601 timestamp (e.g., "2025-11-19T12:34:56.789Z")
    pub executed_at: String,

    /// Query execution time in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_time_ms: Option<u64>,

    /// Number of rows returned or affected
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_count: Option<u64>,

    /// Whether query executed successfully
    pub success: bool,

    /// Error message if query failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

impl QueryHistory {
    /// Create a new query history record
    ///
    /// # Arguments
    ///
    /// * `connection_id` - ID of the connection
    /// * `connection_name` - Human-readable connection name
    /// * `database` - Database name
    /// * `query` - SQL query text
    /// * `executed_at` - ISO 8601 timestamp
    ///
    /// # Returns
    ///
    /// A new `QueryHistory` instance with generated UUID
    pub fn new(
        connection_id: String,
        connection_name: String,
        database: String,
        query: String,
        executed_at: String,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            connection_id,
            connection_name,
            database,
            query,
            executed_at,
            execution_time_ms: None,
            row_count: None,
            success: true,
            error_message: None,
        }
    }

    /// Mark query as successful with execution metadata
    pub fn with_success(mut self, execution_time_ms: u64, row_count: Option<u64>) -> Self {
        self.success = true;
        self.execution_time_ms = Some(execution_time_ms);
        self.row_count = row_count;
        self
    }

    /// Mark query as failed with error message
    pub fn with_error(mut self, error_message: String, execution_time_ms: Option<u64>) -> Self {
        self.success = false;
        self.error_message = Some(error_message);
        self.execution_time_ms = execution_time_ms;
        self
    }
}

/// Saved query snippet
///
/// Represents a user-saved SQL query snippet that can be quickly
/// accessed and reused. Snippets can be organized by tags/categories
/// and include descriptions for better organization.
///
/// # Fields
///
/// - **id**: Unique identifier (UUID)
/// - **name**: User-provided name for the snippet
/// - **description**: Optional description of what the snippet does
/// - **query**: The SQL query text
/// - **tags**: Optional array of tags for categorization (e.g., ["backup", "maintenance"])
/// - **created_at**: ISO 8601 timestamp of when snippet was created
/// - **updated_at**: ISO 8601 timestamp of last update
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuerySnippet {
    /// Unique identifier for this snippet
    pub id: String,

    /// User-provided name
    pub name: String,

    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The SQL query text
    pub query: String,

    /// Optional tags for categorization
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,

    /// ISO 8601 timestamp of creation
    pub created_at: String,

    /// ISO 8601 timestamp of last update
    pub updated_at: String,
}

impl QuerySnippet {
    /// Create a new query snippet
    ///
    /// # Arguments
    ///
    /// * `name` - User-provided name
    /// * `query` - SQL query text
    /// * `description` - Optional description
    /// * `tags` - Optional tags for categorization
    ///
    /// # Returns
    ///
    /// A new `QuerySnippet` instance with generated UUID and timestamps
    pub fn new(
        name: String,
        query: String,
        description: Option<String>,
        tags: Option<Vec<String>>,
    ) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            description,
            query,
            tags,
            created_at: now.clone(),
            updated_at: now,
        }
    }

    /// Update the snippet with new values
    pub fn update(
        &mut self,
        name: Option<String>,
        query: Option<String>,
        description: Option<String>,
        tags: Option<Vec<String>>,
    ) {
        if let Some(n) = name {
            self.name = n;
        }
        if let Some(q) = query {
            self.query = q;
        }
        if description.is_some() {
            self.description = description;
        }
        if tags.is_some() {
            self.tags = tags;
        }
        self.updated_at = chrono::Utc::now().to_rfc3339();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_history_new() {
        let history = QueryHistory::new(
            "conn-123".to_string(),
            "Test DB".to_string(),
            "mydb".to_string(),
            "SELECT * FROM users".to_string(),
            "2025-11-19T12:00:00Z".to_string(),
        );

        assert_eq!(history.connection_id, "conn-123");
        assert_eq!(history.query, "SELECT * FROM users");
        assert!(history.success);
        assert!(history.error_message.is_none());
    }

    #[test]
    fn test_query_history_with_success() {
        let history = QueryHistory::new(
            "conn-123".to_string(),
            "Test DB".to_string(),
            "mydb".to_string(),
            "SELECT * FROM users".to_string(),
            "2025-11-19T12:00:00Z".to_string(),
        )
        .with_success(150, Some(42));

        assert!(history.success);
        assert_eq!(history.execution_time_ms, Some(150));
        assert_eq!(history.row_count, Some(42));
    }

    #[test]
    fn test_query_history_with_error() {
        let history = QueryHistory::new(
            "conn-123".to_string(),
            "Test DB".to_string(),
            "mydb".to_string(),
            "SELECT * FROM nonexistent".to_string(),
            "2025-11-19T12:00:00Z".to_string(),
        )
        .with_error("Table not found".to_string(), Some(50));

        assert!(!history.success);
        assert_eq!(history.error_message, Some("Table not found".to_string()));
        assert_eq!(history.execution_time_ms, Some(50));
    }

    #[test]
    fn test_query_snippet_new() {
        let snippet = QuerySnippet::new(
            "User Backup".to_string(),
            "SELECT * FROM users".to_string(),
            Some("Backup all users".to_string()),
            Some(vec!["backup".to_string()]),
        );

        assert_eq!(snippet.name, "User Backup");
        assert_eq!(snippet.query, "SELECT * FROM users");
        assert_eq!(
            snippet.description,
            Some("Backup all users".to_string())
        );
        assert!(!snippet.id.is_empty());
    }

    #[test]
    fn test_query_snippet_update() {
        let mut snippet = QuerySnippet::new(
            "Original".to_string(),
            "SELECT 1".to_string(),
            None,
            None,
        );

        let original_created = snippet.created_at.clone();

        snippet.update(
            Some("Updated".to_string()),
            Some("SELECT 2".to_string()),
            Some("New description".to_string()),
            Some(vec!["test".to_string()]),
        );

        assert_eq!(snippet.name, "Updated");
        assert_eq!(snippet.query, "SELECT 2");
        assert_eq!(
            snippet.description,
            Some("New description".to_string())
        );
        assert_eq!(snippet.created_at, original_created);
        assert_ne!(snippet.updated_at, original_created);
    }
}
