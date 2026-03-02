//! Query execution commands
//!
//! This module provides Tauri commands for executing SQL queries against
//! active database connections. It handles query execution, timing, and
//! result formatting.

use std::sync::Mutex;
use std::time::Instant;
use tauri::State;
use uuid::Uuid;

use crate::models::{DbError, QueryLog};
use crate::state::AppState;
use serde::{Deserialize, Serialize};

/// Result of a query execution
///
/// This structure contains the complete result of executing a SQL query,
/// including the result data, metadata, and performance metrics.
///
/// # Fields
///
/// * `columns` - Names of the columns in the result set (for SELECT queries)
/// * `rows` - The actual data rows, each containing JSON values
/// * `rows_affected` - Number of rows affected (for INSERT/UPDATE/DELETE)
/// * `execution_time` - Time taken to execute the query in milliseconds
/// * `query_type` - The type of query derived from the first SQL keyword (e.g. "SELECT", "INSERT")
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryExecutionResult {
    /// Column names in the result set
    pub columns: Vec<String>,

    /// Rows of data, each row is a vector of JSON values
    pub rows: Vec<Vec<serde_json::Value>>,

    /// Number of rows affected (for INSERT/UPDATE/DELETE operations)
    pub rows_affected: Option<u64>,

    /// Execution time in milliseconds
    pub execution_time: u64,

    /// The type of query derived from the first SQL keyword (e.g. "SELECT", "INSERT", "UPDATE")
    pub query_type: String,
}

impl QueryExecutionResult {
    /// Create a new QueryExecutionResult from a QueryResult and execution time
    ///
    /// # Arguments
    ///
    /// * `query_result` - The raw query result from the database driver
    /// * `execution_time_ms` - Execution time in milliseconds
    ///
    /// # Returns
    ///
    /// A new QueryExecutionResult instance
    pub fn from_query_result(
        query_result: crate::drivers::QueryResult,
        execution_time_ms: u64,
        query_type: String,
    ) -> Self {
        Self {
            columns: query_result.columns,
            rows: query_result.rows,
            rows_affected: query_result.rows_affected,
            execution_time: execution_time_ms,
            query_type,
        }
    }
}

/// Execute a SQL query against an active database connection
///
/// This command executes a SQL query using an established database connection.
/// It measures execution time and returns the results along with performance metrics.
///
/// # Arguments
///
/// * `connection_id` - ID of the active database connection to use
/// * `sql` - SQL query string to execute
/// * `state` - Application state containing active connections
///
/// # Returns
///
/// Returns a `QueryExecutionResult` containing:
/// - Query results (columns and rows)
/// - Number of affected rows (for DML statements)
/// - Execution time in milliseconds
///
/// # Errors
///
/// Returns `DbError` if:
/// - The connection ID is not found
/// - The query execution fails
/// - The database driver encounters an error
///
/// # Example
///
/// ```typescript
/// import { invoke } from '@tauri-apps/api/core';
///
/// const result = await invoke<QueryExecutionResult>('execute_query', {
///     connectionId: 'conn-123',
///     sql: 'SELECT * FROM users WHERE id = 1'
/// });
///
/// console.log(`Query took ${result.executionTime}ms`);
/// console.log(`Found ${result.rows.length} rows`);
/// ```
#[tauri::command]
pub async fn execute_query(
    connection_id: String,
    sql: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<QueryExecutionResult, DbError> {
    // Generate a unique log ID
    let log_id = Uuid::new_v4().to_string();

    // Get the connection and connection name from state, and start logging
    let connection = {
        let state_guard = state.lock().unwrap();

        let connection = state_guard
            .get_connection(&connection_id)
            .ok_or_else(|| {
                DbError::NotFound(format!("Connection with ID {} not found", connection_id))
            })?
            .clone();

        // Get connection profile to get the connection name
        let profile = state_guard
            .connection_profiles
            .values()
            .find(|p| {
                // Find profile by checking if any active connection uses this profile
                state_guard.connections.contains_key(&p.id)
            });

        let connection_name = profile
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "Unknown Connection".to_string());

        let database = profile
            .and_then(|p| p.database.clone());

        // Create and log the query start
        let query_log = QueryLog::new(
            log_id.clone(),
            connection_id.clone(),
            connection_name.clone(),
            database.clone(),
            sql.clone(),
        );
        state_guard.activity_logger.log_query_start(query_log);

        connection
    };

    // Measure execution time
    let start = Instant::now();

    // Execute the query
    let query_result = connection.execute_query(&sql).await;

    // Calculate execution time in milliseconds
    let execution_time_ms = start.elapsed().as_millis() as u64;

    // Update the log based on result
    match &query_result {
        Ok(result) => {
            let state_guard = state.lock().unwrap();
            let row_count = result.rows_affected.or(Some(result.rows.len() as u64));
            state_guard.activity_logger.log_query_complete(
                &log_id,
                execution_time_ms,
                row_count,
            );
        }
        Err(err) => {
            let state_guard = state.lock().unwrap();
            state_guard.activity_logger.log_query_error(
                &log_id,
                execution_time_ms,
                err.to_string(),
            );
        }
    }

    // Derive the query type from the first keyword of the SQL
    let query_type = sql
        .trim()
        .split_whitespace()
        .next()
        .unwrap_or("UNKNOWN")
        .to_uppercase();

    // Convert QueryResult to QueryExecutionResult
    let result = QueryExecutionResult::from_query_result(query_result?, execution_time_ms, query_type);

    Ok(result)
}

/// Result of a keyset-paginated table data fetch
///
/// Uses keyset (cursor-based) pagination for efficient large table browsing.
/// Unlike offset pagination, keyset pagination does not degrade in performance
/// as the cursor advances through large result sets.
///
/// # Fields
///
/// * `columns` - Names of the columns in the result set
/// * `rows` - The actual data rows, each containing JSON values
/// * `next_cursor` - The cursor value to pass on the next request, or `null` if no more pages
/// * `has_more` - Whether additional rows are available beyond this page
/// * `execution_time` - Time taken to execute the query in milliseconds
/// * `total_fetched` - Number of rows returned in this page
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeysetPageResult {
    /// Column names in the result set
    pub columns: Vec<String>,

    /// Rows of data, each row is a vector of JSON values
    pub rows: Vec<Vec<serde_json::Value>>,

    /// Cursor value for the next page request; `null` when there are no more rows
    pub next_cursor: Option<serde_json::Value>,

    /// Whether more rows exist beyond this page
    pub has_more: bool,

    /// Execution time in milliseconds
    pub execution_time: u64,

    /// Number of rows returned in this page
    pub total_fetched: usize,
}

/// Fetch a page of table data using keyset (cursor-based) pagination
///
/// This command retrieves rows from a table in pages, ordered by `cursor_column`.
/// On the first request pass `cursor_value: null` to start from the beginning.
/// Subsequent requests should pass the `nextCursor` returned by the previous response.
///
/// Keyset pagination is significantly more efficient than offset-based pagination
/// for large tables because the database uses an index seek rather than scanning
/// and discarding all prior rows.
///
/// # Arguments
///
/// * `connection_id` - ID of the active database connection
/// * `schema` - Schema name containing the table
/// * `table` - Table name to query
/// * `cursor_column` - Column used for ordering and pagination (should be indexed)
/// * `cursor_value` - Last seen cursor value from the previous page, or `null` to start from the beginning
/// * `page_size` - Number of rows to return per page
/// * `state` - Application state containing active connections
///
/// # Returns
///
/// Returns a `KeysetPageResult` containing:
/// - The columns and row data for this page
/// - A `next_cursor` value for fetching the next page (null when exhausted)
/// - A `has_more` flag indicating whether additional rows exist
/// - Execution time in milliseconds
///
/// # Errors
///
/// Returns `DbError` if:
/// - The connection ID is not found in the active connections
/// - The query execution fails (e.g., table does not exist, permission denied)
///
/// # Example
///
/// ```typescript
/// import { invoke } from '@tauri-apps/api/core';
///
/// // First page
/// let page = await invoke<KeysetPageResult>('get_table_data_keyset', {
///     connectionId: 'conn-123',
///     schema: 'public',
///     table: 'users',
///     cursorColumn: 'id',
///     cursorValue: null,
///     pageSize: 100,
/// });
///
/// // Next page
/// if (page.hasMore) {
///     page = await invoke<KeysetPageResult>('get_table_data_keyset', {
///         connectionId: 'conn-123',
///         schema: 'public',
///         table: 'users',
///         cursorColumn: 'id',
///         cursorValue: page.nextCursor,
///         pageSize: 100,
///     });
/// }
/// ```
#[tauri::command]
pub async fn get_table_data_keyset(
    connection_id: String,
    schema: String,
    table: String,
    cursor_column: String,
    cursor_value: Option<serde_json::Value>,
    page_size: u64,
    state: State<'_, Mutex<AppState>>,
) -> Result<KeysetPageResult, DbError> {
    // Clone the Arc<dyn DatabaseDriver> out of the state before any await points
    let connection = {
        let state_guard = state.lock().unwrap();
        state_guard
            .get_connection(&connection_id)
            .ok_or_else(|| {
                DbError::NotFound(format!("Connection with ID {} not found", connection_id))
            })?
            .clone()
    };

    // Build the paginated SQL query.
    // We fetch page_size + 1 rows so we can determine whether more rows exist
    // without a separate COUNT query.
    let sql = match &cursor_value {
        None => {
            // First page: no WHERE clause, just ORDER BY + LIMIT
            format!(
                r#"SELECT * FROM "{schema}"."{table}" ORDER BY "{cursor_column}" LIMIT {limit}"#,
                schema = schema,
                table = table,
                cursor_column = cursor_column,
                limit = page_size + 1,
            )
        }
        Some(serde_json::Value::Null) | Some(serde_json::Value::Bool(_)) => {
            // Null or boolean cursor — treat as "start from beginning"
            format!(
                r#"SELECT * FROM "{schema}"."{table}" ORDER BY "{cursor_column}" LIMIT {limit}"#,
                schema = schema,
                table = table,
                cursor_column = cursor_column,
                limit = page_size + 1,
            )
        }
        Some(val) => {
            // Subsequent pages: add a WHERE clause filtering rows after the cursor
            format!(
                r#"SELECT * FROM "{schema}"."{table}" WHERE "{cursor_column}" > {cursor_val} ORDER BY "{cursor_column}" LIMIT {limit}"#,
                schema = schema,
                table = table,
                cursor_column = cursor_column,
                cursor_val = val.to_string(),
                limit = page_size + 1,
            )
        }
    };

    // Measure execution time
    let start = Instant::now();
    let query_result = connection.execute_query(&sql).await?;
    let execution_time_ms = start.elapsed().as_millis() as u64;

    let columns = query_result.columns.clone();
    let mut rows = query_result.rows;

    // Determine whether more rows exist and trim the extra sentinel row
    let has_more = rows.len() > page_size as usize;
    if has_more {
        rows.truncate(page_size as usize);
    }

    // Derive the next cursor from the cursor_column value in the last returned row
    let next_cursor = if has_more {
        // Find the index of cursor_column in the columns list
        let cursor_idx = columns.iter().position(|c| c == &cursor_column);
        cursor_idx.and_then(|idx| rows.last().and_then(|row| row.get(idx)).cloned())
    } else {
        None
    };

    let total_fetched = rows.len();

    Ok(KeysetPageResult {
        columns,
        rows,
        next_cursor,
        has_more,
        execution_time: execution_time_ms,
        total_fetched,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::QueryResult;

    #[test]
    fn test_query_execution_result_from_query_result_with_data() {
        let columns = vec!["id".to_string(), "name".to_string()];
        let rows = vec![
            vec![
                serde_json::json!(1),
                serde_json::json!("Alice"),
            ],
            vec![
                serde_json::json!(2),
                serde_json::json!("Bob"),
            ],
        ];

        let query_result = QueryResult::with_data(columns.clone(), rows.clone());
        let execution_result = QueryExecutionResult::from_query_result(query_result, 150, "SELECT".to_string());

        assert_eq!(execution_result.columns, columns);
        assert_eq!(execution_result.rows, rows);
        assert_eq!(execution_result.rows_affected, None);
        assert_eq!(execution_result.execution_time, 150);
        assert_eq!(execution_result.query_type, "SELECT");
    }

    #[test]
    fn test_query_execution_result_from_query_result_with_affected() {
        let query_result = QueryResult::with_affected(5);
        let execution_result = QueryExecutionResult::from_query_result(query_result, 50, "INSERT".to_string());

        assert_eq!(execution_result.columns.len(), 0);
        assert_eq!(execution_result.rows.len(), 0);
        assert_eq!(execution_result.rows_affected, Some(5));
        assert_eq!(execution_result.execution_time, 50);
        assert_eq!(execution_result.query_type, "INSERT");
    }

    #[test]
    fn test_query_execution_result_empty() {
        let query_result = QueryResult::empty();
        let execution_result = QueryExecutionResult::from_query_result(query_result, 10, "UNKNOWN".to_string());

        assert_eq!(execution_result.columns.len(), 0);
        assert_eq!(execution_result.rows.len(), 0);
        assert_eq!(execution_result.rows_affected, None);
        assert_eq!(execution_result.execution_time, 10);
        assert_eq!(execution_result.query_type, "UNKNOWN");
    }

    #[test]
    fn test_query_execution_result_serialization() {
        let columns = vec!["id".to_string(), "name".to_string()];
        let rows = vec![vec![serde_json::json!(1), serde_json::json!("Alice")]];

        let query_result = QueryResult::with_data(columns, rows);
        let execution_result = QueryExecutionResult::from_query_result(query_result, 100, "SELECT".to_string());

        // Test that it can be serialized to JSON
        let json = serde_json::to_string(&execution_result);
        assert!(json.is_ok());

        // Verify camelCase naming in JSON
        let json_str = json.unwrap();
        assert!(json_str.contains("executionTime"));
        assert!(json_str.contains("rowsAffected"));
        assert!(json_str.contains("queryType"));
        assert!(!json_str.contains("execution_time"));
        assert!(!json_str.contains("rows_affected"));
        assert!(!json_str.contains("query_type"));
    }

    // Note: Integration tests for execute_query command would require
    // a real or mock database connection. These are better placed in
    // integration tests with actual database drivers or mocked drivers.
}
