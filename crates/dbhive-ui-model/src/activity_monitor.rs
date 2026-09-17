//! Activity Monitor view logic, lifted from the egui `activity_monitor`
//! screen. See lib.rs.
//!
//! Formatting, the grid projections of query logs and server sessions, the
//! filter panel's raw inputs and the query they build, the sort mapping, the
//! poll-deadline arithmetic and the chart's series projection. The metric
//! series itself, downsampling, log querying and export live in
//! `dbhive_core::service_monitoring`; this is only the view side of it.

use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use dbhive_core::drivers::QueryResult;
use dbhive_core::models::{
    ExportFormat, QueryLog, QueryLogFilter, QueryLogSort, QueryLogSortField, QueryStatus,
    QueryType, SortDirection,
};
use dbhive_core::service_monitoring as mon;

// ===========================================================================
// Options
// ===========================================================================

/// Auto-refresh choices, in seconds. `0` is "off", matching the web shell's
/// Select which kept the interval visible while paused.
pub const REFRESH_SECONDS: [u64; 5] = [0, 5, 10, 30, 60];

/// Page sizes offered by the log pager. 50 is what the React page hard-coded.
pub const PAGE_SIZES: [usize; 4] = [25, 50, 100, 250];

/// How much of the metric series the chart shows, in seconds. `None` is all.
pub const CHART_SPANS: [(Option<i64>, &str); 4] = [
    (Some(60), "1m"),
    (Some(300), "5m"),
    (Some(900), "15m"),
    (None, "All"),
];

/// Most points handed to a plot in one frame. A chart is at most a few
/// hundred pixels wide, so more vertices than this cost time and show nothing.
pub const MAX_PLOT_POINTS: usize = 240;

/// Longest SQL shown in a grid cell. The full statement is in the detail pane.
pub const SQL_CELL_CHARS: usize = 160;

// ===========================================================================
// Formatting
// ===========================================================================

/// Human-readable duration, matching the web shell's `formatDuration`.
pub fn format_duration(ms: Option<u64>) -> String {
    match ms {
        None => "-".to_string(),
        Some(ms) if ms < 1_000 => format!("{}ms", ms),
        Some(ms) => format!("{:.2}s", ms as f64 / 1_000.0),
    }
}

/// Same, for the signed durations the server's session views report.
///
/// A negative value means the server's clock moved between `query_start` and
/// `now()`; showing it as "-" is more honest than a negative age.
pub fn format_duration_i64(ms: Option<i64>) -> String {
    match ms {
        Some(ms) if ms >= 0 => format_duration(Some(ms as u64)),
        _ => "-".to_string(),
    }
}

/// Compact count, matching the web shell's `formatNumber`.
pub fn format_count(n: u64) -> String {
    const B: f64 = 1_000_000_000.0;
    const M: f64 = 1_000_000.0;
    const K: f64 = 1_000.0;
    let v = n as f64;
    if v >= B {
        format!("{:.1}B", v / B)
    } else if v >= M {
        format!("{:.1}M", v / M)
    } else if v >= K {
        format!("{:.1}K", v / K)
    } else {
        n.to_string()
    }
}

/// Binary-prefixed byte size, used to report how large an export came out.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

/// A UTC instant as local wall-clock time, the way a log reader thinks.
pub fn format_timestamp(t: DateTime<Utc>) -> String {
    t.with_timezone(&Local)
        .format("%b %d, %H:%M:%S")
        .to_string()
}

/// Milliseconds since the epoch as a local `HH:MM:SS` axis label.
///
/// egui_plot hands the formatter whatever the axis range works out to, which
/// on an empty or degenerate plot can be NaN or infinite. `as i64` saturates
/// those to a real timestamp, so they are rejected before the cast rather
/// than labelled "01:00:00".
pub fn format_clock_ms(t_ms: f64) -> String {
    if !t_ms.is_finite() {
        return String::new();
    }
    match DateTime::from_timestamp_millis(t_ms as i64) {
        Some(dt) => dt.with_timezone(&Local).format("%H:%M:%S").to_string(),
        None => String::new(),
    }
}

/// `count` as a percentage of `total`, rounded. Zero total is 0%, not NaN.
pub fn percentage(count: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        (count as f64 / total as f64) * 100.0
    }
}

/// Shorten SQL to one line for a grid cell.
pub fn one_line_sql(sql: &str, max: usize) -> String {
    let flat = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        let mut out: String = flat.chars().take(max).collect();
        out.push('…');
        out
    }
}

// ===========================================================================
// Grid projections
// ===========================================================================

/// Project query logs into the shared results grid.
///
/// Durations and row counts stay numeric so the grid sorts them as numbers;
/// the human-readable form would sort 9ms after 100ms.
pub fn logs_to_result(logs: &[QueryLog]) -> QueryResult {
    let columns = vec![
        "Started".to_string(),
        "Connection".to_string(),
        "Database".to_string(),
        "Type".to_string(),
        "Status".to_string(),
        "Duration (ms)".to_string(),
        "Rows".to_string(),
        "SQL".to_string(),
        "Error".to_string(),
    ];
    let rows = logs
        .iter()
        .map(|log| {
            vec![
                serde_json::Value::String(format_timestamp(log.started_at)),
                serde_json::Value::String(log.connection_name.clone()),
                json_opt_string(log.database.as_deref()),
                serde_json::Value::String(mon::query_type_key(log.query_type).to_string()),
                serde_json::Value::String(mon::status_key(log.status).to_string()),
                json_opt_u64(log.duration_ms),
                json_opt_u64(log.row_count),
                serde_json::Value::String(one_line_sql(&log.sql, SQL_CELL_CHARS)),
                json_opt_string(log.error.as_deref()),
            ]
        })
        .collect();
    QueryResult::with_data(columns, rows)
}

/// Project server sessions into the shared results grid.
pub fn processes_to_result(processes: &[mon::ActiveQuery]) -> QueryResult {
    let columns = vec![
        "PID".to_string(),
        "User".to_string(),
        "Database".to_string(),
        "Client".to_string(),
        "State".to_string(),
        "Duration (ms)".to_string(),
        "Query".to_string(),
    ];
    let rows = processes
        .iter()
        .map(|p| {
            vec![
                serde_json::Value::Number(p.pid.into()),
                json_opt_string(p.user.as_deref()),
                json_opt_string(p.database.as_deref()),
                json_opt_string(p.client_addr.as_deref()),
                json_opt_string(p.state.as_deref()),
                match p.duration_ms {
                    Some(ms) => serde_json::Value::Number(ms.into()),
                    None => serde_json::Value::Null,
                },
                json_opt_string(p.query_text.as_deref()),
            ]
        })
        .collect();
    QueryResult::with_data(columns, rows)
}

/// A missing value becomes a grid NULL, which renders distinctly from the
/// literal string "NULL".
fn json_opt_string(v: Option<&str>) -> serde_json::Value {
    match v {
        Some(s) if !s.is_empty() => serde_json::Value::String(s.to_string()),
        _ => serde_json::Value::Null,
    }
}

fn json_opt_u64(v: Option<u64>) -> serde_json::Value {
    match v {
        Some(n) => serde_json::Value::Number(n.into()),
        None => serde_json::Value::Null,
    }
}

// ===========================================================================
// Filter state
// ===========================================================================

/// The filter panel's raw inputs.
///
/// Kept separate from [`mon::LogQuery`] because the panel holds text the user
/// is still typing ("12" on the way to "120"), which must not be pushed at
/// the log on every keystroke.
#[derive(Debug, Clone, Default)]
pub struct FilterState {
    pub connection_id: Option<String>,
    pub database: Option<String>,
    pub types: Vec<QueryType>,
    pub statuses: Vec<QueryStatus>,
    pub min_duration: String,
    pub max_duration: String,
    pub search_text: String,
    pub time_range: mon::TimeRange,
}

impl FilterState {
    /// Build the query this panel describes, as of `now`.
    ///
    /// Unparseable duration text is treated as "no bound" rather than as
    /// zero: a half-typed number must not silently hide every row.
    pub fn to_query(&self, now: DateTime<Utc>) -> mon::LogQuery {
        mon::LogQuery {
            filter: QueryLogFilter {
                connection_id: self.connection_id.clone(),
                database: self.database.clone(),
                query_type: None,
                status: None,
                min_duration: self.min_duration.trim().parse::<u64>().ok(),
                max_duration: self.max_duration.trim().parse::<u64>().ok(),
                start_date: self.time_range.start_rfc3339(now),
                end_date: None,
                search_text: match self.search_text.trim() {
                    "" => None,
                    s => Some(s.to_string()),
                },
                tags: None,
            },
            types: self.types.clone(),
            statuses: self.statuses.clone(),
        }
    }

    /// Whether anything is narrowing the log at all.
    pub fn is_active(&self) -> bool {
        self.connection_id.is_some()
            || self.database.is_some()
            || !self.types.is_empty()
            || !self.statuses.is_empty()
            || !self.min_duration.trim().is_empty()
            || !self.max_duration.trim().is_empty()
            || !self.search_text.trim().is_empty()
            || self.time_range != mon::TimeRange::All
    }

    pub fn toggle_type(&mut self, t: QueryType) {
        match self.types.iter().position(|x| *x == t) {
            Some(i) => {
                self.types.remove(i);
            }
            None => self.types.push(t),
        }
    }

    pub fn toggle_status(&mut self, s: QueryStatus) {
        match self.statuses.iter().position(|x| *x == s) {
            Some(i) => {
                self.statuses.remove(i);
            }
            None => self.statuses.push(s),
        }
    }
}

// ===========================================================================
// Sorting
// ===========================================================================

/// Sort options, mirroring [`QueryLogSortField`] which is not `PartialEq`
/// and so cannot drive a selectable list directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    StartedAt,
    DurationMs,
    RowCount,
    ConnectionName,
}

impl SortField {
    pub const ALL: [SortField; 4] = [
        SortField::StartedAt,
        SortField::DurationMs,
        SortField::RowCount,
        SortField::ConnectionName,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SortField::StartedAt => "Started at",
            SortField::DurationMs => "Duration",
            SortField::RowCount => "Rows",
            SortField::ConnectionName => "Connection",
        }
    }

    pub fn field(self) -> QueryLogSortField {
        match self {
            SortField::StartedAt => QueryLogSortField::StartedAt,
            SortField::DurationMs => QueryLogSortField::DurationMs,
            SortField::RowCount => QueryLogSortField::RowCount,
            SortField::ConnectionName => QueryLogSortField::ConnectionName,
        }
    }
}

/// The backend sort for a field and a descending toggle.
pub fn sort_for(field: SortField, desc: bool) -> QueryLogSort {
    QueryLogSort {
        field: field.field(),
        direction: if desc {
            SortDirection::Desc
        } else {
            SortDirection::Asc
        },
    }
}

// ===========================================================================
// Polling
// ===========================================================================

/// The auto-refresh period, or `None` when paused or set to "Off".
pub fn refresh_interval(auto_refresh: bool, refresh_seconds: u64) -> Option<Duration> {
    if auto_refresh && refresh_seconds > 0 {
        Some(Duration::from_secs(refresh_seconds))
    } else {
        None
    }
}

/// Time left until the next tick, or `None` if one is due (or paused).
///
/// Split out from the frame loop so the deadline arithmetic is not
/// tangled with drawing. A remainder of exactly zero counts as due:
/// asking the UI to repaint in zero time would spin a frame without ever
/// firing the tick. Never having ticked also counts as due, so the panel
/// populates on open.
pub fn remaining_until_tick(
    interval: Option<Duration>,
    last_tick: Option<Instant>,
    now: Instant,
) -> Option<Duration> {
    let interval = interval?;
    let last = last_tick?;
    interval
        .checked_sub(now.duration_since(last))
        .filter(|remaining| !remaining.is_zero())
}

/// Label for an auto-refresh choice. `0` is "Off".
pub fn interval_label(seconds: u64) -> String {
    if seconds == 0 {
        "Off".to_string()
    } else {
        format!("{}s", seconds)
    }
}

// ===========================================================================
// Export
// ===========================================================================

pub fn export_format_label(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Csv => "CSV",
        ExportFormat::Json => "JSON",
        ExportFormat::Txt => "TXT",
    }
}

/// File name for an export taken at `stamp`, e.g.
/// `query-logs-2026-09-08-120000.csv`. The directory is the shell's choice.
pub fn export_file_name(format: ExportFormat, stamp: DateTime<Local>) -> String {
    let extension = match format {
        ExportFormat::Csv => "csv",
        ExportFormat::Json => "json",
        ExportFormat::Txt => "txt",
    };
    format!(
        "query-logs-{}.{}",
        stamp.format("%Y-%m-%d-%H%M%S"),
        extension
    )
}

// ===========================================================================
// Charts and statistics
// ===========================================================================

/// Project one field of a sample window into downsampled plot points.
pub fn series_points(
    samples: &[mon::MetricsSample],
    pick: impl Fn(&mon::MetricsSample) -> f64,
) -> Vec<[f64; 2]> {
    let points: Vec<[f64; 2]> = samples.iter().map(|s| [s.t_ms as f64, pick(s)]).collect();
    mon::downsample(&points, MAX_PLOT_POINTS)
}

/// Which palette role a status is drawn in. Each shell maps it to a colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTone {
    Success,
    Destructive,
    Primary,
    Muted,
}

pub fn status_tone(status: QueryStatus) -> StatusTone {
    match status {
        QueryStatus::Completed => StatusTone::Success,
        QueryStatus::Failed => StatusTone::Destructive,
        QueryStatus::Running => StatusTone::Primary,
        QueryStatus::Cancelled => StatusTone::Muted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- formatting -------------------------------------------------------

    #[test]
    fn durations_switch_to_seconds_at_one_second() {
        assert_eq!(format_duration(None), "-");
        assert_eq!(format_duration(Some(0)), "0ms");
        assert_eq!(format_duration(Some(999)), "999ms");
        assert_eq!(format_duration(Some(1_000)), "1.00s");
        assert_eq!(format_duration(Some(1_234)), "1.23s");
        assert_eq!(format_duration(Some(90_000)), "90.00s");
    }

    #[test]
    fn negative_session_ages_are_shown_as_absent() {
        assert_eq!(format_duration_i64(Some(500)), "500ms");
        assert_eq!(format_duration_i64(None), "-");
        // A clock jump on the server, not a half-second-in-the-past query.
        assert_eq!(format_duration_i64(Some(-500)), "-");
    }

    #[test]
    fn counts_get_compact_suffixes() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1_000), "1.0K");
        assert_eq!(format_count(1_500), "1.5K");
        assert_eq!(format_count(999_999), "1000.0K");
        assert_eq!(format_count(1_000_000), "1.0M");
        assert_eq!(format_count(2_500_000_000), "2.5B");
    }

    #[test]
    fn bytes_use_binary_prefixes_and_stay_exact_below_a_kilobyte() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1_023), "1023 B");
        assert_eq!(format_bytes(1_024), "1.0 KB");
        assert_eq!(format_bytes(1_536), "1.5 KB");
        assert_eq!(format_bytes(1_048_576), "1.0 MB");
        assert_eq!(format_bytes(3 * 1_073_741_824), "3.0 GB");
    }

    #[test]
    fn byte_formatting_saturates_at_the_largest_unit() {
        // Must not index past the unit table on an absurd size.
        assert!(format_bytes(u64::MAX).ends_with(" TB"));
    }

    #[test]
    fn percentages_of_nothing_are_zero_not_nan() {
        assert_eq!(percentage(0, 0), 0.0);
        assert_eq!(percentage(1, 4), 25.0);
        assert_eq!(percentage(4, 4), 100.0);
    }

    #[test]
    fn sql_is_flattened_and_elided_for_a_grid_cell() {
        assert_eq!(one_line_sql("SELECT *\n  FROM t", 40), "SELECT * FROM t");
        let long = one_line_sql(&"a".repeat(300), 10);
        assert_eq!(long.chars().count(), 11);
        assert!(long.ends_with('…'));
    }

    #[test]
    fn clock_labels_reject_nonsense_timestamps() {
        assert!(format_clock_ms(f64::NAN).is_empty());
        assert_eq!(format_clock_ms(0.0).len(), 8);
    }

    // -- grid projections -------------------------------------------------

    fn log(id: &str, sql: &str) -> QueryLog {
        QueryLog::new(
            id.to_string(),
            "conn-1".to_string(),
            "Local".to_string(),
            Some("shop".to_string()),
            sql.to_string(),
        )
    }

    #[test]
    fn log_rows_keep_numbers_numeric_so_the_grid_sorts_them() {
        let mut a = log("1", "SELECT 1");
        a.complete(9, Some(3));
        let mut b = log("2", "SELECT 2");
        b.complete(100, Some(4));
        let result = logs_to_result(&[a, b]);

        assert_eq!(result.columns.len(), 9);
        assert_eq!(result.rows.len(), 2);
        // Text durations would sort "100" before "9".
        assert_eq!(result.rows[0][5], serde_json::json!(9));
        assert_eq!(result.rows[1][5], serde_json::json!(100));
        assert_eq!(result.rows[0][3], serde_json::json!("SELECT"));
    }

    #[test]
    fn missing_log_fields_become_grid_nulls() {
        let mut l = log("1", "SELECT 1");
        l.database = None;
        let result = logs_to_result(&[l]);
        assert_eq!(result.rows[0][2], serde_json::Value::Null);
        // Still running: no duration, no rows, no error.
        assert_eq!(result.rows[0][5], serde_json::Value::Null);
        assert_eq!(result.rows[0][8], serde_json::Value::Null);
    }

    #[test]
    fn process_rows_project_every_column_in_order() {
        let p = mon::ActiveQuery {
            pid: 42,
            user: Some("app".into()),
            database: Some("shop".into()),
            client_addr: None,
            query_start: None,
            state: Some("active".into()),
            query_text: Some("SELECT 1".into()),
            duration_ms: Some(1_500),
        };
        let result = processes_to_result(&[p]);
        assert_eq!(result.columns.len(), 7);
        assert_eq!(result.rows[0][0], serde_json::json!(42));
        assert_eq!(result.rows[0][3], serde_json::Value::Null);
        assert_eq!(result.rows[0][5], serde_json::json!(1_500));
    }

    // -- filter state -----------------------------------------------------

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-08T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn an_empty_panel_produces_an_unfiltered_query() {
        let f = FilterState::default();
        assert!(!f.is_active());
        let q = f.to_query(now());
        assert!(q.types.is_empty());
        assert!(q.statuses.is_empty());
        assert!(q.filter.connection_id.is_none());
        assert!(q.filter.start_date.is_none());
        assert!(q.filter.search_text.is_none());
        assert!(q.matches(&log("1", "SELECT 1")));
    }

    #[test]
    fn every_ticked_type_and_status_reaches_the_query() {
        // The React panel dropped all but the first of each.
        let f = FilterState {
            types: vec![QueryType::Insert, QueryType::Delete],
            statuses: vec![QueryStatus::Failed, QueryStatus::Cancelled],
            ..Default::default()
        };
        let q = f.to_query(now());
        assert_eq!(q.types.len(), 2);
        assert_eq!(q.statuses.len(), 2);
        // The single-valued core fields stay unset; the sets carry it all.
        assert!(q.filter.query_type.is_none());
        assert!(q.filter.status.is_none());
    }

    #[test]
    fn half_typed_duration_bounds_are_ignored_not_treated_as_zero() {
        let f = FilterState {
            min_duration: "  120 ".to_string(),
            max_duration: "12x".to_string(),
            ..Default::default()
        };
        let q = f.to_query(now());
        assert_eq!(q.filter.min_duration, Some(120));
        // "12x" is not a bound; treating it as 0 would hide every row.
        assert_eq!(q.filter.max_duration, None);
    }

    #[test]
    fn blank_search_text_is_no_search_at_all() {
        let f = FilterState {
            search_text: "   ".to_string(),
            ..Default::default()
        };
        assert!(f.to_query(now()).filter.search_text.is_none());

        let f = FilterState {
            search_text: " users ".to_string(),
            ..Default::default()
        };
        assert_eq!(
            f.to_query(now()).filter.search_text.as_deref(),
            Some("users")
        );
    }

    #[test]
    fn time_range_becomes_a_parseable_start_date() {
        let f = FilterState {
            time_range: mon::TimeRange::LastHour,
            ..Default::default()
        };
        let q = f.to_query(now());
        assert_eq!(
            q.filter.start_date.as_deref(),
            Some("2026-09-08T11:00:00+00:00")
        );
    }

    #[test]
    fn every_field_counts_as_an_active_filter() {
        let base = FilterState::default();
        assert!(!base.is_active());

        let cases = [
            FilterState {
                connection_id: Some("c".into()),
                ..Default::default()
            },
            FilterState {
                database: Some("db".into()),
                ..Default::default()
            },
            FilterState {
                types: vec![QueryType::Select],
                ..Default::default()
            },
            FilterState {
                statuses: vec![QueryStatus::Failed],
                ..Default::default()
            },
            FilterState {
                min_duration: "1".into(),
                ..Default::default()
            },
            FilterState {
                max_duration: "1".into(),
                ..Default::default()
            },
            FilterState {
                search_text: "x".into(),
                ..Default::default()
            },
            FilterState {
                time_range: mon::TimeRange::Last7Days,
                ..Default::default()
            },
        ];
        for case in cases {
            assert!(case.is_active(), "{:?} should count as filtered", case);
        }
        // Whitespace alone is not a filter.
        assert!(!FilterState {
            search_text: "  ".into(),
            min_duration: " ".into(),
            ..Default::default()
        }
        .is_active());
    }

    #[test]
    fn toggling_a_type_adds_then_removes_it() {
        let mut f = FilterState::default();
        f.toggle_type(QueryType::Select);
        f.toggle_type(QueryType::Insert);
        assert_eq!(f.types, vec![QueryType::Select, QueryType::Insert]);
        f.toggle_type(QueryType::Select);
        assert_eq!(f.types, vec![QueryType::Insert]);

        f.toggle_status(QueryStatus::Failed);
        assert_eq!(f.statuses, vec![QueryStatus::Failed]);
        f.toggle_status(QueryStatus::Failed);
        assert!(f.statuses.is_empty());
    }

    // -- polling ----------------------------------------------------------

    #[test]
    fn no_interval_while_paused_or_switched_off() {
        assert!(refresh_interval(false, 10).is_none());
        assert!(refresh_interval(true, 0).is_none(), "Off means no polling");
        assert_eq!(refresh_interval(true, 10), Some(Duration::from_secs(10)));
    }

    #[test]
    fn a_tick_is_due_immediately_and_then_once_per_interval() {
        let interval = refresh_interval(true, 10);
        let start = Instant::now();
        // Never ticked: due now, so the panel populates on open.
        assert_eq!(remaining_until_tick(interval, None, start), None);

        let last = Some(start);
        let after_4s = start + Duration::from_secs(4);
        assert_eq!(
            remaining_until_tick(interval, last, after_4s),
            Some(Duration::from_secs(6))
        );
        let after_10s = start + Duration::from_secs(10);
        assert_eq!(remaining_until_tick(interval, last, after_10s), None);
        // Overdue is still "due now", not a negative wait.
        let after_30s = start + Duration::from_secs(30);
        assert_eq!(remaining_until_tick(interval, last, after_30s), None);
    }

    #[test]
    fn a_paused_monitor_never_reports_a_deadline() {
        let now = Instant::now();
        assert!(remaining_until_tick(None, Some(now), now).is_none());
    }

    #[test]
    fn interval_labels_name_off_explicitly() {
        assert_eq!(interval_label(0), "Off");
        assert_eq!(interval_label(30), "30s");
        for seconds in REFRESH_SECONDS {
            assert!(!interval_label(seconds).is_empty());
        }
    }

    // -- sort mapping -----------------------------------------------------

    #[test]
    fn sort_direction_follows_the_toggle() {
        let desc = sort_for(SortField::DurationMs, true);
        assert!(matches!(desc.direction, SortDirection::Desc));
        assert!(matches!(desc.field, QueryLogSortField::DurationMs));
        assert!(matches!(
            sort_for(SortField::DurationMs, false).direction,
            SortDirection::Asc
        ));
    }

    #[test]
    fn every_sort_field_has_a_label_and_a_backend_field() {
        for field in SortField::ALL {
            assert!(!field.label().is_empty());
            let _ = field.field();
        }
    }

    // -- export -----------------------------------------------------------

    #[test]
    fn export_names_carry_the_stamp_and_matching_extension() {
        use chrono::TimeZone;
        let stamp = Local.with_ymd_and_hms(2026, 9, 8, 12, 0, 5).unwrap();
        assert_eq!(
            export_file_name(ExportFormat::Csv, stamp),
            "query-logs-2026-09-08-120005.csv"
        );
        assert!(export_file_name(ExportFormat::Json, stamp).ends_with(".json"));
        assert!(export_file_name(ExportFormat::Txt, stamp).ends_with(".txt"));
        assert_eq!(export_format_label(ExportFormat::Json), "JSON");
    }

    // -- charts and statistics --------------------------------------------

    #[test]
    fn series_points_pick_the_field_and_cap_the_point_count() {
        let samples: Vec<mon::MetricsSample> = (0..1_000)
            .map(|i| mon::MetricsSample {
                t_ms: i * 1_000,
                connections: i as f64,
                active: 1.0,
                tx_per_sec: 0.0,
            })
            .collect();
        let short = series_points(&samples[..3], |s| s.connections);
        assert_eq!(short, vec![[0.0, 0.0], [1_000.0, 1.0], [2_000.0, 2.0]]);

        let long = series_points(&samples, |s| s.active);
        assert!(long.len() <= MAX_PLOT_POINTS);
        assert!(long.iter().all(|p| p[1] == 1.0));
    }

    #[test]
    fn every_status_has_a_distinct_tone() {
        let tones: Vec<StatusTone> = mon::QUERY_STATUSES
            .iter()
            .map(|s| status_tone(*s))
            .collect();
        assert_eq!(status_tone(QueryStatus::Failed), StatusTone::Destructive);
        assert_eq!(status_tone(QueryStatus::Completed), StatusTone::Success);
        for (i, a) in tones.iter().enumerate() {
            assert!(tones[i + 1..].iter().all(|b| b != a));
        }
    }
}
