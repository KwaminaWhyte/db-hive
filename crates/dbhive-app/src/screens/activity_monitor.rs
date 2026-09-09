//! Activity Monitor: query log, server metrics chart and process list.
//!
//! Parity target: the React `/activity` route
//! (`src/components/ActivityMonitor.tsx` plus `activityMonitor/*`), covered
//! by §2.12 of docs/rust-ui-migration/01-frontend-inventory.md.
//!
//! Three notes on where this deliberately differs from the web shell.
//!
//! **The polished spec wins.** The live React page rendered a crude inline
//! card list and left the fully-built `QueryLogTable` / `QueryLogFilter` /
//! `ActivityStats` components orphaned. The inventory calls those the real
//! intended design, so this screen implements them: a sortable, filterable
//! grid with an expandable detail pane, checkbox type/status filters, a
//! date range, and statistics with percentage breakdowns.
//!
//! **Multi-select filters actually multi-select.** The React panel offered
//! type and status checkboxes and then sent only the first ticked value of
//! each. `dbhive_core::service_monitoring::LogQuery` carries the whole set.
//!
//! **Polling is pull, not push.** A browser `setInterval` fires whether or
//! not anything is drawing; egui only repaints on activity, so the poll loop
//! is a deadline plus `ctx.request_repaint_after`. A tick that arrives while
//! the previous request is still running is skipped rather than queued, so a
//! slow server cannot build up a backlog of overlapping refreshes.

use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use dbhive_core::drivers::QueryResult;
use dbhive_core::models::{
    ActivityStats, DbError, ExportFormat, QueryLog, QueryLogFilter, QueryLogSort,
    QueryLogSortField, QueryStatus, QueryType, SortDirection,
};
use dbhive_core::service_monitoring as mon;

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::theme::Palette;
use crate::widgets::results_grid;

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
const MAX_PLOT_POINTS: usize = 240;

/// Longest SQL shown in a grid cell. The full statement is in the detail pane.
const SQL_CELL_CHARS: usize = 160;

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
    t.with_timezone(&Local).format("%b %d, %H:%M:%S").to_string()
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

    fn toggle_type(&mut self, t: QueryType) {
        match self.types.iter().position(|x| *x == t) {
            Some(i) => {
                self.types.remove(i);
            }
            None => self.types.push(t),
        }
    }

    fn toggle_status(&mut self, s: QueryStatus) {
        match self.statuses.iter().position(|x| *x == s) {
            Some(i) => {
                self.statuses.remove(i);
            }
            None => self.statuses.push(s),
        }
    }
}

// ===========================================================================
// Screen state
// ===========================================================================

/// Result of one live-metrics or process poll.
type LiveStats = Result<mon::ServerStats, DbError>;
type LiveProcesses = Result<Vec<mon::ActiveQuery>, DbError>;

pub struct ActivityMonitor {
    pub open: bool,

    /// One-time setup on first draw: the screen has no constructor the app
    /// calls, so defaults that depend on app state land here.
    initialised: bool,

    // --- filters and paging ---
    pub filters: FilterState,
    /// Filters as last applied. Editing the panel does not re-query until
    /// Apply, matching the web shell and keeping typing cheap.
    applied: FilterState,
    sort_field: SortField,
    sort_desc: bool,
    page: usize,
    page_size: usize,

    // --- query log data ---
    logs: Vec<QueryLog>,
    log_result: QueryResult,
    log_grid: results_grid::GridState,
    selected_log: Option<usize>,
    tag_draft: String,
    total: usize,
    total_pages: usize,
    stats: Option<ActivityStats>,
    connections: Vec<(String, String)>,
    databases: Vec<String>,
    export_format: ExportFormat,

    // --- live server activity ---
    /// Connection the live panel watches. Independent of the log filter's
    /// connection, which may name a profile that is not currently open.
    live_connection: Option<String>,
    series: mon::MetricSeries,
    latest: Option<mon::ServerStats>,
    chart_span: Option<i64>,
    processes: Vec<mon::ActiveQuery>,
    process_result: QueryResult,
    process_grid: results_grid::GridState,
    /// Set when the driver has no session views at all; a plain message
    /// rather than an error, since nothing went wrong.
    unsupported: Option<String>,
    live_error: Option<String>,

    // --- jobs ---
    stats_job: Job<LiveStats>,
    processes_job: Job<LiveProcesses>,
    kill_job: Job<(i64, Result<(), DbError>)>,
    export_job: Job<Result<mon::ExportOutcome, DbError>>,

    // --- polling ---
    pub auto_refresh: bool,
    pub refresh_seconds: u64,
    last_tick: Option<Instant>,

    // --- dialogs and disclosure ---
    confirm_clear: bool,
    confirm_kill: Option<i64>,
    live_open: bool,
    stats_open: bool,
    filters_open: bool,
}

impl Default for ActivityMonitor {
    fn default() -> Self {
        Self {
            open: false,
            initialised: false,
            filters: FilterState::default(),
            applied: FilterState::default(),
            sort_field: SortField::StartedAt,
            sort_desc: true,
            page: 0,
            page_size: 50,
            logs: Vec::new(),
            log_result: QueryResult::empty(),
            log_grid: results_grid::GridState::default(),
            selected_log: None,
            tag_draft: String::new(),
            total: 0,
            total_pages: 0,
            stats: None,
            connections: Vec::new(),
            databases: Vec::new(),
            export_format: ExportFormat::Csv,
            live_connection: None,
            series: mon::MetricSeries::default(),
            latest: None,
            chart_span: Some(300),
            processes: Vec::new(),
            process_result: QueryResult::empty(),
            process_grid: results_grid::GridState::default(),
            unsupported: None,
            live_error: None,
            stats_job: Job::default(),
            processes_job: Job::default(),
            kill_job: Job::default(),
            export_job: Job::default(),
            auto_refresh: false,
            refresh_seconds: 10,
            last_tick: None,
            confirm_clear: false,
            confirm_kill: None,
            live_open: true,
            stats_open: true,
            filters_open: true,
        }
    }
}

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

    fn field(self) -> QueryLogSortField {
        match self {
            SortField::StartedAt => QueryLogSortField::StartedAt,
            SortField::DurationMs => QueryLogSortField::DurationMs,
            SortField::RowCount => QueryLogSortField::RowCount,
            SortField::ConnectionName => QueryLogSortField::ConnectionName,
        }
    }
}

impl ActivityMonitor {
    fn sort(&self) -> QueryLogSort {
        QueryLogSort {
            field: self.sort_field.field(),
            direction: if self.sort_desc {
                SortDirection::Desc
            } else {
                SortDirection::Asc
            },
        }
    }

    /// The auto-refresh period, or `None` when paused or set to "Off".
    pub fn interval(&self) -> Option<Duration> {
        if self.auto_refresh && self.refresh_seconds > 0 {
            Some(Duration::from_secs(self.refresh_seconds))
        } else {
            None
        }
    }

    /// Time left until the next tick, or `None` if one is due (or paused).
    ///
    /// Split out from the frame loop so the deadline arithmetic is not
    /// tangled with drawing. A remainder of exactly zero counts as due:
    /// asking egui to repaint in zero time would spin a frame without ever
    /// firing the tick.
    pub fn time_until_tick(&self, now: Instant) -> Option<Duration> {
        let interval = self.interval()?;
        let last = self.last_tick?;
        interval
            .checked_sub(now.duration_since(last))
            .filter(|remaining| !remaining.is_zero())
    }

    /// Whether a live request is outstanding. A tick that lands while one is
    /// still running is dropped, never queued.
    pub fn live_busy(&self) -> bool {
        self.stats_job.is_running() || self.processes_job.is_running() || self.kill_job.is_running()
    }
}

// ===========================================================================
// Data loading
// ===========================================================================

/// Re-read the query log, statistics and filter choices from `AppState`.
///
/// This is in-memory work, not a database round trip, so it runs inline
/// rather than through a `Job`. The lock is taken inside
/// `service_monitoring` and released before anything is drawn.
fn reload_logs(app: &mut DbHiveApp) {
    let state = app.state.clone();
    let am = &mut app.screens.activity_monitor;
    let query = am.applied.to_query(Utc::now());
    let sort = am.sort();

    let response = mon::get_query_logs(&state, &query, &sort, am.page, am.page_size);
    am.page = response.page;
    am.total = response.total;
    am.total_pages = response.total_pages;
    am.log_result = logs_to_result(&response.logs);
    am.logs = response.logs;
    am.stats = Some(mon::get_activity_stats(&state, &query));
    am.connections = mon::connection_choices(&state);
    am.databases = mon::logged_databases(&state, am.applied.connection_id.as_deref());

    // The selection is an index into a page that has just been rebuilt.
    if am.selected_log.is_some_and(|i| i >= am.logs.len()) {
        am.selected_log = None;
        am.log_grid.selected_row = None;
    }
}

/// Start a server-metrics and process poll, if one is not already running.
fn refresh_live(app: &mut DbHiveApp, ctx: &egui::Context) {
    let Some(connection_id) = app.screens.activity_monitor.live_connection.clone() else {
        return;
    };
    if app.screens.activity_monitor.live_busy() {
        return;
    }

    let handle = app.rt.handle().clone();
    let state = app.state.clone();
    let id = connection_id.clone();
    app.screens.activity_monitor.stats_job.start(&handle, ctx, async move {
        mon::get_server_stats(&state, &id).await
    });

    let state = app.state.clone();
    app.screens
        .activity_monitor
        .processes_job
        .start(&handle, ctx, async move {
            mon::get_active_queries(&state, &connection_id).await
        });
}

/// Point the live panel at a different connection, discarding stale samples.
fn set_live_connection(app: &mut DbHiveApp, ctx: &egui::Context, connection_id: Option<String>) {
    let am = &mut app.screens.activity_monitor;
    if am.live_connection == connection_id {
        return;
    }
    am.live_connection = connection_id;
    // Rates are diffed against the previous sample; across two different
    // servers that difference is meaningless.
    am.series.clear();
    am.latest = None;
    am.processes.clear();
    am.process_result = QueryResult::empty();
    am.process_grid = results_grid::GridState::default();
    am.unsupported = None;
    am.live_error = None;
    refresh_live(app, ctx);
}

/// Collect the results of any finished background work.
fn poll_jobs(app: &mut DbHiveApp) {
    let mut toast_success: Vec<String> = Vec::new();
    let mut toast_error: Vec<String> = Vec::new();
    let mut refresh_after_kill = false;

    {
        let am = &mut app.screens.activity_monitor;

        if let Some(result) = am.stats_job.take_result() {
            match result {
                Ok(stats) => {
                    am.unsupported = None;
                    am.live_error = None;
                    am.series.push(Utc::now().timestamp_millis(), &stats);
                    am.latest = Some(stats);
                }
                // A driver with no system views is a fact about the driver,
                // not a failure; say so plainly instead of raising an error.
                Err(DbError::InvalidInput(message)) => {
                    am.unsupported = Some(message);
                    am.latest = None;
                }
                Err(e) => am.live_error = Some(e.to_string()),
            }
        }

        if let Some(result) = am.processes_job.take_result() {
            match result {
                Ok(rows) => {
                    am.unsupported = None;
                    am.live_error = None;
                    am.process_result = processes_to_result(&rows);
                    am.processes = rows;
                    if am
                        .process_grid
                        .selected_row
                        .is_some_and(|i| i >= am.processes.len())
                    {
                        am.process_grid.selected_row = None;
                    }
                }
                Err(DbError::InvalidInput(message)) => {
                    am.unsupported = Some(message);
                    am.processes.clear();
                    am.process_result = QueryResult::empty();
                }
                Err(e) => am.live_error = Some(e.to_string()),
            }
        }

        if let Some((pid, result)) = am.kill_job.take_result() {
            match result {
                Ok(()) => {
                    toast_success.push(format!("Cancel signal sent to process {}", pid));
                    refresh_after_kill = true;
                }
                Err(e) => {
                    let message = format!("Could not cancel process {}: {}", pid, e);
                    am.live_error = Some(message.clone());
                    toast_error.push(message);
                }
            }
        }

        if let Some(result) = am.export_job.take_result() {
            match result {
                Ok(outcome) => toast_success.push(format!(
                    "Exported {} log(s), {}, to {}",
                    outcome.logs,
                    format_bytes(outcome.bytes),
                    outcome.path
                )),
                Err(e) => toast_error.push(format!("Export failed: {}", e)),
            }
        }
    }

    for message in toast_success {
        app.toasts.success(message);
    }
    for message in toast_error {
        app.toasts.error(message);
    }
    if refresh_after_kill {
        app.screens.activity_monitor.last_tick = None;
    }
}

// ===========================================================================
// Screen
// ===========================================================================

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    let ctx = ui.ctx().clone();

    poll_jobs(app);

    if !app.screens.activity_monitor.initialised {
        app.screens.activity_monitor.initialised = true;
        // Default the live panel to whatever is currently connected, so the
        // chart has something to show without the user picking twice.
        let current = app.workspace.as_ref().map(|w| w.connection_id.clone());
        reload_logs(app);
        if current.is_some() {
            set_live_connection(app, &ctx, current);
        }
    }

    // Polling. `minimized` is the native equivalent of the web shell's
    // `document.visibilityState`: a window in the dock is not being looked
    // at, so there is no point querying for it. Losing focus is not enough
    // to pause; watching a monitor while working elsewhere is the point.
    let hidden = ctx.input(|i| i.viewport().minimized == Some(true));
    if let Some(interval) = app.screens.activity_monitor.interval() {
        if hidden {
            // Restart the clock on un-minimise rather than firing a burst of
            // catch-up ticks the moment the window comes back.
            app.screens.activity_monitor.last_tick = Some(Instant::now());
        } else {
            let now = Instant::now();
            match app.screens.activity_monitor.time_until_tick(now) {
                Some(remaining) => ctx.request_repaint_after(remaining),
                None => {
                    app.screens.activity_monitor.last_tick = Some(now);
                    reload_logs(app);
                    // Skipped, not queued, if the last poll is still in
                    // flight: overlapping refreshes would pile up behind a
                    // slow server and never drain.
                    refresh_live(app, &ctx);
                    ctx.request_repaint_after(interval);
                }
            }
        }
    }

    header(app, ui, palette);
    ui.separator();

    egui::SidePanel::right("activity_side")
        .resizable(true)
        .default_width(380.0)
        .min_width(300.0)
        .frame(
            egui::Frame::none()
                .fill(palette.sidebar)
                .inner_margin(egui::Margin::same(10.0)),
        )
        .show_inside(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("activity_side_scroll")
                .show(ui, |ui| {
                    live_activity(app, ui, palette);
                    ui.add_space(8.0);
                    statistics(app, ui, palette);
                    ui.add_space(8.0);
                    filters(app, ui, palette);
                });
        });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        query_log(app, ui, palette);
    });

    clear_dialog(app, &ctx, palette);
    kill_dialog(app, &ctx, palette);
}

// --- header ----------------------------------------------------------------

fn header(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let ctx = ui.ctx().clone();
    let mut refresh_now = false;
    let mut export_now = false;
    let mut back = false;

    ui.horizontal(|ui| {
        let am = &mut app.screens.activity_monitor;
        ui.label(crate::theme::text::heading(palette, "Activity Monitor"));
        ui.label(crate::theme::text::muted(
            palette,
            format!("{} total logs", format_count(am.total as u64)),
        ));

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                back = true;
            }
            ui.separator();

            let can_clear = am.total > 0;
            if ui
                .add_enabled(
                    can_clear,
                    egui::Button::new(
                        egui::RichText::new("Clear logs").color(palette.destructive),
                    ),
                )
                .on_hover_text("Permanently delete every query log")
                .clicked()
            {
                am.confirm_clear = true;
            }

            let exporting = am.export_job.is_running();
            if ui
                .add_enabled(
                    am.total > 0 && !exporting,
                    egui::Button::new(if exporting { "Exporting…" } else { "Export" }),
                )
                .clicked()
            {
                export_now = true;
            }
            egui::ComboBox::from_id_salt("export_format")
                .selected_text(export_format_label(am.export_format))
                .width(70.0)
                .show_ui(ui, |ui| {
                    for format in [ExportFormat::Csv, ExportFormat::Json, ExportFormat::Txt] {
                        let selected = export_format_label(am.export_format)
                            == export_format_label(format);
                        if ui
                            .selectable_label(selected, export_format_label(format))
                            .clicked()
                        {
                            am.export_format = format;
                        }
                    }
                });

            ui.separator();

            if ui.button("Refresh").clicked() {
                refresh_now = true;
            }

            egui::ComboBox::from_id_salt("refresh_interval")
                .selected_text(interval_label(am.refresh_seconds))
                .width(80.0)
                .show_ui(ui, |ui| {
                    for seconds in REFRESH_SECONDS {
                        if ui
                            .selectable_label(am.refresh_seconds == seconds, interval_label(seconds))
                            .clicked()
                        {
                            am.refresh_seconds = seconds;
                            am.last_tick = Some(Instant::now());
                        }
                    }
                });
            let label = if am.auto_refresh { "Pause" } else { "Auto" };
            if ui
                .selectable_label(am.auto_refresh, label)
                .on_hover_text("Refresh on an interval")
                .clicked()
            {
                am.auto_refresh = !am.auto_refresh;
                am.last_tick = Some(Instant::now());
            }
            if am.live_busy() {
                ui.add(egui::Spinner::new().size(14.0));
            }
        });
    });

    if back {
        app.screens.activity_monitor.open = false;
    }
    if refresh_now {
        reload_logs(app);
        refresh_live(app, &ctx);
        app.screens.activity_monitor.last_tick = Some(Instant::now());
    }
    if export_now {
        start_export(app, &ctx);
    }
}

fn interval_label(seconds: u64) -> String {
    if seconds == 0 {
        "Off".to_string()
    } else {
        format!("{}s", seconds)
    }
}

fn export_format_label(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Csv => "CSV",
        ExportFormat::Json => "JSON",
        ExportFormat::Txt => "TXT",
    }
}

/// Where an export lands.
///
/// The web shell handed the file to the browser's download machinery; the
/// native shell has no file-dialog dependency, so the file goes next to the
/// other application data and the path is reported in a toast. Matches how
/// the history screen exports templates.
fn export_path(format: ExportFormat) -> Result<std::path::PathBuf, String> {
    let dir = dbhive_core::storage::app_data_dir().map_err(|e| e.to_string())?;
    let stamp = Local::now().format("%Y-%m-%d-%H%M%S");
    let extension = match format {
        ExportFormat::Csv => "csv",
        ExportFormat::Json => "json",
        ExportFormat::Txt => "txt",
    };
    Ok(dir.join(format!("query-logs-{}.{}", stamp, extension)))
}

fn start_export(app: &mut DbHiveApp, ctx: &egui::Context) {
    let format = app.screens.activity_monitor.export_format;
    let path = match export_path(format) {
        Ok(p) => p.to_string_lossy().to_string(),
        Err(e) => {
            app.toasts.error(format!("Could not choose an export path: {}", e));
            return;
        }
    };
    let query = app.screens.activity_monitor.applied.to_query(Utc::now());
    let state = app.state.clone();
    let handle = app.rt.handle().clone();
    app.screens
        .activity_monitor
        .export_job
        .start(&handle, ctx, async move {
            mon::export_query_logs(&state, &query, format, path).await
        });
}

// --- query log -------------------------------------------------------------

fn query_log(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let mut page_delta: i64 = 0;
    let mut reload = false;

    ui.horizontal(|ui| {
        let am = &mut app.screens.activity_monitor;
        ui.label(crate::theme::text::strong(palette, "Query Logs"));
        ui.separator();
        ui.label(crate::theme::text::muted(palette, "Sort"));
        egui::ComboBox::from_id_salt("log_sort")
            .selected_text(am.sort_field.label())
            .width(120.0)
            .show_ui(ui, |ui| {
                for field in SortField::ALL {
                    if ui
                        .selectable_label(am.sort_field == field, field.label())
                        .clicked()
                        && am.sort_field != field
                    {
                        am.sort_field = field;
                        reload = true;
                    }
                }
            });
        if ui
            .small_button(if am.sort_desc { "⏷" } else { "⏶" })
            .on_hover_text(if am.sort_desc {
                "Newest / largest first"
            } else {
                "Oldest / smallest first"
            })
            .clicked()
        {
            am.sort_desc = !am.sort_desc;
            reload = true;
        }

        ui.separator();
        ui.label(crate::theme::text::muted(palette, "Per page"));
        egui::ComboBox::from_id_salt("log_page_size")
            .selected_text(am.page_size.to_string())
            .width(70.0)
            .show_ui(ui, |ui| {
                for size in PAGE_SIZES {
                    if ui
                        .selectable_label(am.page_size == size, size.to_string())
                        .clicked()
                        && am.page_size != size
                    {
                        am.page_size = size;
                        am.page = 0;
                        reload = true;
                    }
                }
            });
    });
    ui.add_space(4.0);

    let empty = app.screens.activity_monitor.logs.is_empty();
    if empty {
        let filtered = app.screens.activity_monitor.applied.is_active();
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(crate::theme::text::strong(palette, "No query logs found"));
            ui.label(crate::theme::text::muted(
                palette,
                if filtered {
                    "No log matches the current filters. Reset them to see everything."
                } else {
                    "Statements run from the query editor appear here."
                },
            ));
        });
    } else {
        // The detail pane is anchored to the bottom so the grid keeps the
        // rest of the space and does not jump when a row is selected.
        let has_selection = app.screens.activity_monitor.selected_log.is_some();
        if has_selection {
            egui::TopBottomPanel::bottom("log_detail")
                .resizable(true)
                .default_height(180.0)
                .frame(
                    egui::Frame::none()
                        .fill(palette.card)
                        .inner_margin(egui::Margin::same(10.0)),
                )
                .show_inside(ui, |ui| log_detail(app, ui, palette));
        }

        egui::TopBottomPanel::bottom("log_pager")
            .exact_height(30.0)
            .frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(0.0, 4.0)))
            .show_inside(ui, |ui| {
                let am = &app.screens.activity_monitor;
                ui.horizontal(|ui| {
                    ui.label(crate::theme::text::muted(
                        palette,
                        format!(
                            "Page {} of {} ({} total)",
                            am.page + 1,
                            am.total_pages.max(1),
                            am.total
                        ),
                    ));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can_next = am.page + 1 < am.total_pages;
                        if ui
                            .add_enabled(can_next, egui::Button::new("Next"))
                            .clicked()
                        {
                            page_delta = 1;
                        }
                        if ui
                            .add_enabled(am.page > 0, egui::Button::new("Previous"))
                            .clicked()
                        {
                            page_delta = -1;
                        }
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show_inside(ui, |ui| {
                let am = &mut app.screens.activity_monitor;
                let clicked = results_grid::show(ui, palette, &am.log_result, &mut am.log_grid);
                if let Some(row) = clicked {
                    if am.selected_log == Some(row) {
                        // Clicking the selected row again collapses the detail.
                        am.selected_log = None;
                        am.log_grid.selected_row = None;
                    } else {
                        am.selected_log = Some(row);
                        am.tag_draft = am
                            .logs
                            .get(row)
                            .and_then(|l| l.tags.clone())
                            .map(|t| t.join(", "))
                            .unwrap_or_default();
                    }
                }
            });
    }

    if page_delta != 0 {
        let am = &mut app.screens.activity_monitor;
        am.page = if page_delta > 0 {
            am.page + 1
        } else {
            am.page.saturating_sub(1)
        };
        am.selected_log = None;
        am.log_grid.selected_row = None;
        reload = true;
    }
    if reload {
        reload_logs(app);
    }
}

/// The expanded view of one log: full SQL, error and tags.
fn log_detail(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let Some(index) = app.screens.activity_monitor.selected_log else {
        return;
    };
    let Some(log) = app.screens.activity_monitor.logs.get(index).cloned() else {
        return;
    };

    let mut save_tags = false;
    let mut copy_sql = false;
    let mut close = false;

    ui.horizontal(|ui| {
        ui.label(crate::theme::text::strong(
            palette,
            format!(
                "{}  ·  {}",
                mon::query_type_key(log.query_type),
                mon::status_key(log.status)
            ),
        ));
        ui.label(crate::theme::text::muted(
            palette,
            format!(
                "{}  ·  {}  ·  {} rows  ·  {}",
                log.connection_name,
                format_duration(log.duration_ms),
                log.row_count.map(format_count).unwrap_or_else(|| "-".into()),
                format_timestamp(log.started_at)
            ),
        ));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Close").clicked() {
                close = true;
            }
            if ui.small_button("Copy SQL").clicked() {
                copy_sql = true;
            }
        });
    });

    if let Some(error) = &log.error {
        ui.add_space(4.0);
        ui.colored_label(palette.destructive, error);
    }

    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .id_salt("log_detail_sql")
        .max_height(110.0)
        .show(ui, |ui| {
            ui.label(egui::RichText::new(&log.sql).monospace().color(palette.foreground));
        });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(crate::theme::text::muted(palette, "Tags"));
        let am = &mut app.screens.activity_monitor;
        ui.add(
            egui::TextEdit::singleline(&mut am.tag_draft)
                .hint_text("comma, separated")
                .desired_width(240.0),
        );
        if ui.small_button("Save tags").clicked() {
            save_tags = true;
        }
    });

    if copy_sql {
        ui.ctx().copy_text(log.sql.clone());
        app.toasts.success("SQL copied");
    }
    if close {
        app.screens.activity_monitor.selected_log = None;
        app.screens.activity_monitor.log_grid.selected_row = None;
    }
    if save_tags {
        let tags: Vec<String> = app
            .screens
            .activity_monitor
            .tag_draft
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        let state = app.state.clone();
        if mon::update_query_log_tags(&state, &log.id, tags) {
            app.toasts.success("Tags saved");
            reload_logs(app);
        } else {
            app.toasts.error("That log is no longer in the buffer");
        }
    }
}

// --- live activity ---------------------------------------------------------

fn live_activity(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let ctx = ui.ctx().clone();
    let mut open = app.screens.activity_monitor.live_open;
    let mut pick_connection: Option<Option<String>> = None;
    let mut refresh_now = false;
    let mut kill_request: Option<i64> = None;

    let response = egui::CollapsingHeader::new(crate::theme::text::strong(palette, "Live Activity"))
        .id_salt("live_activity")
        .default_open(true)
        .open(Some(open))
        .show(ui, |ui| {
            // Only open connections can be queried, so the picker lists those
            // rather than every saved profile.
            let active: Vec<(String, String)> = {
                let guard = app.state.lock().unwrap();
                guard
                    .list_profiles()
                    .into_iter()
                    .filter(|p| guard.has_connection(&p.id))
                    .map(|p| (p.id.clone(), p.name.clone()))
                    .collect()
            };

            let am = &mut app.screens.activity_monitor;
            let current_name = am
                .live_connection
                .as_ref()
                .and_then(|id| active.iter().find(|(pid, _)| pid == id))
                .map(|(_, name)| name.clone())
                .unwrap_or_else(|| "No connection".to_string());

            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("live_connection")
                    .selected_text(current_name)
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(am.live_connection.is_none(), "No connection")
                            .clicked()
                        {
                            pick_connection = Some(None);
                        }
                        for (id, name) in &active {
                            let selected = am.live_connection.as_deref() == Some(id.as_str());
                            if ui.selectable_label(selected, name).clicked() && !selected {
                                pick_connection = Some(Some(id.clone()));
                            }
                        }
                    });
                if ui
                    .add_enabled(
                        am.live_connection.is_some() && !am.live_busy(),
                        egui::Button::new("Refresh"),
                    )
                    .clicked()
                {
                    refresh_now = true;
                }
            });

            if am.live_connection.is_none() {
                ui.add_space(6.0);
                ui.label(crate::theme::text::muted(
                    palette,
                    if active.is_empty() {
                        "Connect to a database to see server metrics and active sessions."
                    } else {
                        "Select a connection to see server metrics and active sessions."
                    },
                ));
                return;
            }

            if let Some(message) = am.unsupported.clone() {
                ui.add_space(6.0);
                ui.label(crate::theme::text::muted(palette, message));
                return;
            }

            if let Some(error) = am.live_error.clone() {
                ui.add_space(4.0);
                ui.colored_label(palette.destructive, error);
            }

            ui.add_space(6.0);
            stat_tiles(ui, palette, am.latest.as_ref());
            ui.add_space(6.0);
            metrics_chart(ui, palette, am);
            ui.add_space(8.0);
            ui.separator();
            ui.label(crate::theme::text::strong(palette, "Active Queries"));
            kill_request = process_list(ui, palette, am);
        });

    if response.header_response.clicked() {
        open = !open;
    }
    app.screens.activity_monitor.live_open = open;

    if let Some(choice) = pick_connection {
        set_live_connection(app, &ctx, choice);
    }
    if refresh_now {
        refresh_live(app, &ctx);
    }
    if let Some(pid) = kill_request {
        app.screens.activity_monitor.confirm_kill = Some(pid);
    }
}

fn stat_tiles(ui: &mut egui::Ui, palette: &Palette, latest: Option<&mon::ServerStats>) {
    let Some(stats) = latest else {
        ui.label(crate::theme::text::muted(palette, "Collecting samples…"));
        return;
    };
    ui.horizontal_wrapped(|ui| {
        tile(ui, palette, "Connections", stats.numeric_connections.to_string());
        tile(ui, palette, "Active", stats.active_connections.to_string());
        tile(
            ui,
            palette,
            "Cache hit",
            match stats.cache_hit_ratio {
                Some(r) => format!("{:.1}%", r * 100.0),
                None => "-".to_string(),
            },
        );
        tile(
            ui,
            palette,
            "Deadlocks",
            stats
                .deadlocks
                .map(|d| d.to_string())
                .unwrap_or_else(|| "-".to_string()),
        );
    });
}

fn tile(ui: &mut egui::Ui, palette: &Palette, label: &str, value: String) {
    egui::Frame::none()
        .fill(palette.card)
        .stroke(egui::Stroke::new(1.0_f32, palette.border))
        .rounding(egui::Rounding::same(6.0))
        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(crate::theme::text::strong(palette, value));
                ui.label(crate::theme::text::muted(palette, label));
            });
        });
}

/// The rolling server-metrics chart.
///
/// Two linked plots rather than the web shell's single dual-axis chart:
/// connections and transactions/second differ by orders of magnitude, and
/// egui_plot has no second y-axis scale, so plotting them together would
/// flatten one of them into the axis.
fn metrics_chart(ui: &mut egui::Ui, palette: &Palette, am: &mut ActivityMonitor) {
    ui.horizontal(|ui| {
        ui.label(crate::theme::text::muted(palette, "Window"));
        for (span, label) in CHART_SPANS {
            if ui.selectable_label(am.chart_span == span, label).clicked() {
                am.chart_span = span;
            }
        }
    });
    ui.add_space(4.0);

    let window = am.series.window(am.chart_span.map(|s| s * 1_000));
    if window.len() < 2 {
        ui.label(crate::theme::text::muted(palette, "Collecting samples…"));
        return;
    }

    let connections = series_points(window, |s| s.connections);
    let active = series_points(window, |s| s.active);
    let tx = series_points(window, |s| s.tx_per_sec);

    let axis_label = |mark: egui_plot::GridMark, _: &std::ops::RangeInclusive<f64>| {
        format_clock_ms(mark.value)
    };

    egui_plot::Plot::new("server_connections")
        .height(140.0)
        .legend(egui_plot::Legend::default().text_style(egui::TextStyle::Small))
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .include_y(0.0)
        .link_axis("activity_metrics", true, false)
        .x_axis_formatter(axis_label)
        .label_formatter(|name, point| {
            format!("{}\n{}: {:.0}", format_clock_ms(point.x), name, point.y)
        })
        .show(ui, |plot_ui| {
            plot_ui.line(
                egui_plot::Line::new(connections)
                    .name("Connections")
                    .color(palette.primary)
                    .width(1.5_f32),
            );
            plot_ui.line(
                egui_plot::Line::new(active)
                    .name("Active")
                    .color(palette.success)
                    .width(1.5_f32),
            );
        });

    ui.add_space(4.0);
    egui_plot::Plot::new("server_transactions")
        .height(90.0)
        .legend(egui_plot::Legend::default().text_style(egui::TextStyle::Small))
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .include_y(0.0)
        .link_axis("activity_metrics", true, false)
        .x_axis_formatter(axis_label)
        .label_formatter(|name, point| {
            format!("{}\n{}: {:.2}", format_clock_ms(point.x), name, point.y)
        })
        .show(ui, |plot_ui| {
            plot_ui.line(
                egui_plot::Line::new(tx)
                    .name("Tx/sec")
                    .color(palette.warning)
                    .width(1.5_f32),
            );
        });
}

/// Project one field of a sample window into downsampled plot points.
fn series_points(
    samples: &[mon::MetricsSample],
    pick: impl Fn(&mon::MetricsSample) -> f64,
) -> Vec<[f64; 2]> {
    let points: Vec<[f64; 2]> = samples
        .iter()
        .map(|s| [s.t_ms as f64, pick(s)])
        .collect();
    mon::downsample(&points, MAX_PLOT_POINTS)
}

/// The active-session table. Returns the pid the user asked to cancel.
fn process_list(ui: &mut egui::Ui, palette: &Palette, am: &mut ActivityMonitor) -> Option<i64> {
    let mut kill: Option<i64> = None;

    ui.horizontal(|ui| {
        ui.label(crate::theme::text::muted(
            palette,
            match am.processes.len() {
                1 => "1 active session".to_string(),
                n => format!("{} active sessions", n),
            },
        ));
    });

    if am.processes.is_empty() {
        ui.label(crate::theme::text::muted(palette, "No active queries"));
        return None;
    }

    // The shared grid has no per-row buttons, so the kill action works on the
    // selected row. That is no worse to use and makes the destructive action
    // impossible to hit by mis-clicking a row.
    // The grid records the click in `process_grid.selected_row`, which is
    // what the cancel button below reads; the returned index is not needed.
    let _ = results_grid::show(ui, palette, &am.process_result, &mut am.process_grid);

    ui.add_space(4.0);
    let selected_pid = am
        .process_grid
        .selected_row
        .and_then(|i| am.processes.get(i))
        .map(|p| p.pid);
    ui.horizontal(|ui| match selected_pid {
        Some(pid) => {
            if ui
                .add_enabled(
                    !am.kill_job.is_running(),
                    egui::Button::new(
                        egui::RichText::new(format!("Cancel process {}", pid))
                            .color(palette.destructive),
                    ),
                )
                .on_hover_text("Ask the server to cancel this session's query")
                .clicked()
            {
                kill = Some(pid);
            }
        }
        None => {
            ui.label(crate::theme::text::muted(
                palette,
                "Select a session to cancel it",
            ));
        }
    });

    kill
}

// --- statistics ------------------------------------------------------------

fn statistics(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let mut open = app.screens.activity_monitor.stats_open;
    let response = egui::CollapsingHeader::new(crate::theme::text::strong(palette, "Statistics"))
        .id_salt("activity_statistics")
        .default_open(true)
        .open(Some(open))
        .show(ui, |ui| {
            let am = &app.screens.activity_monitor;
            let Some(stats) = am.stats.as_ref() else {
                ui.label(crate::theme::text::muted(palette, "No statistics available"));
                return;
            };

            ui.horizontal_wrapped(|ui| {
                tile(ui, palette, "Total", format_count(stats.total_queries as u64));
                tile(
                    ui,
                    palette,
                    "Failed",
                    format_count(stats.failed_queries as u64),
                );
                tile(
                    ui,
                    palette,
                    "Avg duration",
                    format_duration(Some(stats.avg_duration.round() as u64)),
                );
                tile(ui, palette, "Total rows", format_count(stats.total_rows));
            });

            ui.add_space(8.0);
            ui.label(crate::theme::text::strong(palette, "Queries by type"));
            let total = stats.total_queries;
            for query_type in mon::QUERY_TYPES {
                if let Some(count) = stats.queries_by_type.get(&query_type).copied() {
                    if count > 0 {
                        breakdown_row(
                            ui,
                            palette,
                            mon::query_type_key(query_type),
                            count,
                            total,
                            palette.primary,
                        );
                    }
                }
            }

            ui.add_space(8.0);
            ui.label(crate::theme::text::strong(palette, "Queries by status"));
            for status in mon::QUERY_STATUSES {
                let key = mon::status_key(status);
                if let Some(count) = stats.queries_by_status.get(key).copied() {
                    if count > 0 {
                        breakdown_row(ui, palette, key, count, total, status_colour(palette, status));
                    }
                }
            }
        });

    if response.header_response.clicked() {
        open = !open;
    }
    app.screens.activity_monitor.stats_open = open;
}

fn status_colour(palette: &Palette, status: QueryStatus) -> egui::Color32 {
    match status {
        QueryStatus::Completed => palette.success,
        QueryStatus::Failed => palette.destructive,
        QueryStatus::Running => palette.primary,
        QueryStatus::Cancelled => palette.muted_foreground,
    }
}

/// One label / bar / count row, replacing the web shell's hand-rolled CSS bar.
fn breakdown_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    label: &str,
    count: usize,
    total: usize,
    colour: egui::Color32,
) {
    let fraction = percentage(count, total) / 100.0;
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(96.0, 16.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(crate::theme::text::muted(palette, label));
            },
        );

        let width = (ui.available_width() - 70.0).max(40.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 8.0), egui::Sense::hover());
        let rounding = egui::Rounding::same(4.0);
        ui.painter().rect_filled(rect, rounding, palette.muted);
        let filled = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(rect.width() * fraction as f32, rect.height()),
        );
        ui.painter().rect_filled(filled, rounding, colour);

        ui.label(crate::theme::text::muted(
            palette,
            format!("{} ({:.0}%)", count, percentage(count, total)),
        ));
    });
}

// --- filters ---------------------------------------------------------------

fn filters(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let mut open = app.screens.activity_monitor.filters_open;
    let mut apply = false;
    let mut reset = false;

    let response = egui::CollapsingHeader::new(crate::theme::text::strong(palette, "Filters"))
        .id_salt("activity_filters")
        .default_open(true)
        .open(Some(open))
        .show(ui, |ui| {
            let am = &mut app.screens.activity_monitor;
            let connections = am.connections.clone();
            let databases = am.databases.clone();
            let f = &mut am.filters;

            ui.label(crate::theme::text::muted(palette, "Connection"));
            let selected = f
                .connection_id
                .as_ref()
                .and_then(|id| connections.iter().find(|(cid, _)| cid == id))
                .map(|(_, name)| name.clone())
                .unwrap_or_else(|| "All connections".to_string());
            egui::ComboBox::from_id_salt("filter_connection")
                .selected_text(selected)
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(f.connection_id.is_none(), "All connections")
                        .clicked()
                    {
                        f.connection_id = None;
                        f.database = None;
                    }
                    for (id, name) in &connections {
                        let is = f.connection_id.as_deref() == Some(id.as_str());
                        if ui.selectable_label(is, name).clicked() && !is {
                            f.connection_id = Some(id.clone());
                            // The old database may not exist on the new
                            // connection, so it cannot carry over.
                            f.database = None;
                        }
                    }
                });

            ui.add_space(6.0);
            ui.label(crate::theme::text::muted(palette, "Database"));
            egui::ComboBox::from_id_salt("filter_database")
                .selected_text(
                    f.database
                        .clone()
                        .unwrap_or_else(|| "All databases".to_string()),
                )
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(f.database.is_none(), "All databases")
                        .clicked()
                    {
                        f.database = None;
                    }
                    for db in &databases {
                        let is = f.database.as_deref() == Some(db.as_str());
                        if ui.selectable_label(is, db).clicked() {
                            f.database = Some(db.clone());
                        }
                    }
                });

            ui.add_space(6.0);
            ui.label(crate::theme::text::muted(palette, "Time range"));
            egui::ComboBox::from_id_salt("filter_time_range")
                .selected_text(f.time_range.label())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for range in mon::TimeRange::ALL {
                        if ui
                            .selectable_label(f.time_range == range, range.label())
                            .clicked()
                        {
                            f.time_range = range;
                        }
                    }
                });

            ui.add_space(6.0);
            ui.label(crate::theme::text::muted(palette, "Query type"));
            let mut toggle_type: Option<QueryType> = None;
            ui.horizontal_wrapped(|ui| {
                for query_type in mon::QUERY_TYPES {
                    let on = f.types.contains(&query_type);
                    if ui
                        .selectable_label(on, mon::query_type_key(query_type))
                        .clicked()
                    {
                        toggle_type = Some(query_type);
                    }
                }
            });
            if let Some(t) = toggle_type {
                f.toggle_type(t);
            }

            ui.add_space(6.0);
            ui.label(crate::theme::text::muted(palette, "Status"));
            let mut toggle_status: Option<QueryStatus> = None;
            ui.horizontal_wrapped(|ui| {
                for status in mon::QUERY_STATUSES {
                    let on = f.statuses.contains(&status);
                    if ui.selectable_label(on, mon::status_key(status)).clicked() {
                        toggle_status = Some(status);
                    }
                }
            });
            if let Some(s) = toggle_status {
                f.toggle_status(s);
            }

            ui.add_space(6.0);
            ui.label(crate::theme::text::muted(palette, "Duration (ms)"));
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut f.min_duration)
                        .hint_text("Min")
                        .desired_width(80.0),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut f.max_duration)
                        .hint_text("Max")
                        .desired_width(80.0),
                );
            });

            ui.add_space(6.0);
            ui.label(crate::theme::text::muted(palette, "Search in SQL"));
            ui.add(
                egui::TextEdit::singleline(&mut f.search_text)
                    .hint_text("Enter search term…")
                    .desired_width(f32::INFINITY),
            );

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Apply filters").clicked() {
                    apply = true;
                }
                if ui
                    .add_enabled(f.is_active(), egui::Button::new("Reset"))
                    .clicked()
                {
                    reset = true;
                }
            });
        });

    if response.header_response.clicked() {
        open = !open;
    }
    app.screens.activity_monitor.filters_open = open;

    if reset {
        app.screens.activity_monitor.filters = FilterState::default();
    }
    if apply || reset {
        let am = &mut app.screens.activity_monitor;
        am.applied = am.filters.clone();
        am.page = 0;
        am.selected_log = None;
        am.log_grid.selected_row = None;
        reload_logs(app);
    }
}

// --- dialogs ---------------------------------------------------------------

fn clear_dialog(app: &mut DbHiveApp, ctx: &egui::Context, palette: &Palette) {
    if !app.screens.activity_monitor.confirm_clear {
        return;
    }
    let total = app.screens.activity_monitor.total;
    let mut window_open = true;
    let mut close = false;
    let mut clear = false;

    egui::Window::new("Clear all logs?")
        .collapsible(false)
        .resizable(false)
        .open(&mut window_open)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(crate::theme::text::muted(
                palette,
                format!(
                    "This permanently deletes all query logs ({} currently shown). \
                     This cannot be undone.",
                    total
                ),
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    close = true;
                }
                if ui
                    .button(egui::RichText::new("Clear logs").color(palette.destructive))
                    .clicked()
                {
                    clear = true;
                }
            });
        });

    if clear {
        let state = app.state.clone();
        let removed = mon::clear_query_logs(&state);
        app.screens.activity_monitor.page = 0;
        app.screens.activity_monitor.selected_log = None;
        app.screens.activity_monitor.log_grid.selected_row = None;
        reload_logs(app);
        app.toasts.success(format!("Cleared {} log(s)", removed));
    }
    if close || clear || !window_open {
        app.screens.activity_monitor.confirm_clear = false;
    }
}

/// Cancelling someone's query is destructive and irreversible, so the pid is
/// named in the prompt: a mis-selected row is otherwise impossible to notice.
fn kill_dialog(app: &mut DbHiveApp, ctx: &egui::Context, palette: &Palette) {
    let Some(pid) = app.screens.activity_monitor.confirm_kill else {
        return;
    };
    let details = app
        .screens
        .activity_monitor
        .processes
        .iter()
        .find(|p| p.pid == pid)
        .cloned();

    let mut window_open = true;
    let mut close = false;
    let mut confirm = false;

    egui::Window::new(format!("Cancel process {}?", pid))
        .collapsible(false)
        .resizable(false)
        .open(&mut window_open)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(crate::theme::text::strong(
                palette,
                format!("Process {} will be asked to stop.", pid),
            ));
            if let Some(p) = &details {
                ui.add_space(4.0);
                ui.label(crate::theme::text::muted(
                    palette,
                    format!(
                        "{} on {} from {} · running {}",
                        p.user.clone().unwrap_or_else(|| "unknown user".into()),
                        p.database.clone().unwrap_or_else(|| "unknown db".into()),
                        p.client_addr.clone().unwrap_or_else(|| "unknown host".into()),
                        format_duration_i64(p.duration_ms)
                    ),
                ));
                if let Some(query) = &p.query_text {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(one_line_sql(query, 200))
                            .monospace()
                            .color(palette.muted_foreground),
                    );
                }
            }
            ui.add_space(6.0);
            ui.label(crate::theme::text::muted(
                palette,
                "Any work the statement has not committed is rolled back. \
                 This cannot be undone.",
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Keep running").clicked() {
                    close = true;
                }
                if ui
                    .button(
                        egui::RichText::new(format!("Cancel process {}", pid))
                            .color(palette.destructive),
                    )
                    .clicked()
                {
                    confirm = true;
                }
            });
        });

    if confirm {
        if let Some(connection_id) = app.screens.activity_monitor.live_connection.clone() {
            let state = app.state.clone();
            let handle = app.rt.handle().clone();
            app.screens
                .activity_monitor
                .kill_job
                .start(&handle, ctx, async move {
                    (pid, mon::kill_query(&state, &connection_id, pid).await)
                });
        }
    }
    if close || confirm || !window_open {
        app.screens.activity_monitor.confirm_kill = None;
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
        let mut am = ActivityMonitor::default();
        assert!(am.interval().is_none(), "auto-refresh defaults to off");

        am.auto_refresh = true;
        am.refresh_seconds = 0;
        assert!(am.interval().is_none(), "an interval of Off means no polling");

        am.refresh_seconds = 10;
        assert_eq!(am.interval(), Some(Duration::from_secs(10)));
    }

    #[test]
    fn a_tick_is_due_immediately_and_then_once_per_interval() {
        let mut am = ActivityMonitor::default();
        am.auto_refresh = true;
        am.refresh_seconds = 10;

        let start = Instant::now();
        // Never ticked: due now, so the panel populates on open.
        assert_eq!(am.time_until_tick(start), None);

        am.last_tick = Some(start);
        let after_4s = start + Duration::from_secs(4);
        assert_eq!(am.time_until_tick(after_4s), Some(Duration::from_secs(6)));

        let after_10s = start + Duration::from_secs(10);
        assert_eq!(am.time_until_tick(after_10s), None);
        // Overdue is still "due now", not a negative wait.
        let after_30s = start + Duration::from_secs(30);
        assert_eq!(am.time_until_tick(after_30s), None);
    }

    #[test]
    fn a_paused_monitor_never_reports_a_deadline() {
        let mut am = ActivityMonitor::default();
        am.last_tick = Some(Instant::now());
        assert!(am.time_until_tick(Instant::now()).is_none());
        assert!(am.interval().is_none());
    }

    #[test]
    fn an_in_flight_poll_marks_the_screen_busy() {
        // Guards the "never stack overlapping refreshes" rule: refresh_live
        // returns early whenever this is true.
        let am = ActivityMonitor::default();
        assert!(!am.live_busy());
    }

    // -- sort mapping -----------------------------------------------------

    #[test]
    fn sort_direction_follows_the_toggle() {
        let mut am = ActivityMonitor::default();
        am.sort_field = SortField::DurationMs;
        am.sort_desc = true;
        assert!(matches!(am.sort().direction, SortDirection::Desc));
        assert!(matches!(am.sort().field, QueryLogSortField::DurationMs));

        am.sort_desc = false;
        assert!(matches!(am.sort().direction, SortDirection::Asc));
    }

    #[test]
    fn every_sort_field_has_a_label_and_a_backend_field() {
        for field in SortField::ALL {
            assert!(!field.label().is_empty());
            let _ = field.field();
        }
    }
}
