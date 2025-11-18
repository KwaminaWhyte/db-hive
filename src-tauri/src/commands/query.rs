//! Query execution commands
//!
//! This module provides Tauri commands for executing SQL queries against
//! active database connections. It handles query execution, timing, and
//! result formatting.

use std::sync::Mutex;
use std::time::Instant;
use tauri::State;

use crate::models::DbError;
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
    ) -> Self {
        Self {
            columns: query_result.columns,
            rows: query_result.rows,
            rows_affected: query_result.rows_affected,
            execution_time: execution_time_ms,
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
    // Get the connection from state
    let connection = {
        let state_guard = state.lock().unwrap();
        state_guard
            .get_connection(&connection_id)
            .ok_or_else(|| {
                DbError::NotFound(format!("Connection with ID {} not found", connection_id))
            })?
            .clone()
    };

    // Measure execution time
    let start = Instant::now();

    // Execute the query
    let query_result = connection.execute_query(&sql).await?;

    // Calculate execution time in milliseconds
    let execution_time_ms = start.elapsed().as_millis() as u64;

    // Convert QueryResult to QueryExecutionResult
    let result = QueryExecutionResult::from_query_result(query_result, execution_time_ms);

    Ok(result)
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
        let execution_result = QueryExecutionResult::from_query_result(query_result, 150);

        assert_eq!(execution_result.columns, columns);
        assert_eq!(execution_result.rows, rows);
        assert_eq!(execution_result.rows_affected, None);
        assert_eq!(execution_result.execution_time, 150);
    }

    #[test]
    fn test_query_execution_result_from_query_result_with_affected() {
        let query_result = QueryResult::with_affected(5);
        let execution_result = QueryExecutionResult::from_query_result(query_result, 50);

        assert_eq!(execution_result.columns.len(), 0);
        assert_eq!(execution_result.rows.len(), 0);
        assert_eq!(execution_result.rows_affected, Some(5));
        assert_eq!(execution_result.execution_time, 50);
    }

    #[test]
    fn test_query_execution_result_empty() {
        let query_result = QueryResult::empty();
        let execution_result = QueryExecutionResult::from_query_result(query_result, 10);

        assert_eq!(execution_result.columns.len(), 0);
        assert_eq!(execution_result.rows.len(), 0);
        assert_eq!(execution_result.rows_affected, None);
        assert_eq!(execution_result.execution_time, 10);
    }

    #[test]
    fn test_query_execution_result_serialization() {
        let columns = vec!["id".to_string(), "name".to_string()];
        let rows = vec![vec![serde_json::json!(1), serde_json::json!("Alice")]];

        let query_result = QueryResult::with_data(columns, rows);
        let execution_result = QueryExecutionResult::from_query_result(query_result, 100);

        // Test that it can be serialized to JSON
        let json = serde_json::to_string(&execution_result);
        assert!(json.is_ok());

        // Verify camelCase naming in JSON
        let json_str = json.unwrap();
        assert!(json_str.contains("executionTime"));
        assert!(json_str.contains("rowsAffected"));
        assert!(!json_str.contains("execution_time"));
        assert!(!json_str.contains("rows_affected"));
    }

    // Note: Integration tests for execute_query command would require
    // a real or mock database connection. These are better placed in
    // integration tests with actual database drivers or mocked drivers.
}
