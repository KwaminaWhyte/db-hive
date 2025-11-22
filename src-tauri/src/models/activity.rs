//! Activity logging models
//!
//! This module defines types for tracking query execution, including query logs,
//! status tracking, and activity statistics.

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Query execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueryStatus {
    /// Query is currently running
    Running,
    /// Query completed successfully
    Completed,
    /// Query failed with an error
    Failed,
    /// Query was cancelled by user
    Cancelled,
}

/// Query type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum QueryType {
    /// SELECT query (data retrieval)
    Select,
    /// INSERT query (data insertion)
    Insert,
    /// UPDATE query (data modification)
    Update,
    /// DELETE query (data deletion)
    Delete,
    /// CREATE query (DDL - creating objects)
    Create,
    /// ALTER query (DDL - modifying objects)
    Alter,
    /// DROP query (DDL - dropping objects)
    Drop,
    /// Transaction control (BEGIN, COMMIT, ROLLBACK)
    Transaction,
    /// Other query types
    Other,
}

impl QueryType {
    /// Detect query type from SQL string
    ///
    /// Uses regex patterns to identify the query type from the first significant
    /// SQL keyword. This is a simple heuristic and may not handle complex cases
    /// like CTEs or nested queries perfectly.
    ///
    /// # Arguments
    ///
    /// * `sql` - SQL query string to analyze
    ///
    /// # Returns
    ///
    /// The detected QueryType
    pub fn from_sql(sql: &str) -> Self {
        // Get or compile regex patterns on first use
        static PATTERNS: OnceLock<Vec<(Regex, QueryType)>> = OnceLock::new();
        let patterns = PATTERNS.get_or_init(|| {
            vec![
                (
                    Regex::new(r"(?i)^\s*(WITH\s+\w+\s+AS\s+\(.*?\)\s+)?SELECT\b").unwrap(),
                    QueryType::Select,
                ),
                (
                    Regex::new(r"(?i)^\s*INSERT\b").unwrap(),
                    QueryType::Insert,
                ),
                (
                    Regex::new(r"(?i)^\s*UPDATE\b").unwrap(),
                    QueryType::Update,
                ),
                (
                    Regex::new(r"(?i)^\s*DELETE\b").unwrap(),
                    QueryType::Delete,
                ),
                (
                    Regex::new(r"(?i)^\s*CREATE\b").unwrap(),
                    QueryType::Create,
                ),
                (
                    Regex::new(r"(?i)^\s*ALTER\b").unwrap(),
                    QueryType::Alter,
                ),
                (
                    Regex::new(r"(?i)^\s*DROP\b").unwrap(),
                    QueryType::Drop,
                ),
                (
                    Regex::new(r"(?i)^\s*(BEGIN|COMMIT|ROLLBACK|START\s+TRANSACTION)\b").unwrap(),
                    QueryType::Transaction,
                ),
            ]
        });

        // Match against patterns
        for (pattern, query_type) in patterns {
            if pattern.is_match(sql) {
                return *query_type;
            }
        }

        QueryType::Other
    }
}

/// Individual query log entry
///
/// Records all relevant information about a query execution, including timing,
/// results, and error information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryLog {
    /// Unique log entry ID
    pub id: String,

    /// Connection ID this query was executed on
    pub connection_id: String,

    /// Connection profile name
    pub connection_name: String,

    /// Database name (if applicable)
    pub database: Option<String>,

    /// SQL query text
    pub sql: String,

    /// Query type (SELECT, INSERT, etc.)
    pub query_type: QueryType,

    /// Execution status
    pub status: QueryStatus,

    /// Start timestamp (ISO 8601)
    #[serde(with = "chrono::serde::ts_seconds")]
    pub started_at: DateTime<Utc>,

    /// End timestamp (ISO 8601)
    #[serde(
        with = "chrono::serde::ts_seconds_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub completed_at: Option<DateTime<Utc>>,

    /// Execution duration in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,

    /// Number of rows affected/returned
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_count: Option<u64>,

    /// Error message if failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// User-added tags for categorization
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

impl QueryLog {
    /// Create a new query log entry for a query that just started
    ///
    /// # Arguments
    ///
    /// * `id` - Unique log entry ID
    /// * `connection_id` - Connection ID
    /// * `connection_name` - Connection profile name
    /// * `database` - Database name (optional)
    /// * `sql` - SQL query string
    ///
    /// # Returns
    ///
    /// A new QueryLog instance with status Running
    pub fn new(
        id: String,
        connection_id: String,
        connection_name: String,
        database: Option<String>,
        sql: String,
    ) -> Self {
        let query_type = QueryType::from_sql(&sql);
        Self {
            id,
            connection_id,
            connection_name,
            database,
            sql,
            query_type,
            status: QueryStatus::Running,
            started_at: Utc::now(),
            completed_at: None,
            duration_ms: None,
            row_count: None,
            error: None,
            tags: None,
        }
    }

    /// Mark the query as completed with results
    ///
    /// # Arguments
    ///
    /// * `duration_ms` - Execution duration in milliseconds
    /// * `row_count` - Number of rows affected/returned
    pub fn complete(&mut self, duration_ms: u64, row_count: Option<u64>) {
        self.status = QueryStatus::Completed;
        self.completed_at = Some(Utc::now());
        self.duration_ms = Some(duration_ms);
        self.row_count = row_count;
    }

    /// Mark the query as failed with an error
    ///
    /// # Arguments
    ///
    /// * `duration_ms` - Execution duration in milliseconds before failure
    /// * `error` - Error message
    pub fn fail(&mut self, duration_ms: u64, error: String) {
        self.status = QueryStatus::Failed;
        self.completed_at = Some(Utc::now());
        self.duration_ms = Some(duration_ms);
        self.error = Some(error);
    }

    /// Mark the query as cancelled
    ///
    /// # Arguments
    ///
    /// * `duration_ms` - Execution duration in milliseconds before cancellation
    pub fn cancel(&mut self, duration_ms: u64) {
        self.status = QueryStatus::Cancelled;
        self.completed_at = Some(Utc::now());
        self.duration_ms = Some(duration_ms);
    }
}

/// Query log filter options
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryLogFilter {
    /// Filter by connection ID
    pub connection_id: Option<String>,

    /// Filter by database name
    pub database: Option<String>,

    /// Filter by query type
    pub query_type: Option<QueryType>,

    /// Filter by status
    pub status: Option<QueryStatus>,

    /// Filter by minimum duration (ms)
    pub min_duration: Option<u64>,

    /// Filter by maximum duration (ms)
    pub max_duration: Option<u64>,

    /// Filter by date range start (ISO 8601)
    pub start_date: Option<String>,

    /// Filter by date range end (ISO 8601)
    pub end_date: Option<String>,

    /// Search text in query SQL
    pub search_text: Option<String>,

    /// Filter by tags
    pub tags: Option<Vec<String>>,
}

impl QueryLogFilter {
    /// Check if a query log matches this filter
    ///
    /// # Arguments
    ///
    /// * `log` - Query log to check
    ///
    /// # Returns
    ///
    /// true if the log matches all filter criteria
    pub fn matches(&self, log: &QueryLog) -> bool {
        // Connection ID filter
        if let Some(ref conn_id) = self.connection_id {
            if &log.connection_id != conn_id {
                return false;
            }
        }

        // Database filter
        if let Some(ref db) = self.database {
            if log.database.as_ref() != Some(db) {
                return false;
            }
        }

        // Query type filter
        if let Some(qt) = self.query_type {
            if log.query_type != qt {
                return false;
            }
        }

        // Status filter
        if let Some(status) = self.status {
            if log.status != status {
                return false;
            }
        }

        // Duration filters
        if let Some(min_dur) = self.min_duration {
            if log.duration_ms.map(|d| d < min_dur).unwrap_or(true) {
                return false;
            }
        }

        if let Some(max_dur) = self.max_duration {
            if log.duration_ms.map(|d| d > max_dur).unwrap_or(false) {
                return false;
            }
        }

        // Date range filters
        if let Some(ref start) = self.start_date {
            if let Ok(start_dt) = DateTime::parse_from_rfc3339(start) {
                if log.started_at < start_dt.with_timezone(&Utc) {
                    return false;
                }
            }
        }

        if let Some(ref end) = self.end_date {
            if let Ok(end_dt) = DateTime::parse_from_rfc3339(end) {
                if log.started_at > end_dt.with_timezone(&Utc) {
                    return false;
                }
            }
        }

        // Search text filter
        if let Some(ref search) = self.search_text {
            if !log.sql.to_lowercase().contains(&search.to_lowercase()) {
                return false;
            }
        }

        // Tags filter
        if let Some(ref filter_tags) = self.tags {
            if let Some(ref log_tags) = log.tags {
                if !filter_tags.iter().any(|tag| log_tags.contains(tag)) {
                    return false;
                }
            } else {
                return false;
            }
        }

        true
    }
}

/// Sort field for query logs
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QueryLogSortField {
    /// Sort by start timestamp
    StartedAt,
    /// Sort by duration
    DurationMs,
    /// Sort by row count
    RowCount,
    /// Sort by connection name
    ConnectionName,
}

/// Sort direction
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortDirection {
    /// Ascending order
    Asc,
    /// Descending order
    Desc,
}

/// Sort options for query logs
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryLogSort {
    /// Field to sort by
    pub field: QueryLogSortField,
    /// Sort direction
    pub direction: SortDirection,
}

impl Default for QueryLogSort {
    fn default() -> Self {
        Self {
            field: QueryLogSortField::StartedAt,
            direction: SortDirection::Desc,
        }
    }
}

/// Paginated query log response
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryLogResponse {
    /// Query logs for this page
    pub logs: Vec<QueryLog>,
    /// Total number of logs matching filter
    pub total: usize,
    /// Current page number (0-indexed)
    pub page: usize,
    /// Page size
    pub page_size: usize,
    /// Total number of pages
    pub total_pages: usize,
}

/// Activity statistics
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityStats {
    /// Total queries executed
    pub total_queries: usize,
    /// Total failed queries
    pub failed_queries: usize,
    /// Average query duration (ms)
    pub avg_duration: f64,
    /// Total rows affected
    pub total_rows: u64,
    /// Queries by type
    pub queries_by_type: std::collections::HashMap<QueryType, usize>,
    /// Queries by status
    pub queries_by_status: std::collections::HashMap<String, usize>,
}

/// Export format options
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    /// JSON format
    Json,
    /// CSV format
    Csv,
    /// Plain text format
    Txt,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_type_from_sql_select() {
        assert_eq!(QueryType::from_sql("SELECT * FROM users"), QueryType::Select);
        assert_eq!(
            QueryType::from_sql("  select id, name from products"),
            QueryType::Select
        );
        assert_eq!(
            QueryType::from_sql("WITH cte AS (SELECT 1) SELECT * FROM cte"),
            QueryType::Select
        );
    }

    #[test]
    fn test_query_type_from_sql_dml() {
        assert_eq!(
            QueryType::from_sql("INSERT INTO users VALUES (1, 'John')"),
            QueryType::Insert
        );
        assert_eq!(
            QueryType::from_sql("UPDATE users SET name = 'Jane'"),
            QueryType::Update
        );
        assert_eq!(
            QueryType::from_sql("DELETE FROM users WHERE id = 1"),
            QueryType::Delete
        );
    }

    #[test]
    fn test_query_type_from_sql_ddl() {
        assert_eq!(
            QueryType::from_sql("CREATE TABLE users (id INT)"),
            QueryType::Create
        );
        assert_eq!(
            QueryType::from_sql("ALTER TABLE users ADD COLUMN email VARCHAR"),
            QueryType::Alter
        );
        assert_eq!(QueryType::from_sql("DROP TABLE users"), QueryType::Drop);
    }

    #[test]
    fn test_query_type_from_sql_transaction() {
        assert_eq!(QueryType::from_sql("BEGIN"), QueryType::Transaction);
        assert_eq!(QueryType::from_sql("COMMIT"), QueryType::Transaction);
        assert_eq!(QueryType::from_sql("ROLLBACK"), QueryType::Transaction);
        assert_eq!(
            QueryType::from_sql("START TRANSACTION"),
            QueryType::Transaction
        );
    }

    #[test]
    fn test_query_type_from_sql_other() {
        assert_eq!(QueryType::from_sql("EXPLAIN SELECT * FROM users"), QueryType::Other);
        assert_eq!(QueryType::from_sql("SET work_mem = '64MB'"), QueryType::Other);
    }

    #[test]
    fn test_query_log_creation() {
        let log = QueryLog::new(
            "log-1".to_string(),
            "conn-1".to_string(),
            "Test DB".to_string(),
            Some("mydb".to_string()),
            "SELECT * FROM users".to_string(),
        );

        assert_eq!(log.id, "log-1");
        assert_eq!(log.connection_id, "conn-1");
        assert_eq!(log.connection_name, "Test DB");
        assert_eq!(log.database, Some("mydb".to_string()));
        assert_eq!(log.query_type, QueryType::Select);
        assert_eq!(log.status, QueryStatus::Running);
        assert!(log.completed_at.is_none());
    }

    #[test]
    fn test_query_log_complete() {
        let mut log = QueryLog::new(
            "log-1".to_string(),
            "conn-1".to_string(),
            "Test DB".to_string(),
            None,
            "SELECT * FROM users".to_string(),
        );

        log.complete(150, Some(10));

        assert_eq!(log.status, QueryStatus::Completed);
        assert!(log.completed_at.is_some());
        assert_eq!(log.duration_ms, Some(150));
        assert_eq!(log.row_count, Some(10));
        assert!(log.error.is_none());
    }

    #[test]
    fn test_query_log_fail() {
        let mut log = QueryLog::new(
            "log-1".to_string(),
            "conn-1".to_string(),
            "Test DB".to_string(),
            None,
            "SELECT * FROM users".to_string(),
        );

        log.fail(50, "Table does not exist".to_string());

        assert_eq!(log.status, QueryStatus::Failed);
        assert!(log.completed_at.is_some());
        assert_eq!(log.duration_ms, Some(50));
        assert_eq!(log.error, Some("Table does not exist".to_string()));
    }

    #[test]
    fn test_query_log_filter_matches() {
        let log = QueryLog::new(
            "log-1".to_string(),
            "conn-1".to_string(),
            "Test DB".to_string(),
            Some("mydb".to_string()),
            "SELECT * FROM users".to_string(),
        );

        // Empty filter matches all
        let filter = QueryLogFilter::default();
        assert!(filter.matches(&log));

        // Connection ID filter
        let filter = QueryLogFilter {
            connection_id: Some("conn-1".to_string()),
            ..Default::default()
        };
        assert!(filter.matches(&log));

        let filter = QueryLogFilter {
            connection_id: Some("conn-2".to_string()),
            ..Default::default()
        };
        assert!(!filter.matches(&log));

        // Query type filter
        let filter = QueryLogFilter {
            query_type: Some(QueryType::Select),
            ..Default::default()
        };
        assert!(filter.matches(&log));

        let filter = QueryLogFilter {
            query_type: Some(QueryType::Insert),
            ..Default::default()
        };
        assert!(!filter.matches(&log));

        // Search text filter
        let filter = QueryLogFilter {
            search_text: Some("users".to_string()),
            ..Default::default()
        };
        assert!(filter.matches(&log));

        let filter = QueryLogFilter {
            search_text: Some("products".to_string()),
            ..Default::default()
        };
        assert!(!filter.matches(&log));
    }
}
