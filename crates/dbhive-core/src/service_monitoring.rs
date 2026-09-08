//! Shell-agnostic monitoring operations.
//!
//! Two things that the React shell treated as unrelated live here together,
//! because the Activity Monitor screen shows them side by side:
//!
//! * the **query log**, an in-process record of statements this application
//!   ran, held by [`crate::activity_logger::ActivityLogger`];
//! * **server-side activity**, read out of the database's own system views
//!   (`pg_stat_activity`, `information_schema.PROCESSLIST`, ...).
//!
//! These are the bodies of the `#[tauri::command]` handlers in
//! `src-tauri/src/commands/monitoring.rs` and `.../activity.rs`, taking a
//! plain `&Mutex<AppState>` instead of Tauri's `State`, so both shells run
//! the same code. See docs/rust-ui-migration/03-conventions.md.
//!
//! Everything that can be a pure function is one: the per-driver SQL, the
//! row mapping, the filter predicate, the sort/paginate step, the statistics
//! roll-up and the metric series maths are all testable without a database.

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::drivers::{DatabaseDriver, QueryResult};
use crate::models::{
    ActivityStats, DbDriver, DbError, ExportFormat, QueryLog, QueryLogFilter, QueryLogResponse,
    QueryLogSort, QueryLogSortField, QueryStatus, QueryType, SortDirection,
};
use crate::state::AppState;

// ===========================================================================
// Server-side activity: types
// ===========================================================================

/// A snapshot of a single active session/query on the database server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveQuery {
    pub pid: i64,
    pub user: Option<String>,
    pub database: Option<String>,
    pub client_addr: Option<String>,
    pub query_start: Option<String>,
    pub state: Option<String>,
    pub query_text: Option<String>,
    pub duration_ms: Option<i64>,
}

/// Aggregate server-side stats for charting.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerStats {
    pub numeric_connections: i64,
    pub active_connections: i64,
    pub cache_hit_ratio: Option<f64>,
    /// NOTE: despite the name this is the server's *cumulative* transaction
    /// counter, not a rate. A rate only exists relative to a previous
    /// sample, which is what [`MetricSeries::push`] computes. The name is
    /// kept because it is what the wire format and the Tauri shell use.
    pub transactions_per_sec: Option<f64>,
    pub deadlocks: Option<i64>,
}

// ===========================================================================
// Server-side activity: per-driver SQL
// ===========================================================================

fn not_supported(driver: &DbDriver) -> DbError {
    DbError::InvalidInput(format!("Not supported for this driver ({:?})", driver))
}

/// Whether this driver exposes session metadata at all.
pub fn supports_session_introspection(driver: &DbDriver) -> bool {
    driver.is_postgres_compatible() || matches!(driver, DbDriver::MySql)
}

/// SQL listing server-side sessions.
///
/// Both dialects deliberately project the same eight columns in the same
/// order so one row mapper serves both; see [`map_active_row`].
pub fn active_queries_sql(driver: &DbDriver) -> Result<&'static str, DbError> {
    if driver.is_postgres_compatible() {
        // pg_stat_activity hides other users' query text unless the caller is
        // a superuser or has the pg_read_all_stats role; callers may see NULL
        // query_text for sessions they don't own.
        return Ok(r#"
            SELECT pid,
                   usename,
                   datname,
                   client_addr::text,
                   to_char(query_start, 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS query_start,
                   state,
                   query,
                   CASE WHEN query_start IS NOT NULL
                        THEN (EXTRACT(EPOCH FROM (now() - query_start)) * 1000)::bigint
                        ELSE NULL END AS duration_ms
            FROM pg_stat_activity
            WHERE pid <> pg_backend_pid()
            ORDER BY query_start NULLS LAST
        "#);
    }
    match driver {
        DbDriver::MySql => Ok(
            "SELECT ID, USER, DB, HOST, NULL AS query_start, COMMAND, INFO, TIME*1000 AS duration_ms \
             FROM information_schema.PROCESSLIST",
        ),
        other => Err(not_supported(other)),
    }
}

/// SQL cancelling a running statement by session pid.
///
/// `pid` is an `i64`, so interpolating it cannot inject: there is no string
/// to escape. Postgres has no way to parameterise this call anyway.
pub fn kill_query_sql(driver: &DbDriver, pid: i64) -> Result<String, DbError> {
    if driver.is_postgres_compatible() {
        return Ok(format!("SELECT pg_cancel_backend({})", pid));
    }
    match driver {
        DbDriver::MySql => Ok(format!("KILL QUERY {}", pid)),
        other => Err(not_supported(other)),
    }
}

/// SQL for the aggregate server metrics behind the chart.
pub fn server_stats_sql(driver: &DbDriver) -> Result<&'static str, DbError> {
    if driver.is_postgres_compatible() {
        // Aggregated across all databases, so the chart represents
        // server-wide load rather than just the connected database.
        return Ok(r#"
            SELECT
                (SELECT count(*) FROM pg_stat_activity)::bigint AS total_conns,
                (SELECT count(*) FROM pg_stat_activity WHERE state = 'active')::bigint AS active_conns,
                COALESCE(SUM(blks_hit)::float8 / NULLIF(SUM(blks_hit + blks_read), 0), 0) AS cache_hit_ratio,
                COALESCE(SUM(xact_commit + xact_rollback), 0)::bigint AS xact_total,
                COALESCE(SUM(deadlocks), 0)::bigint AS deadlocks
            FROM pg_stat_database
        "#);
    }
    match driver {
        DbDriver::MySql => Ok(
            "SHOW GLOBAL STATUS WHERE Variable_name IN \
             ('Threads_connected','Threads_running','Com_commit','Com_rollback','Innodb_deadlocks')",
        ),
        other => Err(not_supported(other)),
    }
}

// ===========================================================================
// Server-side activity: value coercion and row mapping
// ===========================================================================

fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.parse::<i64>().ok(),
        _ => None,
    }
}

fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn as_string(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => {
            if s.is_empty() {
                None
            } else {
                Some(s.clone())
            }
        }
        other => Some(other.to_string()),
    }
}

/// Truncate to `max` characters, appending an ellipsis.
///
/// Counted in characters, not bytes: a query full of multi-byte text must
/// not be cut mid-character.
pub fn truncate(s: String, max: usize) -> String {
    if s.chars().count() <= max {
        s
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push_str("...");
        out
    }
}

/// Longest query text kept per session. The full statement is not useful in
/// a monitor list and a runaway query can be megabytes long.
pub const MAX_QUERY_TEXT: usize = 500;

/// Map one projected row to an [`ActiveQuery`].
pub fn map_active_row(row: &[Value]) -> ActiveQuery {
    ActiveQuery {
        pid: row.first().and_then(as_i64).unwrap_or(0),
        user: row.get(1).and_then(as_string),
        database: row.get(2).and_then(as_string),
        client_addr: row.get(3).and_then(as_string),
        query_start: row.get(4).and_then(as_string),
        state: row.get(5).and_then(as_string),
        query_text: row
            .get(6)
            .and_then(as_string)
            .map(|q| truncate(q, MAX_QUERY_TEXT)),
        duration_ms: row.get(7).and_then(as_i64),
    }
}

/// Turn a driver result into [`ServerStats`].
///
/// Postgres returns one wide row; MySQL returns name/value pairs, so the
/// two shapes are decoded separately even though the output is the same.
pub fn parse_server_stats(driver: &DbDriver, result: QueryResult) -> Result<ServerStats, DbError> {
    if driver.is_postgres_compatible() {
        let row = result
            .rows
            .into_iter()
            .next()
            .ok_or_else(|| DbError::QueryError("pg_stat_database returned no rows".to_string()))?;
        let xact_total = row.get(3).and_then(as_i64).unwrap_or(0);
        return Ok(ServerStats {
            numeric_connections: row.first().and_then(as_i64).unwrap_or(0),
            active_connections: row.get(1).and_then(as_i64).unwrap_or(0),
            cache_hit_ratio: row.get(2).and_then(as_f64),
            transactions_per_sec: Some(xact_total as f64),
            deadlocks: Some(row.get(4).and_then(as_i64).unwrap_or(0)),
        });
    }

    match driver {
        DbDriver::MySql => {
            let mut threads_connected = 0i64;
            let mut threads_running = 0i64;
            let mut commits = 0i64;
            let mut rollbacks = 0i64;
            let mut deadlocks = 0i64;
            for row in result.rows {
                let name = row.first().and_then(as_string).unwrap_or_default();
                let val = row.get(1).and_then(as_i64).unwrap_or(0);
                match name.as_str() {
                    "Threads_connected" => threads_connected = val,
                    "Threads_running" => threads_running = val,
                    "Com_commit" => commits = val,
                    "Com_rollback" => rollbacks = val,
                    "Innodb_deadlocks" => deadlocks = val,
                    _ => {}
                }
            }
            Ok(ServerStats {
                numeric_connections: threads_connected,
                active_connections: threads_running,
                cache_hit_ratio: None,
                transactions_per_sec: Some((commits + rollbacks) as f64),
                deadlocks: Some(deadlocks),
            })
        }
        other => Err(not_supported(other)),
    }
}

// ===========================================================================
// Server-side activity: operations
// ===========================================================================

/// Pull the driver and its type out of state, cloning the `Arc` so the lock
/// is released before any await.
fn take_connection(
    state: &Mutex<AppState>,
    connection_id: &str,
) -> Result<(Arc<dyn DatabaseDriver>, DbDriver), DbError> {
    let guard = state.lock().unwrap();
    let driver = guard
        .get_profile(connection_id)
        .ok_or_else(|| DbError::ConnectionError("Connection not found".to_string()))?
        .driver
        .clone();
    let conn = guard
        .get_connection(connection_id)
        .ok_or_else(|| DbError::ConnectionError("Connection not found".to_string()))?
        .clone();
    Ok((conn, driver))
}

/// Retrieve the list of active queries / sessions from the database server.
pub async fn get_active_queries(
    state: &Mutex<AppState>,
    connection_id: &str,
) -> Result<Vec<ActiveQuery>, DbError> {
    let (conn, driver) = take_connection(state, connection_id)?;
    let sql = active_queries_sql(&driver)?;
    let res = conn.execute_query(sql).await?;
    Ok(res.rows.iter().map(|r| map_active_row(r)).collect())
}

/// Cancel / kill a running query by session pid.
pub async fn kill_query(
    state: &Mutex<AppState>,
    connection_id: &str,
    pid: i64,
) -> Result<(), DbError> {
    let (conn, driver) = take_connection(state, connection_id)?;
    let sql = kill_query_sql(&driver, pid)?;
    conn.execute_query(&sql).await?;
    Ok(())
}

/// Retrieve aggregate server metrics for charting.
pub async fn get_server_stats(
    state: &Mutex<AppState>,
    connection_id: &str,
) -> Result<ServerStats, DbError> {
    let (conn, driver) = take_connection(state, connection_id)?;
    let sql = server_stats_sql(&driver)?;
    let res = conn.execute_query(sql).await?;
    parse_server_stats(&driver, res)
}

// ===========================================================================
// Metric series
// ===========================================================================

/// One point on the server-metrics chart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetricsSample {
    /// Milliseconds since the Unix epoch, used as the chart's x value.
    pub t_ms: i64,
    pub connections: f64,
    pub active: f64,
    /// Transactions per second, derived by diffing the server's cumulative
    /// counter against the previous sample. Zero for the first sample,
    /// which has nothing to diff against.
    pub tx_per_sec: f64,
}

/// A rolling window of server-metric samples.
///
/// The React chart kept the last 60 samples in component state and did the
/// counter-diffing inline. Both belong here: they are arithmetic, not
/// drawing, and the native chart needs exactly the same numbers.
#[derive(Debug, Clone)]
pub struct MetricSeries {
    samples: Vec<MetricsSample>,
    max_samples: usize,
    /// Previous `(timestamp, cumulative transaction counter)`.
    prev_counter: Option<(i64, f64)>,
}

impl Default for MetricSeries {
    fn default() -> Self {
        Self::new(600)
    }
}

impl MetricSeries {
    pub fn new(max_samples: usize) -> Self {
        Self {
            samples: Vec::new(),
            max_samples: max_samples.max(1),
            prev_counter: None,
        }
    }

    /// Add a sample taken at `t_ms`, deriving the transaction rate from the
    /// difference against the previous sample's cumulative counter.
    ///
    /// A negative delta means the server restarted and reset its counters;
    /// that is reported as zero rather than as a nonsensical negative rate.
    pub fn push(&mut self, t_ms: i64, stats: &ServerStats) {
        let counter = stats.transactions_per_sec.unwrap_or(0.0);
        let rate = match self.prev_counter {
            Some((prev_t, prev_counter)) if t_ms > prev_t => {
                let dt = (t_ms - prev_t) as f64 / 1000.0;
                let delta = counter - prev_counter;
                if delta > 0.0 {
                    delta / dt
                } else {
                    0.0
                }
            }
            _ => 0.0,
        };
        self.prev_counter = Some((t_ms, counter));

        self.samples.push(MetricsSample {
            t_ms,
            connections: stats.numeric_connections as f64,
            active: stats.active_connections as f64,
            tx_per_sec: rate,
        });
        if self.samples.len() > self.max_samples {
            let excess = self.samples.len() - self.max_samples;
            self.samples.drain(0..excess);
        }
    }

    /// Forget every sample and the counter baseline.
    ///
    /// Called when the selected connection changes: rates diffed across two
    /// different servers are meaningless.
    pub fn clear(&mut self) {
        self.samples.clear();
        self.prev_counter = None;
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn all(&self) -> &[MetricsSample] {
        &self.samples
    }

    pub fn latest(&self) -> Option<&MetricsSample> {
        self.samples.last()
    }

    /// The trailing `span_ms` of samples, measured back from the newest one.
    ///
    /// Anchored to the newest sample rather than to "now" on purpose: when
    /// polling is paused the chart should keep showing the last data it
    /// collected instead of emptying itself a minute later.
    pub fn window(&self, span_ms: Option<i64>) -> &[MetricsSample] {
        let Some(span) = span_ms else {
            return &self.samples;
        };
        let Some(last) = self.samples.last() else {
            return &self.samples;
        };
        let cutoff = last.t_ms - span.max(0);
        let start = self
            .samples
            .iter()
            .position(|s| s.t_ms >= cutoff)
            .unwrap_or(self.samples.len());
        &self.samples[start..]
    }
}

/// Reduce a series to at most `max_points`, keeping the first and last.
///
/// A plot cannot show more points than it has pixels, and handing egui_plot
/// thousands of vertices per frame costs real time. Stride sampling is used
/// rather than averaging so a spike stays a spike instead of being smoothed
/// into the baseline, which for a monitoring chart is the whole point.
pub fn downsample(points: &[[f64; 2]], max_points: usize) -> Vec<[f64; 2]> {
    if max_points == 0 {
        return Vec::new();
    }
    if points.len() <= max_points {
        return points.to_vec();
    }
    if max_points == 1 {
        return vec![points[points.len() - 1]];
    }
    let last = points.len() - 1;
    let steps = max_points - 1;
    let mut out = Vec::with_capacity(max_points);
    let mut previous: Option<usize> = None;
    for step in 0..=steps {
        // Rounded proportional index, so the samples stay evenly spread and
        // both endpoints are always included.
        let idx = (step * last + steps / 2) / steps;
        if previous != Some(idx) {
            out.push(points[idx]);
            previous = Some(idx);
        }
    }
    out
}

// ===========================================================================
// Query log: filtering
// ===========================================================================

/// How far back a log or chart view reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeRange {
    #[default]
    All,
    Last15Min,
    LastHour,
    Last24Hours,
    Last7Days,
}

impl TimeRange {
    pub const ALL: [TimeRange; 5] = [
        TimeRange::All,
        TimeRange::Last15Min,
        TimeRange::LastHour,
        TimeRange::Last24Hours,
        TimeRange::Last7Days,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TimeRange::All => "All time",
            TimeRange::Last15Min => "Last 15 minutes",
            TimeRange::LastHour => "Last hour",
            TimeRange::Last24Hours => "Last 24 hours",
            TimeRange::Last7Days => "Last 7 days",
        }
    }

    /// Length of the range, or `None` for unbounded.
    pub fn span(self) -> Option<ChronoDuration> {
        match self {
            TimeRange::All => None,
            TimeRange::Last15Min => Some(ChronoDuration::minutes(15)),
            TimeRange::LastHour => Some(ChronoDuration::hours(1)),
            TimeRange::Last24Hours => Some(ChronoDuration::hours(24)),
            TimeRange::Last7Days => Some(ChronoDuration::days(7)),
        }
    }

    /// The earliest instant included, relative to `now`.
    pub fn start(self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.span().map(|d| now - d)
    }

    /// The same instant as the RFC 3339 string [`QueryLogFilter`] parses.
    pub fn start_rfc3339(self, now: DateTime<Utc>) -> Option<String> {
        self.start(now).map(|t| t.to_rfc3339())
    }
}

/// A query-log selection: the single-valued core filter plus the multi-select
/// dimensions it cannot express.
///
/// [`QueryLogFilter`] carries one `query_type` and one `status`. The React
/// filter panel offered checkboxes for both and then sent only the first
/// ticked value of each, silently ignoring the rest (recorded as a bug in
/// docs/rust-ui-migration/01-frontend-inventory.md §2.12). Rather than
/// reproduce the bug or cripple the UI to one choice, the sets are carried
/// alongside and applied here.
#[derive(Debug, Clone, Default)]
pub struct LogQuery {
    pub filter: QueryLogFilter,
    /// Empty means "any type".
    pub types: Vec<QueryType>,
    /// Empty means "any status".
    pub statuses: Vec<QueryStatus>,
}

impl LogQuery {
    pub fn matches(&self, log: &QueryLog) -> bool {
        if !self.filter.matches(log) {
            return false;
        }
        if !self.types.is_empty() && !self.types.contains(&log.query_type) {
            return false;
        }
        if !self.statuses.is_empty() && !self.statuses.contains(&log.status) {
            return false;
        }
        true
    }
}

/// Sort and page a set of logs.
///
/// Mirrors `ActivityLogger::get_logs`, but total-safe: that method slices
/// `sorted[start..end]`, which panics when the page index is past the end of
/// a shrinking log. Here an out-of-range page clamps to the last one, which
/// is what a user who filtered while on page 9 expects to see anyway.
pub fn sort_and_paginate(
    mut logs: Vec<QueryLog>,
    sort: &QueryLogSort,
    page: usize,
    page_size: usize,
) -> QueryLogResponse {
    let page_size = page_size.max(1);
    let total = logs.len();

    match sort.field {
        QueryLogSortField::StartedAt => logs.sort_by_key(|log| log.started_at),
        QueryLogSortField::DurationMs => logs.sort_by_key(|log| log.duration_ms),
        QueryLogSortField::RowCount => logs.sort_by_key(|log| log.row_count),
        QueryLogSortField::ConnectionName => {
            logs.sort_by(|a, b| a.connection_name.cmp(&b.connection_name))
        }
    }
    if matches!(sort.direction, SortDirection::Desc) {
        logs.reverse();
    }

    let total_pages = total.div_ceil(page_size);
    let page = page.min(total_pages.saturating_sub(1));
    let start = (page * page_size).min(total);
    let end = (start + page_size).min(total);

    QueryLogResponse {
        logs: logs[start..end].to_vec(),
        total,
        page,
        page_size,
        total_pages,
    }
}

/// Roll a set of logs up into the statistics tiles.
///
/// Same arithmetic as `ActivityLogger::get_stats`, over an explicit slice
/// rather than the logger's own filter, so it can be applied to a selection
/// the single-valued [`QueryLogFilter`] cannot express.
pub fn compute_stats(logs: &[QueryLog]) -> ActivityStats {
    use std::collections::HashMap;

    let total_queries = logs.len();
    let failed_queries = logs
        .iter()
        .filter(|log| log.status == QueryStatus::Failed)
        .count();

    let durations: Vec<u64> = logs.iter().filter_map(|log| log.duration_ms).collect();
    let avg_duration = if durations.is_empty() {
        0.0
    } else {
        durations.iter().sum::<u64>() as f64 / durations.len() as f64
    };

    let total_rows = logs.iter().filter_map(|log| log.row_count).sum::<u64>();

    let mut queries_by_type: HashMap<QueryType, usize> = HashMap::new();
    for log in logs {
        *queries_by_type.entry(log.query_type).or_insert(0) += 1;
    }

    let mut queries_by_status: HashMap<String, usize> = HashMap::new();
    for log in logs {
        *queries_by_status
            .entry(status_key(log.status).to_string())
            .or_insert(0) += 1;
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

/// The lowercase key a status is counted under, matching the wire format.
pub fn status_key(status: QueryStatus) -> &'static str {
    match status {
        QueryStatus::Running => "running",
        QueryStatus::Completed => "completed",
        QueryStatus::Failed => "failed",
        QueryStatus::Cancelled => "cancelled",
    }
}

/// The label a query type is shown under, matching the wire format.
pub fn query_type_key(query_type: QueryType) -> &'static str {
    match query_type {
        QueryType::Select => "SELECT",
        QueryType::Insert => "INSERT",
        QueryType::Update => "UPDATE",
        QueryType::Delete => "DELETE",
        QueryType::Create => "CREATE",
        QueryType::Alter => "ALTER",
        QueryType::Drop => "DROP",
        QueryType::Transaction => "TRANSACTION",
        QueryType::Other => "OTHER",
    }
}

/// Every query type, in the order the filter panel lists them.
pub const QUERY_TYPES: [QueryType; 9] = [
    QueryType::Select,
    QueryType::Insert,
    QueryType::Update,
    QueryType::Delete,
    QueryType::Create,
    QueryType::Alter,
    QueryType::Drop,
    QueryType::Transaction,
    QueryType::Other,
];

/// Every status, in the order the filter panel lists them.
pub const QUERY_STATUSES: [QueryStatus; 4] = [
    QueryStatus::Running,
    QueryStatus::Completed,
    QueryStatus::Failed,
    QueryStatus::Cancelled,
];

// ===========================================================================
// Query log: operations
// ===========================================================================

/// Every log matching `query`, newest-first ordering not yet applied.
///
/// The lock is taken only for the clone out of the logger and released
/// before the set predicates run, so a large log does not hold up other
/// work while it is being filtered.
pub fn select_logs(state: &Mutex<AppState>, query: &LogQuery) -> Vec<QueryLog> {
    let mut logs = {
        let guard = state.lock().unwrap();
        guard
            .activity_logger
            .get_all_logs(Some(query.filter.clone()))
    };
    if !query.types.is_empty() || !query.statuses.is_empty() {
        logs.retain(|log| {
            (query.types.is_empty() || query.types.contains(&log.query_type))
                && (query.statuses.is_empty() || query.statuses.contains(&log.status))
        });
    }
    logs
}

/// Filtered, sorted, paginated query logs.
pub fn get_query_logs(
    state: &Mutex<AppState>,
    query: &LogQuery,
    sort: &QueryLogSort,
    page: usize,
    page_size: usize,
) -> QueryLogResponse {
    sort_and_paginate(select_logs(state, query), sort, page, page_size)
}

/// Statistics over the same selection [`get_query_logs`] pages.
pub fn get_activity_stats(state: &Mutex<AppState>, query: &LogQuery) -> ActivityStats {
    compute_stats(&select_logs(state, query))
}

/// Total number of logs held, ignoring any filter.
pub fn get_query_logs_count(state: &Mutex<AppState>) -> usize {
    state.lock().unwrap().activity_logger.count()
}

/// Delete every log. Returns how many were removed.
pub fn clear_query_logs(state: &Mutex<AppState>) -> usize {
    state.lock().unwrap().activity_logger.clear_all_logs()
}

/// Delete logs older than the retention period. Returns how many were removed.
pub fn clear_old_query_logs(state: &Mutex<AppState>) -> usize {
    state.lock().unwrap().activity_logger.clear_old_logs()
}

/// Replace a log's tags. Returns false if no log has that id.
pub fn update_query_log_tags(state: &Mutex<AppState>, log_id: &str, tags: Vec<String>) -> bool {
    state
        .lock()
        .unwrap()
        .activity_logger
        .update_tags(log_id, tags)
}

/// Connection profiles as `(id, name)`, for the connection filter.
pub fn connection_choices(state: &Mutex<AppState>) -> Vec<(String, String)> {
    let guard = state.lock().unwrap();
    let mut out: Vec<(String, String)> = guard
        .list_profiles()
        .into_iter()
        .map(|p| (p.id.clone(), p.name.clone()))
        .collect();
    out.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    out
}

/// Database names that actually appear in the log, for the database filter.
///
/// The React panel asked the server for its database list, which needs a
/// live connection and offers databases no logged query ever touched.
/// Deriving the list from the log needs neither, and every entry it offers
/// selects at least one row.
pub fn logged_databases(state: &Mutex<AppState>, connection_id: Option<&str>) -> Vec<String> {
    let logs = {
        let guard = state.lock().unwrap();
        guard.activity_logger.get_all_logs(None)
    };
    let mut names: Vec<String> = logs
        .iter()
        .filter(|log| connection_id.is_none_or(|id| log.connection_id == id))
        .filter_map(|log| log.database.clone())
        .filter(|db| !db.is_empty())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Serialise logs in the requested export format.
///
/// Pure, so the exact bytes written to disk are testable without touching
/// the filesystem.
pub fn render_logs(logs: &[QueryLog], format: ExportFormat) -> Result<String, DbError> {
    match format {
        ExportFormat::Json => serde_json::to_string_pretty(logs)
            .map_err(|e| DbError::InternalError(format!("Failed to serialize logs to JSON: {}", e))),
        ExportFormat::Csv => {
            let mut csv = String::new();
            csv.push_str(
                "ID,Connection ID,Connection Name,Database,SQL,Query Type,Status,Started At,\
                 Completed At,Duration (ms),Row Count,Error\n",
            );
            for log in logs {
                csv.push_str(&format!(
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
                ));
            }
            Ok(csv)
        }
        ExportFormat::Txt => {
            let mut txt = String::from("=== Query Logs Export ===\n\n");
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
            Ok(txt)
        }
    }
}

/// What an export produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportOutcome {
    pub path: String,
    pub logs: usize,
    pub bytes: u64,
}

/// Write the selected logs to `file_path`.
///
/// The content is built while the lock is held and the disk write happens
/// afterwards on a blocking thread: a `std::sync::MutexGuard` cannot be held
/// across an await, and a slow disk must not occupy a Tokio worker.
pub async fn export_query_logs(
    state: &Mutex<AppState>,
    query: &LogQuery,
    format: ExportFormat,
    file_path: String,
) -> Result<ExportOutcome, DbError> {
    let logs = select_logs(state, query);
    let count = logs.len();
    let content = render_logs(&logs, format)?;
    let bytes = content.len() as u64;

    let write_path = file_path.clone();
    tokio::task::spawn_blocking(move || std::fs::write(&write_path, content))
        .await
        .map_err(|e| DbError::InternalError(format!("Export task failed to join: {}", e)))?
        .map_err(|e| DbError::InternalError(format!("Failed to write {}: {}", file_path, e)))?;

    Ok(ExportOutcome {
        path: file_path,
        logs: count,
        bytes,
    })
}

// ===========================================================================
// Query log: recording
// ===========================================================================

/// Record that a statement has started running.
///
/// The Tauri shell calls the equivalent inline in its `execute_query`
/// command. Exposed here so the native shell's workspace can record into the
/// same log rather than leaving the Activity Monitor permanently empty.
pub fn record_query_start(state: &Mutex<AppState>, log: QueryLog) {
    state.lock().unwrap().activity_logger.log_query_start(log);
}

/// Record a successful completion against a previously started log.
pub fn record_query_complete(
    state: &Mutex<AppState>,
    log_id: &str,
    duration_ms: u64,
    row_count: Option<u64>,
) -> bool {
    state
        .lock()
        .unwrap()
        .activity_logger
        .log_query_complete(log_id, duration_ms, row_count)
}

/// Record a failure against a previously started log.
pub fn record_query_error(
    state: &Mutex<AppState>,
    log_id: &str,
    duration_ms: u64,
    error: String,
) -> bool {
    state
        .lock()
        .unwrap()
        .activity_logger
        .log_query_error(log_id, duration_ms, error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- per-driver SQL ---------------------------------------------------

    #[test]
    fn active_queries_sql_is_driver_specific() {
        let pg = active_queries_sql(&DbDriver::Postgres).unwrap();
        assert!(pg.contains("pg_stat_activity"));
        // The backend's own session must not appear in its own monitor.
        assert!(pg.contains("pid <> pg_backend_pid()"));

        let my = active_queries_sql(&DbDriver::MySql).unwrap();
        assert!(my.contains("information_schema.PROCESSLIST"));
    }

    #[test]
    fn postgres_flavours_share_the_postgres_sql() {
        let pg = active_queries_sql(&DbDriver::Postgres).unwrap();
        assert_eq!(active_queries_sql(&DbDriver::Supabase).unwrap(), pg);
        assert_eq!(active_queries_sql(&DbDriver::Neon).unwrap(), pg);
        assert_eq!(
            server_stats_sql(&DbDriver::Supabase).unwrap(),
            server_stats_sql(&DbDriver::Postgres).unwrap()
        );
    }

    #[test]
    fn drivers_without_session_views_report_not_supported() {
        for driver in [
            DbDriver::Sqlite,
            DbDriver::MongoDb,
            DbDriver::SqlServer,
            DbDriver::Turso,
            DbDriver::Redis,
        ] {
            assert!(!supports_session_introspection(&driver));
            assert!(matches!(
                active_queries_sql(&driver),
                Err(DbError::InvalidInput(_))
            ));
            assert!(matches!(
                server_stats_sql(&driver),
                Err(DbError::InvalidInput(_))
            ));
            assert!(matches!(
                kill_query_sql(&driver, 1),
                Err(DbError::InvalidInput(_))
            ));
        }
        assert!(supports_session_introspection(&DbDriver::Postgres));
        assert!(supports_session_introspection(&DbDriver::MySql));
    }

    #[test]
    fn kill_sql_uses_the_dialects_own_verb() {
        assert_eq!(
            kill_query_sql(&DbDriver::Postgres, 4242).unwrap(),
            "SELECT pg_cancel_backend(4242)"
        );
        assert_eq!(
            kill_query_sql(&DbDriver::MySql, 4242).unwrap(),
            "KILL QUERY 4242"
        );
        // A negative pid is still just an integer: nothing can escape out.
        assert_eq!(
            kill_query_sql(&DbDriver::MySql, -1).unwrap(),
            "KILL QUERY -1"
        );
    }

    #[test]
    fn server_stats_sql_is_driver_specific() {
        assert!(server_stats_sql(&DbDriver::Postgres)
            .unwrap()
            .contains("pg_stat_database"));
        assert!(server_stats_sql(&DbDriver::MySql)
            .unwrap()
            .contains("SHOW GLOBAL STATUS"));
    }

    // -- row mapping ------------------------------------------------------

    #[test]
    fn active_row_maps_both_dialects_identically() {
        let row = vec![
            json!(31),
            json!("app"),
            json!("shop"),
            json!("10.0.0.4"),
            json!("2026-09-08T10:00:00.000Z"),
            json!("active"),
            json!("SELECT 1"),
            json!(1500),
        ];
        let got = map_active_row(&row);
        assert_eq!(got.pid, 31);
        assert_eq!(got.user.as_deref(), Some("app"));
        assert_eq!(got.database.as_deref(), Some("shop"));
        assert_eq!(got.client_addr.as_deref(), Some("10.0.0.4"));
        assert_eq!(got.state.as_deref(), Some("active"));
        assert_eq!(got.query_text.as_deref(), Some("SELECT 1"));
        assert_eq!(got.duration_ms, Some(1500));
    }

    #[test]
    fn active_row_tolerates_nulls_and_string_numbers() {
        // MySQL returns TIME*1000 as a string through some connectors, and
        // pg_stat_activity NULLs the query text of other users' sessions.
        let row = vec![
            json!("77"),
            Value::Null,
            json!(""),
            Value::Null,
            Value::Null,
            json!("Sleep"),
            Value::Null,
            json!("2000"),
        ];
        let got = map_active_row(&row);
        assert_eq!(got.pid, 77);
        assert_eq!(got.user, None);
        // An empty string is absence, not a database named "".
        assert_eq!(got.database, None);
        assert_eq!(got.query_text, None);
        assert_eq!(got.duration_ms, Some(2000));
    }

    #[test]
    fn active_row_truncates_runaway_query_text() {
        let long = "x".repeat(MAX_QUERY_TEXT + 50);
        let row = vec![
            json!(1),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            json!(long),
            Value::Null,
        ];
        let text = map_active_row(&row).query_text.unwrap();
        assert_eq!(text.chars().count(), MAX_QUERY_TEXT + 3);
        assert!(text.ends_with("..."));
    }

    #[test]
    fn truncate_counts_characters_not_bytes() {
        // Cutting on bytes here would split a multi-byte character.
        assert_eq!(truncate("ααααα".to_string(), 3), "ααα...");
        assert_eq!(truncate("abc".to_string(), 3), "abc");
    }

    #[test]
    fn parses_postgres_stats_row() {
        let result = QueryResult::with_data(
            vec![
                "total_conns".into(),
                "active_conns".into(),
                "cache_hit_ratio".into(),
                "xact_total".into(),
                "deadlocks".into(),
            ],
            vec![vec![json!(12), json!(3), json!(0.987), json!(50_000), json!(2)]],
        );
        let stats = parse_server_stats(&DbDriver::Postgres, result).unwrap();
        assert_eq!(stats.numeric_connections, 12);
        assert_eq!(stats.active_connections, 3);
        assert_eq!(stats.cache_hit_ratio, Some(0.987));
        assert_eq!(stats.transactions_per_sec, Some(50_000.0));
        assert_eq!(stats.deadlocks, Some(2));
    }

    #[test]
    fn postgres_stats_with_no_rows_is_an_error_not_a_zero() {
        let empty = QueryResult::with_data(vec!["total_conns".into()], vec![]);
        assert!(matches!(
            parse_server_stats(&DbDriver::Postgres, empty),
            Err(DbError::QueryError(_))
        ));
    }

    #[test]
    fn parses_mysql_name_value_status_rows() {
        let result = QueryResult::with_data(
            vec!["Variable_name".into(), "Value".into()],
            vec![
                vec![json!("Threads_connected"), json!("9")],
                vec![json!("Threads_running"), json!("2")],
                vec![json!("Com_commit"), json!("100")],
                vec![json!("Com_rollback"), json!("5")],
                vec![json!("Uptime"), json!("999")],
            ],
        );
        let stats = parse_server_stats(&DbDriver::MySql, result).unwrap();
        assert_eq!(stats.numeric_connections, 9);
        assert_eq!(stats.active_connections, 2);
        // MySQL exposes no cache-hit equivalent here.
        assert_eq!(stats.cache_hit_ratio, None);
        assert_eq!(stats.transactions_per_sec, Some(105.0));
    }

    // -- metric series ----------------------------------------------------

    fn stats_with(conns: i64, active: i64, counter: f64) -> ServerStats {
        ServerStats {
            numeric_connections: conns,
            active_connections: active,
            cache_hit_ratio: None,
            transactions_per_sec: Some(counter),
            deadlocks: None,
        }
    }

    #[test]
    fn first_sample_has_no_rate_to_diff_against() {
        let mut s = MetricSeries::new(10);
        s.push(1_000, &stats_with(5, 1, 100.0));
        assert_eq!(s.len(), 1);
        assert_eq!(s.latest().unwrap().tx_per_sec, 0.0);
        assert_eq!(s.latest().unwrap().connections, 5.0);
    }

    #[test]
    fn rate_is_the_counter_delta_over_elapsed_seconds() {
        let mut s = MetricSeries::new(10);
        s.push(0, &stats_with(5, 1, 100.0));
        s.push(5_000, &stats_with(5, 1, 150.0));
        // 50 transactions over 5 seconds.
        assert_eq!(s.latest().unwrap().tx_per_sec, 10.0);
    }

    #[test]
    fn counter_reset_reports_zero_rather_than_a_negative_rate() {
        let mut s = MetricSeries::new(10);
        s.push(0, &stats_with(5, 1, 5_000.0));
        // The server restarted, so the cumulative counter went backwards.
        s.push(1_000, &stats_with(5, 1, 3.0));
        assert_eq!(s.latest().unwrap().tx_per_sec, 0.0);
    }

    #[test]
    fn window_keeps_only_the_most_recent_samples() {
        let mut s = MetricSeries::new(3);
        for i in 0..6 {
            s.push(i * 1_000, &stats_with(i, 0, 0.0));
        }
        assert_eq!(s.len(), 3);
        let kept: Vec<f64> = s.all().iter().map(|x| x.connections).collect();
        assert_eq!(kept, vec![3.0, 4.0, 5.0]);
    }

    #[test]
    fn clearing_drops_the_counter_baseline_too() {
        let mut s = MetricSeries::new(10);
        s.push(0, &stats_with(5, 1, 100.0));
        s.clear();
        assert!(s.is_empty());
        // Without the baseline reset, the first sample on a different server
        // would diff against the old server's counter.
        s.push(1_000, &stats_with(5, 1, 1_000_000.0));
        assert_eq!(s.latest().unwrap().tx_per_sec, 0.0);
    }

    #[test]
    fn time_window_is_measured_back_from_the_newest_sample() {
        let mut s = MetricSeries::new(100);
        for i in 0..10 {
            s.push(i * 1_000, &stats_with(i, 0, 0.0));
        }
        // Newest is t=9000, so a 3s window starts at t=6000.
        let w = s.window(Some(3_000));
        assert_eq!(w.len(), 4);
        assert_eq!(w[0].t_ms, 6_000);
        assert_eq!(w[3].t_ms, 9_000);

        assert_eq!(s.window(None).len(), 10);
        // A window longer than the data keeps everything.
        assert_eq!(s.window(Some(1_000_000)).len(), 10);
    }

    #[test]
    fn window_of_an_empty_series_is_empty() {
        let s = MetricSeries::new(10);
        assert!(s.window(Some(1_000)).is_empty());
        assert!(s.window(None).is_empty());
    }

    // -- downsampling -----------------------------------------------------

    fn ramp(n: usize) -> Vec<[f64; 2]> {
        (0..n).map(|i| [i as f64, i as f64]).collect()
    }

    #[test]
    fn downsample_leaves_short_series_untouched() {
        let p = ramp(5);
        assert_eq!(downsample(&p, 10), p);
        assert_eq!(downsample(&p, 5), p);
    }

    #[test]
    fn downsample_keeps_both_endpoints() {
        let p = ramp(1_000);
        let out = downsample(&p, 100);
        assert!(out.len() <= 100);
        assert_eq!(out.first().unwrap()[0], 0.0);
        assert_eq!(out.last().unwrap()[0], 999.0);
    }

    #[test]
    fn downsample_stays_monotonic_in_x() {
        let out = downsample(&ramp(517), 61);
        assert!(out.windows(2).all(|w| w[0][0] < w[1][0]));
    }

    #[test]
    fn downsample_degenerate_limits() {
        assert!(downsample(&ramp(10), 0).is_empty());
        assert_eq!(downsample(&ramp(10), 1), vec![[9.0, 9.0]]);
        assert!(downsample(&[], 10).is_empty());
    }

    // -- time ranges ------------------------------------------------------

    #[test]
    fn time_range_start_is_now_minus_the_span() {
        let now = DateTime::parse_from_rfc3339("2026-09-08T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(TimeRange::All.start(now), None);
        assert_eq!(
            TimeRange::Last15Min.start(now).unwrap().to_rfc3339(),
            "2026-09-08T11:45:00+00:00"
        );
        assert_eq!(
            TimeRange::Last24Hours.start(now).unwrap().to_rfc3339(),
            "2026-09-07T12:00:00+00:00"
        );
        // The string form must be what QueryLogFilter can parse back.
        let s = TimeRange::LastHour.start_rfc3339(now).unwrap();
        assert!(DateTime::parse_from_rfc3339(&s).is_ok());
    }

    #[test]
    fn time_range_start_feeds_the_filter() {
        let now = Utc::now();
        let old = make_log("old", "conn-1", "SELECT 1");
        let mut recent = make_log("new", "conn-1", "SELECT 2");
        recent.started_at = now;
        let mut old = old;
        old.started_at = now - ChronoDuration::hours(3);

        let q = LogQuery {
            filter: QueryLogFilter {
                start_date: TimeRange::LastHour.start_rfc3339(now),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(q.matches(&recent));
        assert!(!q.matches(&old));
    }

    // -- log filtering ----------------------------------------------------

    fn make_log(id: &str, connection_id: &str, sql: &str) -> QueryLog {
        QueryLog::new(
            id.to_string(),
            connection_id.to_string(),
            format!("Profile {}", connection_id),
            Some("shop".to_string()),
            sql.to_string(),
        )
    }

    #[test]
    fn empty_log_query_matches_everything() {
        let q = LogQuery::default();
        assert!(q.matches(&make_log("1", "conn-1", "SELECT 1")));
        assert!(q.matches(&make_log("2", "conn-2", "DROP TABLE t")));
    }

    #[test]
    fn log_query_defers_single_valued_fields_to_the_core_filter() {
        let q = LogQuery {
            filter: QueryLogFilter {
                connection_id: Some("conn-1".to_string()),
                search_text: Some("users".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(q.matches(&make_log("1", "conn-1", "SELECT * FROM users")));
        assert!(!q.matches(&make_log("2", "conn-2", "SELECT * FROM users")));
        assert!(!q.matches(&make_log("3", "conn-1", "SELECT * FROM orders")));
    }

    #[test]
    fn multiple_query_types_all_match() {
        // The React panel sent only the first ticked type; every ticked type
        // must count here.
        let q = LogQuery {
            types: vec![QueryType::Insert, QueryType::Delete],
            ..Default::default()
        };
        assert!(q.matches(&make_log("1", "c", "INSERT INTO t VALUES (1)")));
        assert!(q.matches(&make_log("2", "c", "DELETE FROM t")));
        assert!(!q.matches(&make_log("3", "c", "SELECT * FROM t")));
    }

    #[test]
    fn multiple_statuses_all_match() {
        let mut failed = make_log("1", "c", "SELECT 1");
        failed.fail(10, "boom".to_string());
        let mut done = make_log("2", "c", "SELECT 1");
        done.complete(10, Some(1));
        let running = make_log("3", "c", "SELECT 1");

        let q = LogQuery {
            statuses: vec![QueryStatus::Failed, QueryStatus::Cancelled],
            ..Default::default()
        };
        assert!(q.matches(&failed));
        assert!(!q.matches(&done));
        assert!(!q.matches(&running));
    }

    #[test]
    fn type_and_status_sets_combine_as_and() {
        let mut insert_done = make_log("1", "c", "INSERT INTO t VALUES (1)");
        insert_done.complete(5, Some(1));
        let insert_running = make_log("2", "c", "INSERT INTO t VALUES (2)");

        let q = LogQuery {
            types: vec![QueryType::Insert],
            statuses: vec![QueryStatus::Completed],
            ..Default::default()
        };
        assert!(q.matches(&insert_done));
        assert!(!q.matches(&insert_running));
    }

    // -- sorting and paging -----------------------------------------------

    fn paged_ids(response: &QueryLogResponse) -> Vec<String> {
        response.logs.iter().map(|l| l.id.clone()).collect()
    }

    fn timed(id: &str, duration_ms: u64) -> QueryLog {
        let mut log = make_log(id, "c", "SELECT 1");
        log.complete(duration_ms, Some(duration_ms));
        log
    }

    #[test]
    fn sorts_by_duration_in_both_directions() {
        let logs = vec![timed("a", 30), timed("b", 10), timed("c", 20)];
        let asc = sort_and_paginate(
            logs.clone(),
            &QueryLogSort {
                field: QueryLogSortField::DurationMs,
                direction: SortDirection::Asc,
            },
            0,
            10,
        );
        assert_eq!(paged_ids(&asc), vec!["b", "c", "a"]);

        let desc = sort_and_paginate(
            logs,
            &QueryLogSort {
                field: QueryLogSortField::DurationMs,
                direction: SortDirection::Desc,
            },
            0,
            10,
        );
        assert_eq!(paged_ids(&desc), vec!["a", "c", "b"]);
    }

    #[test]
    fn paginates_with_a_short_last_page() {
        let logs: Vec<QueryLog> = (0..25).map(|i| timed(&format!("{:02}", i), i)).collect();
        let sort = QueryLogSort {
            field: QueryLogSortField::DurationMs,
            direction: SortDirection::Asc,
        };

        let first = sort_and_paginate(logs.clone(), &sort, 0, 10);
        assert_eq!(first.logs.len(), 10);
        assert_eq!(first.total, 25);
        assert_eq!(first.total_pages, 3);
        assert_eq!(first.page, 0);

        let last = sort_and_paginate(logs, &sort, 2, 10);
        assert_eq!(last.logs.len(), 5);
        assert_eq!(last.page, 2);
    }

    #[test]
    fn a_page_past_the_end_clamps_instead_of_panicking() {
        // ActivityLogger::get_logs slices unconditionally and panics here.
        // Filtering while on a late page must not take the window down.
        let logs = vec![timed("a", 1), timed("b", 2)];
        let sort = QueryLogSort::default();
        let response = sort_and_paginate(logs, &sort, 99, 10);
        assert_eq!(response.page, 0);
        assert_eq!(response.logs.len(), 2);
        assert_eq!(response.total_pages, 1);
    }

    #[test]
    fn empty_selection_pages_cleanly() {
        let response = sort_and_paginate(Vec::new(), &QueryLogSort::default(), 3, 50);
        assert_eq!(response.total, 0);
        assert_eq!(response.total_pages, 0);
        assert_eq!(response.page, 0);
        assert!(response.logs.is_empty());
    }

    #[test]
    fn zero_page_size_does_not_divide_by_zero() {
        let response = sort_and_paginate(vec![timed("a", 1)], &QueryLogSort::default(), 0, 0);
        assert_eq!(response.page_size, 1);
        assert_eq!(response.total_pages, 1);
    }

    // -- statistics -------------------------------------------------------

    #[test]
    fn stats_roll_up_totals_failures_and_averages() {
        let mut ok = make_log("1", "c", "SELECT 1");
        ok.complete(100, Some(10));
        let mut bad = make_log("2", "c", "INSERT INTO t VALUES (1)");
        bad.fail(50, "boom".to_string());
        let running = make_log("3", "c", "SELECT 2");

        let stats = compute_stats(&[ok, bad, running]);
        assert_eq!(stats.total_queries, 3);
        assert_eq!(stats.failed_queries, 1);
        // The still-running query has no duration and is excluded from the mean.
        assert_eq!(stats.avg_duration, 75.0);
        assert_eq!(stats.total_rows, 10);
        assert_eq!(stats.queries_by_type.get(&QueryType::Select), Some(&2));
        assert_eq!(stats.queries_by_type.get(&QueryType::Insert), Some(&1));
        assert_eq!(stats.queries_by_status.get("failed"), Some(&1));
        assert_eq!(stats.queries_by_status.get("running"), Some(&1));
    }

    #[test]
    fn stats_of_nothing_are_zero_not_nan() {
        let stats = compute_stats(&[]);
        assert_eq!(stats.total_queries, 0);
        assert_eq!(stats.avg_duration, 0.0);
        assert_eq!(stats.total_rows, 0);
        assert!(stats.queries_by_type.is_empty());
    }

    // -- export -----------------------------------------------------------

    #[test]
    fn csv_export_has_a_header_and_one_line_per_log() {
        let mut log = make_log("log-1", "conn-1", "SELECT *\nFROM users, orders");
        log.complete(120, Some(7));
        let csv = render_logs(&[log], ExportFormat::Csv).unwrap();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("ID,Connection ID,"));
        // Commas and newlines inside SQL would otherwise break the columns.
        assert!(lines[1].contains("SELECT * FROM users; orders"));
        assert!(lines[1].contains(",120,7,"));
    }

    #[test]
    fn json_export_round_trips() {
        let mut log = make_log("log-1", "conn-1", "SELECT 1");
        log.complete(5, Some(1));
        let json = render_logs(&[log], ExportFormat::Json).unwrap();
        let back: Vec<QueryLog> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].id, "log-1");
        assert_eq!(back[0].duration_ms, Some(5));
    }

    #[test]
    fn text_export_names_every_field_it_has() {
        let mut log = make_log("log-1", "conn-1", "SELECT 1");
        log.fail(9, "syntax error".to_string());
        let txt = render_logs(&[log], ExportFormat::Txt).unwrap();
        assert!(txt.starts_with("=== Query Logs Export ==="));
        assert!(txt.contains("Error: syntax error"));
        assert!(txt.contains("Duration: 9ms"));
    }

    #[test]
    fn exporting_nothing_still_produces_a_valid_document() {
        assert_eq!(render_logs(&[], ExportFormat::Json).unwrap(), "[]");
        assert_eq!(render_logs(&[], ExportFormat::Csv).unwrap().lines().count(), 1);
    }

    // -- state-backed operations ------------------------------------------

    fn state_with_logs(logs: Vec<QueryLog>) -> Mutex<AppState> {
        let state = AppState::default();
        for log in logs {
            state.activity_logger.log_query_start(log);
        }
        Mutex::new(state)
    }

    #[test]
    fn select_logs_applies_both_the_filter_and_the_sets() {
        let mut insert = make_log("1", "conn-1", "INSERT INTO t VALUES (1)");
        insert.complete(10, Some(1));
        let select = make_log("2", "conn-1", "SELECT * FROM t");
        let other_conn = make_log("3", "conn-2", "INSERT INTO t VALUES (2)");
        let state = state_with_logs(vec![insert, select, other_conn]);

        let query = LogQuery {
            filter: QueryLogFilter {
                connection_id: Some("conn-1".to_string()),
                ..Default::default()
            },
            types: vec![QueryType::Insert],
            statuses: Vec::new(),
        };
        let ids: Vec<String> = select_logs(&state, &query)
            .into_iter()
            .map(|l| l.id)
            .collect();
        assert_eq!(ids, vec!["1"]);
    }

    #[test]
    fn logged_databases_are_distinct_and_connection_scoped() {
        let mut a = make_log("1", "conn-1", "SELECT 1");
        a.database = Some("shop".to_string());
        let mut b = make_log("2", "conn-1", "SELECT 1");
        b.database = Some("analytics".to_string());
        let mut c = make_log("3", "conn-1", "SELECT 1");
        c.database = Some("shop".to_string());
        let mut d = make_log("4", "conn-2", "SELECT 1");
        d.database = Some("other".to_string());
        let state = state_with_logs(vec![a, b, c, d]);

        assert_eq!(
            logged_databases(&state, Some("conn-1")),
            vec!["analytics".to_string(), "shop".to_string()]
        );
        assert_eq!(logged_databases(&state, None).len(), 3);
    }

    #[test]
    fn clearing_removes_every_log() {
        let state = state_with_logs(vec![
            make_log("1", "c", "SELECT 1"),
            make_log("2", "c", "SELECT 2"),
        ]);
        assert_eq!(get_query_logs_count(&state), 2);
        assert_eq!(clear_query_logs(&state), 2);
        assert_eq!(get_query_logs_count(&state), 0);
    }

    #[test]
    fn tags_can_be_replaced_on_an_existing_log() {
        let state = state_with_logs(vec![make_log("1", "c", "SELECT 1")]);
        assert!(update_query_log_tags(
            &state,
            "1",
            vec!["slow".to_string(), "prod".to_string()]
        ));
        assert!(!update_query_log_tags(&state, "missing", vec![]));

        let logs = select_logs(&state, &LogQuery::default());
        assert_eq!(
            logs[0].tags,
            Some(vec!["slow".to_string(), "prod".to_string()])
        );
    }

    #[test]
    fn recording_a_query_moves_it_from_running_to_completed() {
        let state = Mutex::new(AppState::default());
        record_query_start(&state, make_log("1", "c", "SELECT 1"));
        assert_eq!(get_query_logs_count(&state), 1);
        assert!(record_query_complete(&state, "1", 42, Some(3)));

        let stats = get_activity_stats(&state, &LogQuery::default());
        assert_eq!(stats.total_queries, 1);
        assert_eq!(stats.failed_queries, 0);
        assert_eq!(stats.avg_duration, 42.0);
        assert_eq!(stats.total_rows, 3);

        assert!(record_query_error(&state, "1", 1, "late".to_string()));
        assert_eq!(
            get_activity_stats(&state, &LogQuery::default()).failed_queries,
            1
        );
    }

    #[test]
    fn get_query_logs_pages_the_filtered_selection() {
        let logs: Vec<QueryLog> = (0..7)
            .map(|i| {
                let mut log = make_log(&format!("{}", i), "conn-1", "SELECT 1");
                log.complete(i, Some(i));
                log
            })
            .collect();
        let state = state_with_logs(logs);

        let sort = QueryLogSort {
            field: QueryLogSortField::DurationMs,
            direction: SortDirection::Asc,
        };
        let page = get_query_logs(&state, &LogQuery::default(), &sort, 1, 3);
        assert_eq!(paged_ids(&page), vec!["3", "4", "5"]);
        assert_eq!(page.total, 7);
        assert_eq!(page.total_pages, 3);
    }
}
