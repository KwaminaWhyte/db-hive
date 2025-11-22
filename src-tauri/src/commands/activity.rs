//! Activity logging commands
//!
//! This module provides Tauri commands for retrieving query logs, calculating
//! statistics, and managing activity data.

use std::sync::Mutex;
use tauri::State;

use crate::models::{
    ActivityStats, DbError, ExportFormat, QueryLogFilter, QueryLogResponse, QueryLogSort,
};
use crate::state::AppState;

/// Get query logs with filtering, sorting, and pagination
///
/// # Arguments
///
/// * `filter` - Filter criteria (optional)
/// * `sort` - Sort options (optional)
/// * `page` - Page number (0-indexed)
/// * `page_size` - Number of logs per page
/// * `state` - Application state
///
/// # Returns
///
/// QueryLogResponse with paginated results
///
/// # Example
///
/// ```typescript
/// import { invoke } from '@tauri-apps/api/core';
///
/// const response = await invoke<QueryLogResponse>('get_query_logs', {
///     filter: { connectionId: 'conn-123', status: 'completed' },
///     sort: { field: 'startedAt', direction: 'desc' },
///     page: 0,
///     pageSize: 20
/// });
/// ```
#[tauri::command]
pub async fn get_query_logs(
    filter: Option<QueryLogFilter>,
    sort: Option<QueryLogSort>,
    page: usize,
    page_size: usize,
    state: State<'_, Mutex<AppState>>,
) -> Result<QueryLogResponse, DbError> {
    let state_guard = state.lock().unwrap();
    let response = state_guard
        .activity_logger
        .get_logs(filter, sort, page, page_size);
    Ok(response)
}

/// Get activity statistics
///
/// # Arguments
///
/// * `filter` - Filter criteria (optional)
/// * `state` - Application state
///
/// # Returns
///
/// ActivityStats with calculated statistics
///
/// # Example
///
/// ```typescript
/// import { invoke } from '@tauri-apps/api/core';
///
/// const stats = await invoke<ActivityStats>('get_activity_stats', {
///     filter: { connectionId: 'conn-123' }
/// });
///
/// console.log(`Total queries: ${stats.totalQueries}`);
/// console.log(`Failed queries: ${stats.failedQueries}`);
/// console.log(`Average duration: ${stats.avgDuration}ms`);
/// ```
#[tauri::command]
pub async fn get_activity_stats(
    filter: Option<QueryLogFilter>,
    state: State<'_, Mutex<AppState>>,
) -> Result<ActivityStats, DbError> {
    let state_guard = state.lock().unwrap();
    let stats = state_guard.activity_logger.get_stats(filter);
    Ok(stats)
}

/// Clear all query logs
///
/// # Arguments
///
/// * `state` - Application state
///
/// # Returns
///
/// Number of logs cleared
///
/// # Example
///
/// ```typescript
/// import { invoke } from '@tauri-apps/api/core';
///
/// const count = await invoke<number>('clear_query_logs', {});
/// console.log(`Cleared ${count} logs`);
/// ```
#[tauri::command]
pub async fn clear_query_logs(state: State<'_, Mutex<AppState>>) -> Result<usize, DbError> {
    let state_guard = state.lock().unwrap();
    let count = state_guard.activity_logger.clear_all_logs();
    Ok(count)
}

/// Clear old query logs (older than retention period)
///
/// # Arguments
///
/// * `state` - Application state
///
/// # Returns
///
/// Number of logs cleared
///
/// # Example
///
/// ```typescript
/// import { invoke } from '@tauri-apps/api/core';
///
/// const count = await invoke<number>('clear_old_query_logs', {});
/// console.log(`Cleared ${count} old logs`);
/// ```
#[tauri::command]
pub async fn clear_old_query_logs(state: State<'_, Mutex<AppState>>) -> Result<usize, DbError> {
    let state_guard = state.lock().unwrap();
    let count = state_guard.activity_logger.clear_old_logs();
    Ok(count)
}

/// Export query logs to a file
///
/// # Arguments
///
/// * `filter` - Filter criteria (optional)
/// * `format` - Export format (json, csv, txt)
/// * `file_path` - Path to save the export file
/// * `state` - Application state
///
/// # Returns
///
/// The file path where data was exported
///
/// # Example
///
/// ```typescript
/// import { invoke } from '@tauri-apps/api/core';
/// import { save } from '@tauri-apps/plugin-dialog';
///
/// const filePath = await save({
///     defaultPath: 'query-logs.json',
///     filters: [{ name: 'JSON', extensions: ['json'] }]
/// });
///
/// if (filePath) {
///     const result = await invoke<string>('export_query_logs', {
///         filter: null,
///         format: 'json',
///         filePath
///     });
/// }
/// ```
#[tauri::command]
pub async fn export_query_logs(
    filter: Option<QueryLogFilter>,
    format: ExportFormat,
    file_path: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, DbError> {
    let state_guard = state.lock().unwrap();
    let logs = state_guard.activity_logger.get_all_logs(filter);

    // Export based on format
    match format {
        ExportFormat::Json => {
            let json = serde_json::to_string_pretty(&logs).map_err(|e| {
                DbError::InternalError(format!("Failed to serialize logs to JSON: {}", e))
            })?;
            std::fs::write(&file_path, json).map_err(|e| {
                DbError::InternalError(format!("Failed to write JSON file: {}", e))
            })?;
        }
        ExportFormat::Csv => {
            let mut csv = String::new();
            csv.push_str("ID,Connection ID,Connection Name,Database,SQL,Query Type,Status,Started At,Completed At,Duration (ms),Row Count,Error\n");

            for log in logs {
                let line = format!(
                    "{},{},{},{},{},{:?},{:?},{},{},{},{},{}\n",
                    log.id,
                    log.connection_id,
                    log.connection_name,
                    log.database.as_deref().unwrap_or(""),
                    log.sql.replace(',', ";").replace('\n', " "),
                    log.query_type,
                    log.status,
                    log.started_at.to_rfc3339(),
                    log.completed_at
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_default(),
                    log.duration_ms.map(|d| d.to_string()).unwrap_or_default(),
                    log.row_count.map(|r| r.to_string()).unwrap_or_default(),
                    log.error.as_deref().unwrap_or("")
                );
                csv.push_str(&line);
            }

            std::fs::write(&file_path, csv).map_err(|e| {
                DbError::InternalError(format!("Failed to write CSV file: {}", e))
            })?;
        }
        ExportFormat::Txt => {
            let mut txt = String::new();
            txt.push_str("=== Query Logs Export ===\n\n");

            for log in logs {
                txt.push_str(&format!("ID: {}\n", log.id));
                txt.push_str(&format!("Connection: {}\n", log.connection_name));
                if let Some(ref db) = log.database {
                    txt.push_str(&format!("Database: {}\n", db));
                }
                txt.push_str(&format!("Query Type: {:?}\n", log.query_type));
                txt.push_str(&format!("Status: {:?}\n", log.status));
                txt.push_str(&format!("Started At: {}\n", log.started_at.to_rfc3339()));
                if let Some(completed) = log.completed_at {
                    txt.push_str(&format!("Completed At: {}\n", completed.to_rfc3339()));
                }
                if let Some(duration) = log.duration_ms {
                    txt.push_str(&format!("Duration: {}ms\n", duration));
                }
                if let Some(rows) = log.row_count {
                    txt.push_str(&format!("Row Count: {}\n", rows));
                }
                if let Some(ref error) = log.error {
                    txt.push_str(&format!("Error: {}\n", error));
                }
                txt.push_str(&format!("SQL:\n{}\n", log.sql));
                txt.push_str("\n---\n\n");
            }

            std::fs::write(&file_path, txt).map_err(|e| {
                DbError::InternalError(format!("Failed to write TXT file: {}", e))
            })?;
        }
    }

    Ok(file_path)
}

/// Update tags for a query log
///
/// # Arguments
///
/// * `log_id` - Query log ID
/// * `tags` - New tags to set
/// * `state` - Application state
///
/// # Returns
///
/// true if the log was found and updated
///
/// # Example
///
/// ```typescript
/// import { invoke } from '@tauri-apps/api/core';
///
/// const updated = await invoke<boolean>('update_query_log_tags', {
///     logId: 'log-123',
///     tags: ['slow', 'production']
/// });
/// ```
#[tauri::command]
pub async fn update_query_log_tags(
    log_id: String,
    tags: Vec<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<bool, DbError> {
    let state_guard = state.lock().unwrap();
    let updated = state_guard.activity_logger.update_tags(&log_id, tags);
    Ok(updated)
}

/// Get total count of query logs
///
/// # Arguments
///
/// * `state` - Application state
///
/// # Returns
///
/// Total number of logs
///
/// # Example
///
/// ```typescript
/// import { invoke } from '@tauri-apps/api/core';
///
/// const count = await invoke<number>('get_query_logs_count', {});
/// console.log(`Total logs: ${count}`);
/// ```
#[tauri::command]
pub async fn get_query_logs_count(state: State<'_, Mutex<AppState>>) -> Result<usize, DbError> {
    let state_guard = state.lock().unwrap();
    let count = state_guard.activity_logger.count();
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::QueryLog;

    #[test]
    fn test_activity_logger_integration() {
        let state = AppState::default();

        // Test log start
        let log = QueryLog::new(
            "log-1".to_string(),
            "conn-1".to_string(),
            "Test DB".to_string(),
            None,
            "SELECT * FROM users".to_string(),
        );
        state.activity_logger.log_query_start(log);
        assert_eq!(state.activity_logger.count(), 1);

        // Test get logs
        let response = state.activity_logger.get_logs(None, None, 0, 10);
        assert_eq!(response.total, 1);
        assert_eq!(response.logs.len(), 1);

        // Test stats
        let stats = state.activity_logger.get_stats(None);
        assert_eq!(stats.total_queries, 1);
    }

    #[test]
    fn test_query_log_completion() {
        let state = AppState::default();

        let mut log = QueryLog::new(
            "log-1".to_string(),
            "conn-1".to_string(),
            "Test DB".to_string(),
            None,
            "SELECT * FROM users".to_string(),
        );
        log.complete(100, Some(10));

        state.activity_logger.log_query_start(log);

        let stats = state.activity_logger.get_stats(None);
        assert_eq!(stats.total_queries, 1);
        assert_eq!(stats.failed_queries, 0);
        assert_eq!(stats.total_rows, 10);
    }

    #[test]
    fn test_query_log_filtering() {
        let state = AppState::default();

        // Add logs for different connections
        let log1 = QueryLog::new(
            "log-1".to_string(),
            "conn-1".to_string(),
            "Test DB 1".to_string(),
            None,
            "SELECT * FROM users".to_string(),
        );
        let log2 = QueryLog::new(
            "log-2".to_string(),
            "conn-2".to_string(),
            "Test DB 2".to_string(),
            None,
            "INSERT INTO products VALUES (1)".to_string(),
        );

        state.activity_logger.log_query_start(log1);
        state.activity_logger.log_query_start(log2);

        // Filter by connection ID
        let filter = QueryLogFilter {
            connection_id: Some("conn-1".to_string()),
            ..Default::default()
        };
        let response = state.activity_logger.get_logs(Some(filter), None, 0, 10);
        assert_eq!(response.total, 1);
    }
}
