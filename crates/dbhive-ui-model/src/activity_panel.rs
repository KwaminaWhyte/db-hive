//! What the Slint Activity Monitor shows, as plain values.
//!
//! `activity_monitor` holds the formatting, the filter inputs and the poll
//! arithmetic. This module is the layer the panel needs on top of it: the grid
//! projections with readable cells, the header-click to sort mapping, the
//! stat tiles and breakdown bars, the detail pane's facts, the chart paths,
//! and the gate that keeps polling from overlapping itself or running while
//! nobody is looking.
//!
//! No toolkit types. The shell turns these into its own structs.

use std::time::{Duration, Instant};

use dbhive_core::drivers::QueryResult;
use dbhive_core::models::{ActivityStats, QueryLog, QueryStatus};
use dbhive_core::service_monitoring as mon;

use crate::activity_monitor::{
    format_clock_ms, format_count, format_duration, format_duration_i64, format_timestamp,
    one_line_sql, percentage, remaining_until_tick, status_tone, SortField, StatusTone,
    SQL_CELL_CHARS,
};

// ===========================================================================
// Log grid and sorting
// ===========================================================================

/// Column titles of the query-log grid, in order.
pub const LOG_COLUMNS: [&str; 8] = [
    "Started",
    "Connection",
    "Database",
    "Type",
    "Status",
    "Duration",
    "Rows",
    "SQL",
];

/// Widths of those columns in logical pixels. SQL takes the rest of the row,
/// so it is the widest.
pub const LOG_COLUMN_WIDTHS: [f32; 8] = [140.0, 140.0, 90.0, 84.0, 90.0, 88.0, 60.0, 388.0];

/// Titles of the process grid.
pub const PROCESS_COLUMNS: [&str; 6] = ["PID", "User", "Database", "State", "Duration", "Query"];

pub const PROCESS_COLUMN_WIDTHS: [f32; 6] = [80.0, 120.0, 120.0, 110.0, 90.0, 560.0];

/// The sort a header click on `column` asks for, if that column can be sorted.
///
/// The backend sorts four things; the other columns are display only, and a
/// click on them does nothing rather than pretending to sort.
pub fn sort_field_for_column(column: usize) -> Option<SortField> {
    match column {
        0 => Some(SortField::StartedAt),
        1 => Some(SortField::ConnectionName),
        5 => Some(SortField::DurationMs),
        6 => Some(SortField::RowCount),
        _ => None,
    }
}

/// The grid column a sort field is shown in, for the header arrow.
pub fn column_for_sort_field(field: SortField) -> usize {
    match field {
        SortField::StartedAt => 0,
        SortField::ConnectionName => 1,
        SortField::DurationMs => 5,
        SortField::RowCount => 6,
    }
}

/// The sort after a header click.
///
/// Clicking the sorted column flips its direction. Clicking another starts in
/// the direction a reader wants first: newest, slowest and largest at the top,
/// connection names A to Z.
pub fn sort_after_click(current: (SortField, bool), column: usize) -> Option<(SortField, bool)> {
    let field = sort_field_for_column(column)?;
    if field == current.0 {
        return Some((field, !current.1));
    }
    Some((field, field != SortField::ConnectionName))
}

/// Project query logs into the shared results grid, with readable cells.
///
/// The backend already sorted and paged the logs, so cells can be text: "12ms"
/// instead of a number the grid would have to re-sort.
pub fn log_grid(logs: &[QueryLog]) -> QueryResult {
    let columns = LOG_COLUMNS.iter().map(|c| c.to_string()).collect();
    let rows = logs
        .iter()
        .map(|log| {
            vec![
                text(format_timestamp(log.started_at)),
                text(log.connection_name.clone()),
                opt_text(log.database.as_deref()),
                text(mon::query_type_key(log.query_type).to_string()),
                text(status_label(log.status).to_string()),
                match log.duration_ms {
                    Some(ms) => text(format_duration(Some(ms))),
                    None => serde_json::Value::Null,
                },
                match log.row_count {
                    Some(n) => text(format_count(n)),
                    None => serde_json::Value::Null,
                },
                text(one_line_sql(&log.sql, SQL_CELL_CHARS)),
            ]
        })
        .collect();
    QueryResult::with_data(columns, rows)
}

/// Project server sessions into the grid.
pub fn process_grid(processes: &[mon::ActiveQuery]) -> QueryResult {
    let columns = PROCESS_COLUMNS.iter().map(|c| c.to_string()).collect();
    let rows = processes
        .iter()
        .map(|p| {
            vec![
                text(p.pid.to_string()),
                opt_text(p.user.as_deref()),
                opt_text(p.database.as_deref()),
                opt_text(p.state.as_deref()),
                match p.duration_ms {
                    Some(_) => text(format_duration_i64(p.duration_ms)),
                    None => serde_json::Value::Null,
                },
                match p.query_text.as_deref() {
                    Some(q) if !q.trim().is_empty() => text(one_line_sql(q, SQL_CELL_CHARS)),
                    _ => serde_json::Value::Null,
                },
            ]
        })
        .collect();
    QueryResult::with_data(columns, rows)
}

fn text(s: String) -> serde_json::Value {
    serde_json::Value::String(s)
}

fn opt_text(s: Option<&str>) -> serde_json::Value {
    match s {
        Some(s) if !s.is_empty() => text(s.to_string()),
        _ => serde_json::Value::Null,
    }
}

pub fn status_label(status: QueryStatus) -> &'static str {
    match status {
        QueryStatus::Running => "Running",
        QueryStatus::Completed => "Completed",
        QueryStatus::Failed => "Failed",
        QueryStatus::Cancelled => "Cancelled",
    }
}

// ===========================================================================
// Pager
// ===========================================================================

/// "Page 2 of 5 · 213 logs", or a plain statement when there is nothing.
pub fn page_label(page: usize, total_pages: usize, total: usize) -> String {
    if total == 0 {
        return "No logs".to_string();
    }
    format!(
        "Page {} of {} · {} {}",
        page + 1,
        total_pages.max(1),
        format_count(total as u64),
        if total == 1 { "log" } else { "logs" }
    )
}

/// The line beside the pager: how much of the whole log the filters keep.
pub fn log_summary(matching: usize, overall: usize, filtered: bool) -> String {
    let noun = |n: usize| if n == 1 { "query" } else { "queries" };
    match (overall, filtered) {
        (0, _) => String::new(),
        (_, true) => format!(
            "{} of {} {} match the filters",
            format_count(matching as u64),
            format_count(overall as u64),
            noun(overall)
        ),
        (_, false) => format!(
            "{} {} in the log",
            format_count(overall as u64),
            noun(overall)
        ),
    }
}

pub fn can_go_prev(page: usize) -> bool {
    page > 0
}

pub fn can_go_next(page: usize, total_pages: usize) -> bool {
    page + 1 < total_pages
}

// ===========================================================================
// Statistics
// ===========================================================================

/// One number with a caption.
#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    pub label: &'static str,
    pub value: String,
    pub tone: StatusTone,
}

/// The four tiles over the log: how many ran, how many failed, how long they
/// took, how many rows they moved.
pub fn stat_tiles(stats: &ActivityStats) -> Vec<Tile> {
    vec![
        Tile {
            label: "Queries",
            value: format_count(stats.total_queries as u64),
            tone: StatusTone::Muted,
        },
        Tile {
            label: "Failed",
            value: format_count(stats.failed_queries as u64),
            tone: if stats.failed_queries > 0 {
                StatusTone::Destructive
            } else {
                StatusTone::Muted
            },
        },
        Tile {
            label: "Avg duration",
            value: if stats.total_queries == 0 {
                "-".to_string()
            } else {
                format_duration(Some(stats.avg_duration.round() as u64))
            },
            tone: StatusTone::Muted,
        },
        Tile {
            label: "Rows",
            value: format_count(stats.total_rows),
            tone: StatusTone::Muted,
        },
    ]
}

/// One bar of a breakdown.
#[derive(Debug, Clone, PartialEq)]
pub struct BarRow {
    pub label: String,
    pub count: usize,
    /// Share of the total, 0 to 1, for the bar's length.
    pub fraction: f32,
    /// "42%" for the label next to the bar.
    pub percent_label: String,
    pub tone: StatusTone,
}

fn bar(label: &str, count: usize, total: usize, tone: StatusTone) -> BarRow {
    let pct = percentage(count, total);
    BarRow {
        label: label.to_string(),
        count,
        fraction: (pct / 100.0) as f32,
        percent_label: format!("{:.0}%", pct),
        tone,
    }
}

/// Queries per type, most used first. Types that never ran are left out.
pub fn type_breakdown(stats: &ActivityStats) -> Vec<BarRow> {
    let mut rows: Vec<BarRow> = mon::QUERY_TYPES
        .iter()
        .filter_map(|t| {
            let n = stats.queries_by_type.get(t).copied().unwrap_or(0);
            (n > 0).then(|| {
                bar(
                    mon::query_type_key(*t),
                    n,
                    stats.total_queries,
                    StatusTone::Primary,
                )
            })
        })
        .collect();
    // Stable sort: ties keep the filter panel's order.
    rows.sort_by(|a, b| b.count.cmp(&a.count));
    rows
}

/// Queries per status, in the order the filter panel lists them.
pub fn status_breakdown(stats: &ActivityStats) -> Vec<BarRow> {
    mon::QUERY_STATUSES
        .iter()
        .filter_map(|s| {
            let n = stats
                .queries_by_status
                .get(mon::status_key(*s))
                .copied()
                .unwrap_or(0);
            (n > 0).then(|| bar(status_label(*s), n, stats.total_queries, status_tone(*s)))
        })
        .collect()
}

// ===========================================================================
// Detail
// ===========================================================================

/// Everything the expanded row shows.
#[derive(Debug, Clone, PartialEq)]
pub struct LogDetail {
    pub status: String,
    pub tone: StatusTone,
    /// Label and value pairs, in display order.
    pub facts: Vec<(String, String)>,
    pub sql: String,
    /// Empty when the statement did not fail.
    pub error: String,
}

pub fn log_detail(log: &QueryLog) -> LogDetail {
    let mut facts = vec![
        ("Started".to_string(), format_timestamp(log.started_at)),
        ("Duration".to_string(), format_duration(log.duration_ms)),
        (
            "Rows".to_string(),
            log.row_count.map_or("-".to_string(), |n| n.to_string()),
        ),
        (
            "Type".to_string(),
            mon::query_type_key(log.query_type).to_string(),
        ),
        ("Connection".to_string(), log.connection_name.clone()),
    ];
    if let Some(db) = log.database.as_deref().filter(|d| !d.is_empty()) {
        facts.push(("Database".to_string(), db.to_string()));
    }
    LogDetail {
        status: status_label(log.status).to_string(),
        tone: status_tone(log.status),
        facts,
        sql: log.sql.clone(),
        error: log.error.clone().unwrap_or_default(),
    }
}

// ===========================================================================
// Live server tiles
// ===========================================================================

/// The tiles over the server charts. `rate` is the newest derived
/// transactions per second, which the stats alone cannot give (they carry the
/// server's cumulative counter).
pub fn live_tiles(latest: Option<&mon::ServerStats>, rate: Option<f64>) -> Vec<Tile> {
    let dash = || "-".to_string();
    let stat = |label, value: Option<String>, tone| Tile {
        label,
        value: value.unwrap_or_else(dash),
        tone,
    };
    vec![
        stat(
            "Connections",
            latest.map(|s| format_count(s.numeric_connections.max(0) as u64)),
            StatusTone::Primary,
        ),
        stat(
            "Active",
            latest.map(|s| format_count(s.active_connections.max(0) as u64)),
            StatusTone::Success,
        ),
        stat(
            "Cache hit",
            latest
                .and_then(|s| s.cache_hit_ratio)
                .map(|r| format!("{:.1}%", r * 100.0)),
            StatusTone::Muted,
        ),
        stat(
            "Tx / sec",
            rate.map(|r| format!("{:.1}", r)),
            StatusTone::Muted,
        ),
        stat(
            "Deadlocks",
            latest.and_then(|s| s.deadlocks).map(|d| d.to_string()),
            match latest.and_then(|s| s.deadlocks) {
                Some(d) if d > 0 => StatusTone::Destructive,
                _ => StatusTone::Muted,
            },
        ),
    ]
}

// ===========================================================================
// Chart
// ===========================================================================

/// Side of the square coordinate space chart paths are written in. The Slint
/// `Path` maps it onto whatever size the chart is drawn at, so the paths do
/// not depend on the window.
pub const CHART_VIEW: f64 = 1000.0;

/// One drawn series.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartLineView {
    pub name: &'static str,
    /// SVG path commands in the [`CHART_VIEW`] space, for the line.
    pub commands: String,
    /// The same line closed down to the baseline, for the fill under it.
    pub area: String,
    /// Newest value, formatted, for the legend.
    pub last: String,
    pub tone: StatusTone,
}

/// A chart ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartView {
    pub lines: Vec<ChartLineView>,
    /// Axis labels: the top of the scale, the middle, and the two time ends.
    pub y_top: String,
    pub y_mid: String,
    pub x_start: String,
    pub x_end: String,
}

/// The smallest "round" number (1, 2 or 5 times a power of ten) that is at
/// least `max`, so the axis reads 5, 10, 20, 50 rather than 7.3.
pub fn nice_ceiling(max: f64) -> f64 {
    if !max.is_finite() || max <= 0.0 {
        return 1.0;
    }
    let magnitude = 10f64.powf(max.log10().floor());
    let normalized = max / magnitude;
    let step = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    step * magnitude
}

/// Axis and legend number: whole when it is whole, one decimal when small,
/// compact when large.
pub fn format_axis_value(v: f64) -> String {
    if !v.is_finite() {
        return "-".to_string();
    }
    if v >= 1000.0 {
        format_count(v as u64)
    } else if (v - v.round()).abs() < 1e-9 || v >= 100.0 {
        format!("{:.0}", v)
    } else {
        format!("{:.1}", v)
    }
}

/// Path commands for a series, scaled into [`CHART_VIEW`] with the y axis
/// flipped (larger values are higher). Returns `(line, area)`.
pub fn plot_path(points: &[[f64; 2]], x_min: f64, x_max: f64, y_max: f64) -> (String, String) {
    if points.is_empty() {
        return (String::new(), String::new());
    }
    let span = x_max - x_min;
    let y_max = if y_max > 0.0 { y_max } else { 1.0 };
    let map = |p: &[f64; 2]| {
        let x = if span > 0.0 {
            ((p[0] - x_min) / span).clamp(0.0, 1.0) * CHART_VIEW
        } else {
            0.0
        };
        let y = CHART_VIEW - (p[1] / y_max).clamp(0.0, 1.0) * CHART_VIEW;
        (x, y)
    };

    let mut line = String::new();
    let mut first = (0.0, 0.0);
    let mut last = (0.0, 0.0);
    for (i, p) in points.iter().enumerate() {
        let (x, y) = map(p);
        if i == 0 {
            first = (x, y);
            line.push_str(&format!("M {:.1} {:.1}", x, y));
        } else {
            line.push_str(&format!(" L {:.1} {:.1}", x, y));
        }
        last = (x, y);
    }
    let area = format!(
        "{} L {:.1} {:.1} L {:.1} {:.1} Z",
        line, last.0, CHART_VIEW, first.0, CHART_VIEW
    );
    let _ = first;
    (line, area)
}

fn chart_of(
    window: &[mon::MetricsSample],
    series: &[(&'static str, StatusTone, fn(&mon::MetricsSample) -> f64)],
) -> Option<ChartView> {
    if window.len() < 2 {
        return None;
    }
    let x_min = window.first()?.t_ms as f64;
    let x_max = window.last()?.t_ms as f64;

    let all: Vec<Vec<[f64; 2]>> = series
        .iter()
        .map(|(_, _, pick)| crate::activity_monitor::series_points(window, pick))
        .collect();
    let peak = all
        .iter()
        .flat_map(|pts| pts.iter().map(|p| p[1]))
        .fold(0.0_f64, f64::max);
    let y_max = nice_ceiling(peak);

    let lines = series
        .iter()
        .zip(&all)
        .map(|((name, tone, pick), pts)| {
            let (commands, area) = plot_path(pts, x_min, x_max, y_max);
            ChartLineView {
                name,
                commands,
                area,
                last: window
                    .last()
                    .map_or("-".to_string(), |s| format_axis_value(pick(s))),
                tone: *tone,
            }
        })
        .collect();

    Some(ChartView {
        lines,
        y_top: format_axis_value(y_max),
        y_mid: format_axis_value(y_max / 2.0),
        x_start: format_clock_ms(x_min),
        x_end: format_clock_ms(x_max),
    })
}

/// Connections and active sessions, which share a scale.
pub fn sessions_chart(window: &[mon::MetricsSample]) -> Option<ChartView> {
    chart_of(
        window,
        &[
            ("Connections", StatusTone::Primary, |s| s.connections),
            ("Active", StatusTone::Success, |s| s.active),
        ],
    )
}

/// Transactions per second, on its own scale: it differs from the session
/// counts by orders of magnitude and would flatten them.
pub fn transactions_chart(window: &[mon::MetricsSample]) -> Option<ChartView> {
    chart_of(window, &[("Tx / sec", StatusTone::Muted, |s| s.tx_per_sec)])
}

// ===========================================================================
// Polling gate
// ===========================================================================

/// Decides when a poll may start.
///
/// Two rules, both easy to get wrong inside a timer callback: a poll never
/// starts while the previous one is still in flight (a slow server would pile
/// up requests), and nothing starts while the monitor is hidden (a closed
/// panel must not keep querying a production database). Becoming visible
/// again clears the last-tick time so the panel catches up at once.
#[derive(Debug, Clone, Default)]
pub struct PollGate {
    visible: bool,
    outstanding: u8,
    last_tick: Option<Instant>,
}

impl PollGate {
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn is_busy(&self) -> bool {
        self.outstanding > 0
    }

    pub fn set_visible(&mut self, visible: bool) {
        if visible && !self.visible {
            self.last_tick = None;
        }
        self.visible = visible;
    }

    /// Start a scheduled poll of `requests` requests, if one is allowed now.
    pub fn try_begin(&mut self, interval: Option<Duration>, requests: u8, now: Instant) -> bool {
        if !self.visible || self.is_busy() || requests == 0 {
            return false;
        }
        if interval.is_none() {
            return false;
        }
        if remaining_until_tick(interval, self.last_tick, now).is_some() {
            return false;
        }
        self.last_tick = Some(now);
        self.outstanding = requests;
        true
    }

    /// Start a poll the user asked for. Not scheduled, so the interval and
    /// visibility do not apply; it still never overlaps.
    pub fn begin_manual(&mut self, requests: u8, now: Instant) -> bool {
        if self.is_busy() || requests == 0 {
            return false;
        }
        self.last_tick = Some(now);
        self.outstanding = requests;
        true
    }

    /// One request of the current poll finished.
    pub fn finish_one(&mut self) {
        self.outstanding = self.outstanding.saturating_sub(1);
    }

    /// Forget outstanding requests, for when their results will be discarded
    /// (the watched connection changed).
    pub fn abandon(&mut self) {
        self.outstanding = 0;
    }
}

// ===========================================================================
// Confirmations
// ===========================================================================

/// Text of the clear-logs confirmation.
pub fn clear_logs_message(count: usize) -> String {
    format!(
        "{} will be removed from the activity log. This cannot be undone.",
        if count == 1 {
            "1 query".to_string()
        } else {
            format!("{} queries", format_count(count as u64))
        }
    )
}

/// Text of the cancel-process confirmation.
pub fn kill_message(pid: i64, user: Option<&str>, query: Option<&str>) -> String {
    let who = match user.filter(|u| !u.is_empty()) {
        Some(u) => format!("process {} ({})", pid, u),
        None => format!("process {}", pid),
    };
    let what = match query.map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => format!("\n\n{}", one_line_sql(q, 160)),
        None => String::new(),
    };
    format!(
        "Ask the server to cancel the running statement of {}. The session stays connected.{}",
        who, what
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbhive_core::models::{QueryStatus, QueryType};
    use std::collections::HashMap;

    fn log(sql: &str) -> QueryLog {
        QueryLog::new(
            "1".into(),
            "c1".into(),
            "Local".into(),
            Some("shop".into()),
            sql.into(),
        )
    }

    // -- sorting ----------------------------------------------------------

    #[test]
    fn only_four_columns_sort_and_the_others_ignore_a_click() {
        assert_eq!(sort_field_for_column(0), Some(SortField::StartedAt));
        assert_eq!(sort_field_for_column(1), Some(SortField::ConnectionName));
        assert_eq!(sort_field_for_column(5), Some(SortField::DurationMs));
        assert_eq!(sort_field_for_column(6), Some(SortField::RowCount));
        for col in [2, 3, 4, 7, 8, 99] {
            assert_eq!(sort_field_for_column(col), None, "column {col}");
        }
        assert_eq!(sort_after_click((SortField::StartedAt, true), 7), None);
    }

    #[test]
    fn every_sortable_column_round_trips_through_its_field() {
        for field in SortField::ALL {
            assert_eq!(
                sort_field_for_column(column_for_sort_field(field)),
                Some(field)
            );
        }
    }

    #[test]
    fn clicking_the_sorted_column_flips_it_and_another_starts_at_the_useful_end() {
        assert_eq!(
            sort_after_click((SortField::StartedAt, true), 0),
            Some((SortField::StartedAt, false))
        );
        assert_eq!(
            sort_after_click((SortField::StartedAt, false), 0),
            Some((SortField::StartedAt, true))
        );
        // Slowest first, but connection names A to Z.
        assert_eq!(
            sort_after_click((SortField::StartedAt, true), 5),
            Some((SortField::DurationMs, true))
        );
        assert_eq!(
            sort_after_click((SortField::StartedAt, true), 1),
            Some((SortField::ConnectionName, false))
        );
    }

    // -- grids ------------------------------------------------------------

    #[test]
    fn log_cells_are_readable_and_a_running_query_has_blank_timings() {
        let mut done = log("SELECT\n  1");
        done.complete(1_500, Some(1_200));
        let running = log("SELECT 2");
        let grid = log_grid(&[done, running]);

        assert_eq!(grid.columns.len(), LOG_COLUMNS.len());
        assert_eq!(grid.rows[0].len(), LOG_COLUMNS.len());
        assert_eq!(grid.rows[0][5], serde_json::json!("1.50s"));
        assert_eq!(grid.rows[0][6], serde_json::json!("1.2K"));
        assert_eq!(grid.rows[0][7], serde_json::json!("SELECT 1"));
        assert_eq!(grid.rows[0][4], serde_json::json!("Completed"));
        // No duration or rows yet: NULL, not "-" text.
        assert_eq!(grid.rows[1][5], serde_json::Value::Null);
        assert_eq!(grid.rows[1][6], serde_json::Value::Null);
        assert_eq!(grid.rows[1][4], serde_json::json!("Running"));
    }

    #[test]
    fn process_cells_hide_an_empty_query_and_a_clock_jump() {
        let p = |pid, d, q: Option<&str>| mon::ActiveQuery {
            pid,
            user: Some("app".into()),
            database: None,
            client_addr: None,
            query_start: None,
            state: Some("active".into()),
            query_text: q.map(str::to_string),
            duration_ms: d,
        };
        let grid = process_grid(&[
            p(7, Some(2_500), Some("SELECT 1")),
            p(8, Some(-5), Some("  ")),
        ]);
        assert_eq!(grid.columns.len(), PROCESS_COLUMNS.len());
        assert_eq!(grid.rows[0][0], serde_json::json!("7"));
        assert_eq!(grid.rows[0][4], serde_json::json!("2.50s"));
        // A negative age is a server clock jump: shown as a dash, not "-5ms".
        assert_eq!(grid.rows[1][4], serde_json::json!("-"));
        assert_eq!(grid.rows[1][5], serde_json::Value::Null);
        assert_eq!(grid.rows[0][2], serde_json::Value::Null);
    }

    #[test]
    fn column_widths_match_column_titles() {
        assert_eq!(LOG_COLUMN_WIDTHS.len(), LOG_COLUMNS.len());
        assert_eq!(PROCESS_COLUMN_WIDTHS.len(), PROCESS_COLUMNS.len());
    }

    // -- pager ------------------------------------------------------------

    #[test]
    fn the_page_label_is_one_based_and_says_so_when_empty() {
        assert_eq!(page_label(0, 0, 0), "No logs");
        assert_eq!(page_label(0, 1, 1), "Page 1 of 1 · 1 log");
        assert_eq!(page_label(1, 5, 213), "Page 2 of 5 · 213 logs");
        assert_eq!(page_label(0, 40, 1_999), "Page 1 of 40 · 2.0K logs");
    }

    #[test]
    fn the_summary_says_how_much_the_filters_keep() {
        assert_eq!(log_summary(0, 0, false), "");
        assert_eq!(log_summary(1, 1, false), "1 query in the log");
        assert_eq!(log_summary(40, 40, false), "40 queries in the log");
        assert_eq!(
            log_summary(3, 40, true),
            "3 of 40 queries match the filters"
        );
        assert_eq!(
            log_summary(0, 40, true),
            "0 of 40 queries match the filters"
        );
    }

    #[test]
    fn the_pager_stops_at_both_ends() {
        assert!(!can_go_prev(0));
        assert!(can_go_prev(1));
        assert!(can_go_next(0, 2));
        assert!(!can_go_next(1, 2));
        assert!(!can_go_next(0, 0));
    }

    // -- statistics -------------------------------------------------------

    fn stats() -> ActivityStats {
        let mut by_type = HashMap::new();
        by_type.insert(QueryType::Select, 6);
        by_type.insert(QueryType::Delete, 2);
        by_type.insert(QueryType::Insert, 2);
        let mut by_status = HashMap::new();
        by_status.insert("completed".to_string(), 8);
        by_status.insert("failed".to_string(), 2);
        ActivityStats {
            total_queries: 10,
            failed_queries: 2,
            avg_duration: 41.6,
            total_rows: 12_500,
            queries_by_type: by_type,
            queries_by_status: by_status,
        }
    }

    #[test]
    fn tiles_flag_failures_and_show_a_dash_for_no_average() {
        let tiles = stat_tiles(&stats());
        assert_eq!(tiles.len(), 4);
        assert_eq!(tiles[0].value, "10");
        assert_eq!(tiles[1].value, "2");
        assert_eq!(tiles[1].tone, StatusTone::Destructive);
        assert_eq!(tiles[2].value, "42ms");
        assert_eq!(tiles[3].value, "12.5K");

        let none = ActivityStats {
            total_queries: 0,
            failed_queries: 0,
            avg_duration: 0.0,
            total_rows: 0,
            queries_by_type: HashMap::new(),
            queries_by_status: HashMap::new(),
        };
        let tiles = stat_tiles(&none);
        assert_eq!(tiles[2].value, "-");
        assert_eq!(tiles[1].tone, StatusTone::Muted);
    }

    #[test]
    fn breakdowns_skip_empty_rows_and_order_types_by_use() {
        let types = type_breakdown(&stats());
        let labels: Vec<&str> = types.iter().map(|r| r.label.as_str()).collect();
        // Select leads; Insert before Delete only because the panel lists it first.
        assert_eq!(labels, vec!["SELECT", "INSERT", "DELETE"]);
        assert_eq!(types[0].percent_label, "60%");
        assert!((types[0].fraction - 0.6).abs() < 1e-6);

        let statuses = status_breakdown(&stats());
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].label, "Completed");
        assert_eq!(statuses[0].tone, StatusTone::Success);
        assert_eq!(statuses[1].tone, StatusTone::Destructive);
    }

    #[test]
    fn breakdowns_of_an_empty_log_are_empty_not_nan() {
        let empty = ActivityStats {
            total_queries: 0,
            failed_queries: 0,
            avg_duration: 0.0,
            total_rows: 0,
            queries_by_type: HashMap::new(),
            queries_by_status: HashMap::new(),
        };
        assert!(type_breakdown(&empty).is_empty());
        assert!(status_breakdown(&empty).is_empty());
    }

    // -- detail -----------------------------------------------------------

    #[test]
    fn a_failed_log_carries_its_error_and_full_sql() {
        let mut l = log("SELECT\n  *\nFROM missing");
        l.fail(3, "no such table: missing".to_string());
        let d = log_detail(&l);
        assert_eq!(d.status, "Failed");
        assert_eq!(d.tone, StatusTone::Destructive);
        assert_eq!(d.error, "no such table: missing");
        // The detail keeps the statement as written; only the grid flattens it.
        assert_eq!(d.sql, "SELECT\n  *\nFROM missing");
        assert!(d.facts.iter().any(|(k, v)| k == "Database" && v == "shop"));
        assert!(d.facts.iter().any(|(k, v)| k == "Duration" && v == "3ms"));
    }

    #[test]
    fn a_log_without_a_database_omits_the_fact() {
        let mut l = log("SELECT 1");
        l.database = None;
        assert!(!log_detail(&l).facts.iter().any(|(k, _)| k == "Database"));
        assert_eq!(log_detail(&l).error, "");
    }

    // -- live tiles -------------------------------------------------------

    #[test]
    fn live_tiles_dash_out_what_the_driver_does_not_report() {
        let tiles = live_tiles(None, None);
        assert_eq!(tiles.len(), 5);
        assert!(tiles.iter().all(|t| t.value == "-"));

        let stats = mon::ServerStats {
            numeric_connections: 12,
            active_connections: 3,
            cache_hit_ratio: Some(0.9876),
            transactions_per_sec: Some(1_000_000.0),
            deadlocks: Some(2),
        };
        let tiles = live_tiles(Some(&stats), Some(14.25));
        assert_eq!(tiles[0].value, "12");
        assert_eq!(tiles[1].value, "3");
        assert_eq!(tiles[2].value, "98.8%");
        // The cumulative counter is not shown as a rate.
        assert_eq!(tiles[3].value, "14.2");
        assert_eq!(tiles[4].value, "2");
        assert_eq!(tiles[4].tone, StatusTone::Destructive);
    }

    // -- chart ------------------------------------------------------------

    #[test]
    fn the_axis_rounds_up_to_one_two_or_five() {
        assert_eq!(nice_ceiling(0.0), 1.0);
        assert_eq!(nice_ceiling(-4.0), 1.0);
        assert_eq!(nice_ceiling(f64::NAN), 1.0);
        assert_eq!(nice_ceiling(1.0), 1.0);
        assert_eq!(nice_ceiling(1.2), 2.0);
        assert_eq!(nice_ceiling(3.0), 5.0);
        assert_eq!(nice_ceiling(7.3), 10.0);
        assert_eq!(nice_ceiling(10.0), 10.0);
        assert_eq!(nice_ceiling(41.0), 50.0);
        assert_eq!(nice_ceiling(600.0), 1000.0);
    }

    #[test]
    fn axis_numbers_are_whole_when_whole_and_compact_when_large() {
        assert_eq!(format_axis_value(0.0), "0");
        assert_eq!(format_axis_value(5.0), "5");
        assert_eq!(format_axis_value(2.5), "2.5");
        assert_eq!(format_axis_value(150.0), "150");
        assert_eq!(format_axis_value(1_500.0), "1.5K");
        assert_eq!(format_axis_value(f64::NAN), "-");
    }

    #[test]
    fn a_path_runs_left_to_right_and_higher_values_are_higher_up() {
        let pts = [[0.0, 0.0], [5.0, 5.0], [10.0, 10.0]];
        let (line, area) = plot_path(&pts, 0.0, 10.0, 10.0);
        assert_eq!(line, "M 0.0 1000.0 L 500.0 500.0 L 1000.0 0.0");
        // The fill closes down to the baseline and back.
        assert_eq!(area, format!("{} L 1000.0 1000.0 L 0.0 1000.0 Z", line));
    }

    #[test]
    fn values_beyond_the_scale_are_clamped_inside_the_box() {
        let (line, _) = plot_path(&[[0.0, 50.0], [10.0, -5.0]], 0.0, 10.0, 10.0);
        assert_eq!(line, "M 0.0 0.0 L 1000.0 1000.0");
    }

    #[test]
    fn a_degenerate_time_span_does_not_divide_by_zero() {
        let (line, _) = plot_path(&[[3.0, 1.0], [3.0, 2.0]], 3.0, 3.0, 2.0);
        assert!(!line.contains("NaN") && !line.contains("inf"), "{line}");
        assert_eq!(
            plot_path(&[], 0.0, 1.0, 1.0),
            (String::new(), String::new())
        );
    }

    fn samples(n: i64) -> Vec<mon::MetricsSample> {
        (0..n)
            .map(|i| mon::MetricsSample {
                t_ms: i * 1_000,
                connections: (i % 7) as f64 + 3.0,
                active: (i % 3) as f64,
                tx_per_sec: i as f64 * 2.0,
            })
            .collect()
    }

    #[test]
    fn a_chart_needs_two_samples_and_scales_to_the_peak() {
        assert!(sessions_chart(&[]).is_none());
        assert!(sessions_chart(&samples(1)).is_none());

        let chart = sessions_chart(&samples(20)).unwrap();
        assert_eq!(chart.lines.len(), 2);
        assert_eq!(chart.lines[0].name, "Connections");
        assert_eq!(chart.lines[1].tone, StatusTone::Success);
        // Peak is 9 connections, so the scale tops out at 10.
        assert_eq!(chart.y_top, "10");
        assert_eq!(chart.y_mid, "5");
        assert!(chart.lines[0].commands.starts_with("M 0.0 "));
        assert!(chart.lines[0].area.ends_with('Z'));
        assert!(!chart.x_start.is_empty() && !chart.x_end.is_empty());
    }

    #[test]
    fn transactions_get_their_own_scale() {
        let sessions = sessions_chart(&samples(20)).unwrap();
        let tx = transactions_chart(&samples(20)).unwrap();
        assert_eq!(tx.lines.len(), 1);
        assert_ne!(sessions.y_top, tx.y_top);
        assert_eq!(tx.lines[0].last, "38");
    }

    // -- polling ----------------------------------------------------------

    const TEN: Option<Duration> = Some(Duration::from_secs(10));

    #[test]
    fn nothing_polls_while_hidden() {
        let mut gate = PollGate::default();
        let now = Instant::now();
        assert!(!gate.try_begin(TEN, 2, now), "never shown yet");
        gate.set_visible(true);
        assert!(gate.try_begin(TEN, 2, now));
        gate.finish_one();
        gate.finish_one();
        gate.set_visible(false);
        assert!(!gate.try_begin(TEN, 2, now + Duration::from_secs(60)));
    }

    #[test]
    fn a_poll_never_overlaps_the_one_in_flight() {
        let mut gate = PollGate::default();
        gate.set_visible(true);
        let t0 = Instant::now();
        assert!(gate.try_begin(TEN, 2, t0));
        // Long overdue, but two requests are still out.
        let late = t0 + Duration::from_secs(120);
        assert!(!gate.try_begin(TEN, 2, late));
        assert!(!gate.begin_manual(2, late), "a manual refresh waits too");
        gate.finish_one();
        assert!(gate.is_busy(), "one of two is still outstanding");
        assert!(!gate.try_begin(TEN, 2, late));
        gate.finish_one();
        assert!(!gate.is_busy());
        assert!(gate.try_begin(TEN, 2, late));
    }

    #[test]
    fn polls_are_spaced_by_the_interval() {
        let mut gate = PollGate::default();
        gate.set_visible(true);
        let t0 = Instant::now();
        assert!(gate.try_begin(TEN, 1, t0));
        gate.finish_one();
        assert!(!gate.try_begin(TEN, 1, t0 + Duration::from_secs(4)));
        assert!(gate.try_begin(TEN, 1, t0 + Duration::from_secs(10)));
    }

    #[test]
    fn switched_off_means_no_scheduled_polls_but_manual_ones_still_work() {
        let mut gate = PollGate::default();
        gate.set_visible(true);
        let now = Instant::now();
        assert!(!gate.try_begin(None, 1, now));
        assert!(gate.begin_manual(1, now));
    }

    #[test]
    fn reopening_catches_up_immediately() {
        let mut gate = PollGate::default();
        gate.set_visible(true);
        let t0 = Instant::now();
        assert!(gate.try_begin(TEN, 1, t0));
        gate.finish_one();
        gate.set_visible(false);
        gate.set_visible(true);
        assert!(gate.try_begin(TEN, 1, t0 + Duration::from_secs(1)));
    }

    #[test]
    fn abandoning_frees_the_gate_and_a_stray_finish_cannot_underflow() {
        let mut gate = PollGate::default();
        gate.set_visible(true);
        assert!(gate.try_begin(TEN, 2, Instant::now()));
        gate.abandon();
        assert!(!gate.is_busy());
        gate.finish_one();
        assert!(!gate.is_busy());
        assert!(
            !gate.try_begin(TEN, 0, Instant::now()),
            "zero requests is not a poll"
        );
    }

    // -- confirmations ----------------------------------------------------

    #[test]
    fn confirmations_name_what_they_destroy() {
        assert!(clear_logs_message(1).starts_with("1 query "));
        assert!(clear_logs_message(2_500).starts_with("2.5K queries "));
        let k = kill_message(42, Some("app"), Some("SELECT   pg_sleep(60)"));
        assert!(k.contains("process 42 (app)"));
        assert!(k.contains("SELECT pg_sleep(60)"));
        assert!(kill_message(7, None, None).contains("process 7."));
        assert!(!kill_message(7, Some(""), Some(" ")).contains("()"));
    }

    #[test]
    fn status_labels_are_distinct() {
        let labels: Vec<_> = mon::QUERY_STATUSES
            .iter()
            .map(|s| status_label(*s))
            .collect();
        for (i, a) in labels.iter().enumerate() {
            assert!(labels[i + 1..].iter().all(|b| b != a));
        }
        assert_eq!(status_label(QueryStatus::Failed), "Failed");
    }
}
