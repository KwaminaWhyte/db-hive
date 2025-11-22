//! Activity logger for tracking query execution
//!
//! This module provides the ActivityLogger structure for managing query logs
//! in memory with thread-safe access, filtering, sorting, and statistics.

use crate::models::{
    ActivityStats, QueryLog, QueryLogFilter, QueryLogResponse, QueryLogSort, QueryLogSortField,
    QueryStatus, QueryType, SortDirection,
};
use chrono::{Duration, Utc};
use std::collections::HashMap;
use std::sync::RwLock;

/// Activity logger for managing query logs
///
/// Provides thread-safe access to query logs with filtering, sorting, pagination,
/// and automatic cleanup of old logs based on retention period.
pub struct ActivityLogger {
    /// Query logs stored in memory
    logs: RwLock<Vec<QueryLog>>,

    /// Retention period in days (logs older than this are removed)
    retention_days: u32,
}

impl ActivityLogger {
    /// Create a new ActivityLogger with a retention period
    ///
    /// # Arguments
    ///
    /// * `retention_days` - Number of days to retain logs
    ///
    /// # Returns
    ///
    /// A new ActivityLogger instance
    pub fn new(retention_days: u32) -> Self {
        Self {
            logs: RwLock::new(Vec::new()),
            retention_days,
        }
    }

    /// Log the start of a query execution
    ///
    /// # Arguments
    ///
    /// * `log` - Query log entry to add
    pub fn log_query_start(&self, log: QueryLog) {
        let mut logs = self.logs.write().unwrap();
        logs.push(log);
    }

    /// Update a query log when it completes successfully
    ///
    /// # Arguments
    ///
    /// * `id` - Query log ID to update
    /// * `duration_ms` - Execution duration in milliseconds
    /// * `row_count` - Number of rows affected/returned
    ///
    /// # Returns
    ///
    /// true if the log was found and updated, false otherwise
    pub fn log_query_complete(&self, id: &str, duration_ms: u64, row_count: Option<u64>) -> bool {
        let mut logs = self.logs.write().unwrap();
        if let Some(log) = logs.iter_mut().find(|l| l.id == id) {
            log.complete(duration_ms, row_count);
            true
        } else {
            false
        }
    }

    /// Update a query log when it fails
    ///
    /// # Arguments
    ///
    /// * `id` - Query log ID to update
    /// * `duration_ms` - Execution duration in milliseconds before failure
    /// * `error` - Error message
    ///
    /// # Returns
    ///
    /// true if the log was found and updated, false otherwise
    pub fn log_query_error(&self, id: &str, duration_ms: u64, error: String) -> bool {
        let mut logs = self.logs.write().unwrap();
        if let Some(log) = logs.iter_mut().find(|l| l.id == id) {
            log.fail(duration_ms, error);
            true
        } else {
            false
        }
    }

    /// Update a query log when it is cancelled
    ///
    /// # Arguments
    ///
    /// * `id` - Query log ID to update
    /// * `duration_ms` - Execution duration in milliseconds before cancellation
    ///
    /// # Returns
    ///
    /// true if the log was found and updated, false otherwise
    pub fn log_query_cancel(&self, id: &str, duration_ms: u64) -> bool {
        let mut logs = self.logs.write().unwrap();
        if let Some(log) = logs.iter_mut().find(|l| l.id == id) {
            log.cancel(duration_ms);
            true
        } else {
            false
        }
    }

    /// Get logs with filtering, sorting, and pagination
    ///
    /// # Arguments
    ///
    /// * `filter` - Filter criteria (optional)
    /// * `sort` - Sort options (optional)
    /// * `page` - Page number (0-indexed)
    /// * `page_size` - Number of logs per page
    ///
    /// # Returns
    ///
    /// QueryLogResponse with paginated results
    pub fn get_logs(
        &self,
        filter: Option<QueryLogFilter>,
        sort: Option<QueryLogSort>,
        page: usize,
        page_size: usize,
    ) -> QueryLogResponse {
        let logs = self.logs.read().unwrap();

        // Apply filter
        let filtered_logs: Vec<QueryLog> = if let Some(ref filter) = filter {
            logs.iter()
                .filter(|log| filter.matches(log))
                .cloned()
                .collect()
        } else {
            logs.clone()
        };

        let total = filtered_logs.len();

        // Apply sorting
        let mut sorted_logs = filtered_logs;
        let sort = sort.unwrap_or_default();
        match sort.field {
            QueryLogSortField::StartedAt => {
                sorted_logs.sort_by_key(|log| log.started_at);
            }
            QueryLogSortField::DurationMs => {
                sorted_logs.sort_by_key(|log| log.duration_ms);
            }
            QueryLogSortField::RowCount => {
                sorted_logs.sort_by_key(|log| log.row_count);
            }
            QueryLogSortField::ConnectionName => {
                sorted_logs.sort_by(|a, b| a.connection_name.cmp(&b.connection_name));
            }
        }

        // Reverse if descending
        if matches!(sort.direction, SortDirection::Desc) {
            sorted_logs.reverse();
        }

        // Apply pagination
        let start = page * page_size;
        let end = (start + page_size).min(total);
        let page_logs = sorted_logs[start..end].to_vec();

        let total_pages = (total + page_size - 1) / page_size;

        QueryLogResponse {
            logs: page_logs,
            total,
            page,
            page_size,
            total_pages,
        }
    }

    /// Calculate activity statistics
    ///
    /// # Arguments
    ///
    /// * `filter` - Filter criteria (optional)
    ///
    /// # Returns
    ///
    /// ActivityStats with calculated statistics
    pub fn get_stats(&self, filter: Option<QueryLogFilter>) -> ActivityStats {
        let logs = self.logs.read().unwrap();

        // Apply filter
        let filtered_logs: Vec<&QueryLog> = if let Some(ref filter) = filter {
            logs.iter().filter(|log| filter.matches(log)).collect()
        } else {
            logs.iter().collect()
        };

        let total_queries = filtered_logs.len();
        let failed_queries = filtered_logs
            .iter()
            .filter(|log| log.status == QueryStatus::Failed)
            .count();

        // Calculate average duration (only for completed queries)
        let durations: Vec<u64> = filtered_logs
            .iter()
            .filter_map(|log| log.duration_ms)
            .collect();
        let avg_duration = if !durations.is_empty() {
            durations.iter().sum::<u64>() as f64 / durations.len() as f64
        } else {
            0.0
        };

        // Calculate total rows
        let total_rows = filtered_logs
            .iter()
            .filter_map(|log| log.row_count)
            .sum::<u64>();

        // Count queries by type
        let mut queries_by_type: HashMap<QueryType, usize> = HashMap::new();
        for log in &filtered_logs {
            *queries_by_type.entry(log.query_type).or_insert(0) += 1;
        }

        // Count queries by status
        let mut queries_by_status: HashMap<String, usize> = HashMap::new();
        for log in &filtered_logs {
            let status_str = match log.status {
                QueryStatus::Running => "running",
                QueryStatus::Completed => "completed",
                QueryStatus::Failed => "failed",
                QueryStatus::Cancelled => "cancelled",
            };
            *queries_by_status.entry(status_str.to_string()).or_insert(0) += 1;
        }

        ActivityStats {
            total_queries,
            failed_queries,
            avg_duration,
            total_rows,
            queries_by_type,
            queries_by_status,
        }
    }

    /// Clear logs older than the retention period
    ///
    /// # Returns
    ///
    /// Number of logs removed
    pub fn clear_old_logs(&self) -> usize {
        let mut logs = self.logs.write().unwrap();
        let cutoff = Utc::now() - Duration::days(self.retention_days as i64);
        let original_len = logs.len();

        logs.retain(|log| log.started_at > cutoff);

        original_len - logs.len()
    }

    /// Clear all logs
    ///
    /// # Returns
    ///
    /// Number of logs removed
    pub fn clear_all_logs(&self) -> usize {
        let mut logs = self.logs.write().unwrap();
        let count = logs.len();
        logs.clear();
        count
    }

    /// Get a specific log by ID
    ///
    /// # Arguments
    ///
    /// * `id` - Query log ID
    ///
    /// # Returns
    ///
    /// The query log if found
    pub fn get_log(&self, id: &str) -> Option<QueryLog> {
        let logs = self.logs.read().unwrap();
        logs.iter().find(|log| log.id == id).cloned()
    }

    /// Get the total number of logs
    ///
    /// # Returns
    ///
    /// Number of logs currently stored
    pub fn count(&self) -> usize {
        let logs = self.logs.read().unwrap();
        logs.len()
    }

    /// Update log tags
    ///
    /// # Arguments
    ///
    /// * `id` - Query log ID
    /// * `tags` - New tags to set
    ///
    /// # Returns
    ///
    /// true if the log was found and updated, false otherwise
    pub fn update_tags(&self, id: &str, tags: Vec<String>) -> bool {
        let mut logs = self.logs.write().unwrap();
        if let Some(log) = logs.iter_mut().find(|l| l.id == id) {
            log.tags = Some(tags);
            true
        } else {
            false
        }
    }

    /// Get all logs (for export purposes)
    ///
    /// # Arguments
    ///
    /// * `filter` - Filter criteria (optional)
    ///
    /// # Returns
    ///
    /// Vector of filtered logs
    pub fn get_all_logs(&self, filter: Option<QueryLogFilter>) -> Vec<QueryLog> {
        let logs = self.logs.read().unwrap();

        if let Some(ref filter) = filter {
            logs.iter()
                .filter(|log| filter.matches(log))
                .cloned()
                .collect()
        } else {
            logs.clone()
        }
    }
}

impl Default for ActivityLogger {
    fn default() -> Self {
        Self::new(7) // 7 days retention by default
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_test_log(id: &str, connection_id: &str, sql: &str) -> QueryLog {
        QueryLog::new(
            id.to_string(),
            connection_id.to_string(),
            "Test Connection".to_string(),
            Some("testdb".to_string()),
            sql.to_string(),
        )
    }

    #[test]
    fn test_activity_logger_new() {
        let logger = ActivityLogger::new(7);
        assert_eq!(logger.count(), 0);
    }

    #[test]
    fn test_log_query_start() {
        let logger = ActivityLogger::new(7);
        let log = create_test_log("log-1", "conn-1", "SELECT * FROM users");

        logger.log_query_start(log);
        assert_eq!(logger.count(), 1);
    }

    #[test]
    fn test_log_query_complete() {
        let logger = ActivityLogger::new(7);
        let log = create_test_log("log-1", "conn-1", "SELECT * FROM users");

        logger.log_query_start(log);
        let updated = logger.log_query_complete("log-1", 150, Some(10));

        assert!(updated);
        let retrieved = logger.get_log("log-1").unwrap();
        assert_eq!(retrieved.status, QueryStatus::Completed);
        assert_eq!(retrieved.duration_ms, Some(150));
        assert_eq!(retrieved.row_count, Some(10));
    }

    #[test]
    fn test_log_query_error() {
        let logger = ActivityLogger::new(7);
        let log = create_test_log("log-1", "conn-1", "SELECT * FROM users");

        logger.log_query_start(log);
        let updated = logger.log_query_error("log-1", 50, "Table not found".to_string());

        assert!(updated);
        let retrieved = logger.get_log("log-1").unwrap();
        assert_eq!(retrieved.status, QueryStatus::Failed);
        assert_eq!(retrieved.error, Some("Table not found".to_string()));
    }

    #[test]
    fn test_get_logs_pagination() {
        let logger = ActivityLogger::new(7);

        // Add multiple logs
        for i in 0..25 {
            let log = create_test_log(&format!("log-{}", i), "conn-1", "SELECT * FROM users");
            logger.log_query_start(log);
        }

        // Get first page
        let response = logger.get_logs(None, None, 0, 10);
        assert_eq!(response.logs.len(), 10);
        assert_eq!(response.total, 25);
        assert_eq!(response.total_pages, 3);

        // Get last page
        let response = logger.get_logs(None, None, 2, 10);
        assert_eq!(response.logs.len(), 5);
        assert_eq!(response.total, 25);
    }

    #[test]
    fn test_get_logs_with_filter() {
        let logger = ActivityLogger::new(7);

        let log1 = create_test_log("log-1", "conn-1", "SELECT * FROM users");
        let log2 = create_test_log("log-2", "conn-2", "INSERT INTO products VALUES (1)");
        let log3 = create_test_log("log-3", "conn-1", "UPDATE users SET name = 'John'");

        logger.log_query_start(log1);
        logger.log_query_start(log2);
        logger.log_query_start(log3);

        // Filter by connection ID
        let filter = QueryLogFilter {
            connection_id: Some("conn-1".to_string()),
            ..Default::default()
        };
        let response = logger.get_logs(Some(filter), None, 0, 10);
        assert_eq!(response.total, 2);
    }

    #[test]
    fn test_get_stats() {
        let logger = ActivityLogger::new(7);

        let mut log1 = create_test_log("log-1", "conn-1", "SELECT * FROM users");
        let mut log2 = create_test_log("log-2", "conn-1", "INSERT INTO products VALUES (1)");
        let log3 = create_test_log("log-3", "conn-1", "SELECT * FROM orders");

        log1.complete(100, Some(5));
        log2.fail(50, "Error".to_string());

        logger.log_query_start(log1);
        logger.log_query_start(log2);
        logger.log_query_start(log3);

        let stats = logger.get_stats(None);
        assert_eq!(stats.total_queries, 3);
        assert_eq!(stats.failed_queries, 1);
        assert_eq!(stats.total_rows, 5);
        assert!(stats.queries_by_type.contains_key(&QueryType::Select));
        assert!(stats.queries_by_type.contains_key(&QueryType::Insert));
    }

    #[test]
    fn test_clear_all_logs() {
        let logger = ActivityLogger::new(7);

        for i in 0..10 {
            let log = create_test_log(&format!("log-{}", i), "conn-1", "SELECT * FROM users");
            logger.log_query_start(log);
        }

        assert_eq!(logger.count(), 10);
        let removed = logger.clear_all_logs();
        assert_eq!(removed, 10);
        assert_eq!(logger.count(), 0);
    }

    #[test]
    fn test_update_tags() {
        let logger = ActivityLogger::new(7);
        let log = create_test_log("log-1", "conn-1", "SELECT * FROM users");

        logger.log_query_start(log);
        let updated = logger.update_tags("log-1", vec!["slow".to_string(), "production".to_string()]);

        assert!(updated);
        let retrieved = logger.get_log("log-1").unwrap();
        assert_eq!(retrieved.tags, Some(vec!["slow".to_string(), "production".to_string()]));
    }
}
