//! Controller glue for the Activity Monitor.
//!
//! A child module of `app` so it can use the controller's private pieces
//! (`Ctx`, `Session`, `with_ctx`) without widening their visibility. The
//! panel's logic lives in `dbhive_ui_model::activity_panel` and
//! `activity_monitor`, and the data operations in
//! `dbhive_core::service_monitoring`; this file moves values between those,
//! the stores in `AppState`, and the `ActivityState` global in the window.
//!
//! Polling rules, enforced by `PollGate` and tested there:
//!
//! * a poll never starts while the previous one is still in flight;
//! * nothing starts while the panel is closed, or the window is hidden or
//!   minimised (the Slint timer keeps ticking, but a tick that finds the
//!   gate shut does nothing);
//! * results that belong to a connection the user has since switched away
//!   from are discarded by ticket, so a slow reply cannot paint over a newer
//!   one.

use std::rc::Rc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use dbhive_core::models::{ActivityStats, ConnectionProfile, DbError, ExportFormat, QueryLog};
use dbhive_core::service_monitoring as mon;
use dbhive_ui_model::activity_monitor::{
    export_file_name, format_bytes, interval_label, refresh_interval, sort_for, FilterState,
    SortField, CHART_SPANS, PAGE_SIZES, REFRESH_SECONDS,
};
use dbhive_ui_model::activity_panel::{
    can_go_next, can_go_prev, clear_logs_message, column_for_sort_field, kill_message, live_tiles,
    log_detail, log_grid, log_summary, page_label, process_grid, sessions_chart, sort_after_click,
    stat_tiles, status_breakdown, transactions_chart, type_breakdown, PollGate, LOG_COLUMN_WIDTHS,
    PROCESS_COLUMN_WIDTHS,
};
use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel};

use super::{with_ctx, Ctx, PendingAction};
use crate::{activity_view as view, runtime};
use crate::{ActivityState, MonitorServerState, Screen, Tone};

/// What the server tab is waiting on or showing.
#[derive(Clone, PartialEq)]
enum Live {
    /// A poll is out and nothing has come back yet.
    Loading,
    Unsupported(String),
    Error(String),
    Ready,
}

pub(super) struct MonitorSession {
    filters: FilterState,
    sort: (SortField, bool),
    page: usize,
    page_size_index: usize,
    selected_log: Option<String>,

    // The page being shown, and what it was taken from.
    logs: Vec<QueryLog>,
    matching: usize,
    total_pages: usize,
    overall: usize,
    stats: Option<ActivityStats>,
    connections: Vec<(String, String)>,
    databases: Vec<String>,
    error: String,
    exporting: bool,

    // Live server view.
    live_connection: Option<String>,
    series: mon::MetricSeries,
    latest: Option<mon::ServerStats>,
    span_index: usize,
    processes: Vec<mon::ActiveQuery>,
    process_selected: Option<i64>,
    live: Live,
    /// Some data has arrived for the current connection.
    have_data: bool,
    /// The last failed refresh, kept beside data that is now stale.
    stale_error: Option<String>,
    updated: Option<DateTime<Local>>,
    live_ticket: runtime::Latest,
    kill_busy: bool,

    // Polling.
    auto: bool,
    interval_index: usize,
    gate: PollGate,
    timer: Timer,
}

impl Default for MonitorSession {
    fn default() -> Self {
        Self {
            filters: FilterState::default(),
            sort: (SortField::StartedAt, true),
            page: 0,
            page_size_index: 1,
            selected_log: None,
            logs: Vec::new(),
            matching: 0,
            total_pages: 0,
            overall: 0,
            stats: None,
            connections: Vec::new(),
            databases: Vec::new(),
            error: String::new(),
            exporting: false,
            live_connection: None,
            series: mon::MetricSeries::default(),
            latest: None,
            span_index: 1,
            processes: Vec::new(),
            process_selected: None,
            live: Live::Loading,
            have_data: false,
            stale_error: None,
            updated: None,
            live_ticket: runtime::Latest::default(),
            kill_busy: false,
            auto: false,
            interval_index: 2,
            gate: PollGate::default(),
            timer: Timer::default(),
        }
    }
}

fn strings(items: impl IntoIterator<Item = String>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        items.into_iter().map(SharedString::from).collect::<Vec<_>>(),
    ))
}

/// A chosen dropdown index as an index into a list that has an "All" entry
/// first: 0 is "no choice", `n` is item `n - 1`.
fn after_all<T: Clone>(items: &[T], index: i32) -> Option<T> {
    usize::try_from(index)
        .ok()
        .and_then(|i| i.checked_sub(1))
        .and_then(|i| items.get(i).cloned())
}

impl Ctx {
    // -- Projection ---------------------------------------------------------

    pub(super) fn refresh_activity_panel(&self) {
        let Some(w) = self.window.upgrade() else { return };
        let a = w.global::<ActivityState>();
        let on_workspace = w.get_screen() == Screen::Workspace;

        // Profiles are read before the session is borrowed.
        let open = self.monitor_open_profiles();

        let s = self.session.borrow();
        let m = &s.monitor;

        // Header.
        a.set_auto_refresh(m.auto);
        a.set_interval_options(strings(REFRESH_SECONDS.iter().map(|n| interval_label(*n))));
        a.set_interval_index(m.interval_index as i32);

        // Queries: filters.
        a.set_error(m.error.as_str().into());
        a.set_exporting(m.exporting);
        a.set_filters_active(m.filters.is_active());
        a.set_filter_connections(strings(
            std::iter::once("All connections".to_string())
                .chain(m.connections.iter().map(|(_, name)| name.clone())),
        ));
        a.set_filter_connection(
            m.filters
                .connection_id
                .as_ref()
                .and_then(|id| m.connections.iter().position(|(c, _)| c == id))
                .map_or(0, |i| i as i32 + 1),
        );
        a.set_filter_databases(strings(
            std::iter::once("All databases".to_string()).chain(m.databases.iter().cloned()),
        ));
        a.set_filter_database(
            m.filters
                .database
                .as_ref()
                .and_then(|d| m.databases.iter().position(|x| x == d))
                .map_or(0, |i| i as i32 + 1),
        );
        a.set_filter_ranges(strings(mon::TimeRange::ALL.iter().map(|r| r.label().to_string())));
        a.set_filter_range(
            mon::TimeRange::ALL
                .iter()
                .position(|r| *r == m.filters.time_range)
                .unwrap_or(0) as i32,
        );
        a.set_type_chips(view::chips(
            &mon::QUERY_TYPES
                .iter()
                .map(|t| (mon::query_type_key(*t), m.filters.types.contains(t)))
                .collect::<Vec<_>>(),
        ));
        a.set_status_chips(view::chips(
            &mon::QUERY_STATUSES
                .iter()
                .map(|st| {
                    (
                        dbhive_ui_model::activity_panel::status_label(*st),
                        m.filters.statuses.contains(st),
                    )
                })
                .collect::<Vec<_>>(),
        ));

        // Queries: grid, sort, pager.
        let (columns, rows, width) = view::grid(&log_grid(&m.logs), &LOG_COLUMN_WIDTHS);
        a.set_columns(columns);
        a.set_rows(rows);
        a.set_grid_width(width);
        a.set_sort_column(column_for_sort_field(m.sort.0) as i32);
        a.set_sort_desc(m.sort.1);
        let selected_index = m
            .selected_log
            .as_ref()
            .and_then(|id| m.logs.iter().position(|l| &l.id == id));
        a.set_selected_row(selected_index.map_or(-1, |i| i as i32));
        a.set_log_total(m.overall as i32);
        a.set_summary(log_summary(m.matching, m.overall, m.filters.is_active()).into());
        a.set_page_label(page_label(m.page, m.total_pages, m.matching).into());
        a.set_can_prev(can_go_prev(m.page));
        a.set_can_next(can_go_next(m.page, m.total_pages));
        a.set_page_sizes(strings(PAGE_SIZES.iter().map(|n| n.to_string())));
        a.set_page_size(PAGE_SIZES[m.page_size_index].to_string().into());

        // Queries: the expanded row.
        match selected_index.map(|i| log_detail(&m.logs[i])) {
            Some(d) => {
                a.set_has_detail(true);
                a.set_detail_status(d.status.as_str().into());
                a.set_detail_tone(view::tone(d.tone));
                a.set_detail_facts(view::facts(&d));
                a.set_detail_sql(d.sql.as_str().into());
                a.set_detail_error(d.error.as_str().into());
            }
            None => a.set_has_detail(false),
        }
        a.set_can_open_in_editor(on_workspace);

        // Statistics.
        match &m.stats {
            Some(stats) => {
                a.set_tiles(view::tiles(&stat_tiles(stats)));
                a.set_type_bars(view::bars(&type_breakdown(stats)));
                a.set_status_bars(view::bars(&status_breakdown(stats)));
            }
            None => {
                a.set_tiles(view::tiles(&[]));
                a.set_type_bars(view::bars(&[]));
                a.set_status_bars(view::bars(&[]));
            }
        }

        // Server.
        let server_state = match (&m.live_connection, &m.live) {
            (None, _) => MonitorServerState::None,
            (Some(_), Live::Unsupported(_)) => MonitorServerState::Unsupported,
            (Some(_), Live::Error(_)) if !m.have_data => MonitorServerState::Error,
            (Some(_), _) if m.have_data => MonitorServerState::Ready,
            (Some(_), _) => MonitorServerState::Loading,
        };
        a.set_server_state(server_state);
        a.set_server_message(
            match &m.live {
                Live::Unsupported(msg) | Live::Error(msg) => msg.clone(),
                _ => String::new(),
            }
            .into(),
        );
        a.set_server_connections(strings(open.iter().map(|p| {
            format!("{} ({})", p.name, crate::connection_list::driver_label(&p.driver))
        })));
        a.set_server_connection(
            m.live_connection
                .as_ref()
                .and_then(|id| open.iter().position(|p| &p.id == id))
                .map_or(-1, |i| i as i32),
        );
        a.set_span_options(strings(CHART_SPANS.iter().map(|(_, l)| l.to_string())));
        a.set_span_index(m.span_index as i32);

        let window = m.series.window(CHART_SPANS[m.span_index].0.map(|s| s * 1_000));
        let rate = (m.series.len() >= 2).then(|| m.series.latest().map(|s| s.tx_per_sec)).flatten();
        a.set_live_tiles(view::tiles(&live_tiles(m.latest.as_ref(), rate)));
        a.set_sessions_chart(view::chart(sessions_chart(window).as_ref()));
        a.set_tx_chart(view::chart(transactions_chart(window).as_ref()));
        a.set_updated_label(
            match (&m.stale_error, m.updated) {
                (Some(e), _) => format!("Last refresh failed: {}", short(e)),
                (None, Some(t)) => format!("Updated {}", t.format("%H:%M:%S")),
                _ => String::new(),
            }
            .into(),
        );

        let (pcols, prows, pwidth) = view::grid(&process_grid(&m.processes), &PROCESS_COLUMN_WIDTHS);
        a.set_process_columns(pcols);
        a.set_process_rows(prows);
        a.set_process_width(pwidth);
        let selected_process = m
            .process_selected
            .and_then(|pid| m.processes.iter().position(|p| p.pid == pid));
        a.set_process_selected(selected_process.map_or(-1, |i| i as i32));
        a.set_process_summary(
            match m.processes.len() {
                1 => "1 active session".to_string(),
                n => format!("{n} active sessions"),
            }
            .into(),
        );
        a.set_can_cancel(selected_process.is_some() && m.live_connection.is_some());
        a.set_cancel_label(
            match m.process_selected.filter(|_| selected_process.is_some()) {
                Some(pid) => format!("Cancel query {pid}"),
                None => "Cancel query".to_string(),
            }
            .into(),
        );
        a.set_server_busy(m.gate.is_busy() || m.kill_busy);
    }

    // -- Data ---------------------------------------------------------------

    /// Re-read the query log, its statistics and the filter choices. In-memory
    /// work, so it runs inline; the lock is taken inside `service_monitoring`
    /// and released before anything is drawn.
    fn monitor_reload_logs(&self) {
        let (query, sort, page, page_size) = {
            let s = self.session.borrow();
            let m = &s.monitor;
            (
                m.filters.to_query(Utc::now()),
                sort_for(m.sort.0, m.sort.1),
                m.page,
                PAGE_SIZES[m.page_size_index],
            )
        };
        let response = mon::get_query_logs(&self.state, &query, &sort, page, page_size);
        let stats = mon::get_activity_stats(&self.state, &query);
        let connections = mon::connection_choices(&self.state);
        let databases = mon::logged_databases(&self.state, query.filter.connection_id.as_deref());
        let overall = mon::get_query_logs_count(&self.state);

        let mut s = self.session.borrow_mut();
        let m = &mut s.monitor;
        m.page = response.page;
        m.matching = response.total;
        m.total_pages = response.total_pages;
        m.logs = response.logs;
        m.stats = Some(stats);
        m.connections = connections;
        m.databases = databases;
        m.overall = overall;
        // The selection is an id; a page that no longer holds it drops it.
        if m.selected_log.as_ref().is_some_and(|id| !m.logs.iter().any(|l| &l.id == id)) {
            m.selected_log = None;
        }
    }

    /// Saved connections that are open right now, by name.
    fn monitor_open_profiles(&self) -> Vec<ConnectionProfile> {
        let connected: Vec<String> = self.session.borrow().connected.iter().cloned().collect();
        let mut out: Vec<ConnectionProfile> = {
            let g = self.state.lock().unwrap();
            connected
                .iter()
                .filter(|id| g.get_connection(id).is_some())
                .filter_map(|id| g.get_profile(id).cloned())
                .collect()
        };
        out.sort_by_key(|p| p.name.to_lowercase());
        out
    }

    /// The connection the server tab watches, kept valid: if it was closed,
    /// fall back to the workspace's connection, then to any open one.
    fn monitor_live_target(&self) -> Option<String> {
        let open = self.monitor_open_profiles();
        let mut s = self.session.borrow_mut();
        let current = s.monitor.live_connection.clone();
        if current.as_ref().is_some_and(|id| open.iter().any(|p| &p.id == id)) {
            return current;
        }
        let workspace = s.workspace.connection_id.clone();
        let next = open
            .iter()
            .find(|p| p.id == workspace)
            .or_else(|| open.first())
            .map(|p| p.id.clone());
        Self::reset_live(&mut s.monitor, next.clone());
        next
    }

    fn reset_live(m: &mut MonitorSession, connection: Option<String>) {
        m.live_connection = connection;
        m.series.clear();
        m.latest = None;
        m.processes.clear();
        m.process_selected = None;
        m.live = Live::Loading;
        m.have_data = false;
        m.stale_error = None;
        m.updated = None;
        // Anything still in flight belongs to the old connection.
        m.live_ticket.begin();
        m.gate.abandon();
    }

    fn monitor_window_hidden(&self) -> bool {
        self.window
            .upgrade()
            .is_none_or(|w| !w.window().is_visible() || w.window().is_minimized())
    }

    fn monitor_is_open(&self) -> bool {
        self.window
            .upgrade()
            .is_some_and(|w| w.global::<ActivityState>().get_visible())
    }

    // -- Open and close -------------------------------------------------------

    pub(super) fn open_activity_monitor(self: &Rc<Self>) {
        let Some(w) = self.window.upgrade() else { return };
        {
            let mut s = self.session.borrow_mut();
            s.monitor.gate.set_visible(true);
            s.monitor.error.clear();
        }
        self.monitor_live_target();
        self.monitor_reload_logs();
        w.global::<ActivityState>().set_visible(true);
        self.refresh_activity_panel();
        self.monitor_start_timer();
        self.monitor_poll_live();
    }

    pub(super) fn hide_activity_monitor(&self) {
        {
            let mut s = self.session.borrow_mut();
            s.monitor.gate.set_visible(false);
            s.monitor.timer.stop();
        }
        if let Some(w) = self.window.upgrade() {
            w.global::<ActivityState>().set_visible(false);
        }
    }

    fn monitor_start_timer(self: &Rc<Self>) {
        // One tick a second is plenty for a 5 second minimum interval, and a
        // tick that finds the gate shut costs nothing.
        self.session
            .borrow()
            .monitor
            .timer
            .start(TimerMode::Repeated, Duration::from_secs(1), || {
                with_ctx(|ctx| ctx.monitor_tick())
            });
    }

    // -- Polling --------------------------------------------------------------

    fn monitor_tick(self: &Rc<Self>) {
        if !self.monitor_is_open() {
            return;
        }
        let hidden = self.monitor_window_hidden();
        let interval = {
            let mut s = self.session.borrow_mut();
            s.monitor.gate.set_visible(!hidden);
            refresh_interval(s.monitor.auto, REFRESH_SECONDS[s.monitor.interval_index])
        };
        let live = self.monitor_live_target();
        let requests = if live.is_some() { 2 } else { 1 };
        let started = self
            .session
            .borrow_mut()
            .monitor
            .gate
            .try_begin(interval, requests, Instant::now());
        if started {
            self.monitor_reload_logs();
            self.monitor_run_poll(live);
        }
    }

    /// A refresh the user asked for, or the first one on open: logs now, the
    /// server if no poll is already out.
    fn monitor_poll_live(self: &Rc<Self>) {
        let live = self.monitor_live_target();
        let requests = if live.is_some() { 2 } else { 1 };
        let started = self
            .session
            .borrow_mut()
            .monitor
            .gate
            .begin_manual(requests, Instant::now());
        if started {
            self.monitor_run_poll(live);
        } else {
            self.refresh_activity_panel();
        }
    }

    /// Start the server requests for a poll the gate has already admitted.
    fn monitor_run_poll(self: &Rc<Self>, live: Option<String>) {
        let Some(connection) = live else {
            // Nothing to ask a server: the one virtual request is done.
            self.session.borrow_mut().monitor.gate.finish_one();
            self.refresh_activity_panel();
            return;
        };
        let ticket = self.session.borrow().monitor.live_ticket.begin();
        self.refresh_activity_panel();

        let (state, id, t) = (self.state.clone(), connection.clone(), ticket.clone());
        runtime::spawn(
            async move { mon::get_server_stats(&state, &id).await },
            move |result| with_ctx(|ctx| ctx.monitor_stats_done(&t, result)),
        );
        let (state, id) = (self.state.clone(), connection);
        runtime::spawn(
            async move { mon::get_active_queries(&state, &id).await },
            move |result| with_ctx(|ctx| ctx.monitor_processes_done(&ticket, result)),
        );
    }

    fn monitor_stats_done(&self, ticket: &runtime::Ticket, result: Result<mon::ServerStats, DbError>) {
        if !ticket.is_current() {
            return;
        }
        {
            let mut s = self.session.borrow_mut();
            let m = &mut s.monitor;
            m.gate.finish_one();
            match result {
                Ok(stats) => {
                    m.series.push(Utc::now().timestamp_millis(), &stats);
                    m.latest = Some(stats);
                    m.have_data = true;
                    m.stale_error = None;
                    m.updated = Some(Local::now());
                    m.live = Live::Ready;
                }
                // A driver with no system views is a fact about the driver,
                // not a failure.
                Err(DbError::InvalidInput(message)) => {
                    m.live = Live::Unsupported(message);
                    m.latest = None;
                }
                Err(e) => Self::monitor_note_failure(m, e.to_string()),
            }
        }
        self.refresh_activity_panel();
    }

    fn monitor_processes_done(
        &self,
        ticket: &runtime::Ticket,
        result: Result<Vec<mon::ActiveQuery>, DbError>,
    ) {
        if !ticket.is_current() {
            return;
        }
        {
            let mut s = self.session.borrow_mut();
            let m = &mut s.monitor;
            m.gate.finish_one();
            match result {
                Ok(rows) => {
                    m.processes = rows;
                    m.have_data = true;
                    m.stale_error = None;
                    m.updated = Some(Local::now());
                    if !matches!(m.live, Live::Unsupported(_)) {
                        m.live = Live::Ready;
                    }
                }
                Err(DbError::InvalidInput(message)) => {
                    m.live = Live::Unsupported(message);
                    m.processes.clear();
                }
                Err(e) => Self::monitor_note_failure(m, e.to_string()),
            }
        }
        self.refresh_activity_panel();
    }

    fn monitor_note_failure(m: &mut MonitorSession, message: String) {
        if m.have_data {
            m.stale_error = Some(message);
        } else {
            m.live = Live::Error(message);
        }
    }

    // -- Intents --------------------------------------------------------------

    fn monitor_refresh(self: &Rc<Self>) {
        self.monitor_reload_logs();
        self.monitor_poll_live();
    }

    /// Change the filters (or the page) and re-read the log.
    fn monitor_edit_filters(&self, edit: impl FnOnce(&mut MonitorSession)) {
        {
            let mut s = self.session.borrow_mut();
            edit(&mut s.monitor);
            s.monitor.page = 0;
        }
        self.monitor_reload_logs();
        self.refresh_activity_panel();
    }

    fn monitor_sort_clicked(&self, column: usize) {
        let changed = {
            let mut s = self.session.borrow_mut();
            match sort_after_click(s.monitor.sort, column) {
                Some(next) => {
                    s.monitor.sort = next;
                    s.monitor.page = 0;
                    true
                }
                None => false,
            }
        };
        if changed {
            self.monitor_reload_logs();
            self.refresh_activity_panel();
        }
    }

    fn monitor_row_clicked(&self, row: usize) {
        {
            let mut s = self.session.borrow_mut();
            let id = s.monitor.logs.get(row).map(|l| l.id.clone());
            s.monitor.selected_log = id;
        }
        self.refresh_activity_panel();
    }

    fn monitor_go_page(&self, forward: bool) {
        {
            let mut s = self.session.borrow_mut();
            let m = &mut s.monitor;
            m.page = if forward { m.page + 1 } else { m.page.saturating_sub(1) };
            m.selected_log = None;
        }
        self.monitor_reload_logs();
        self.refresh_activity_panel();
    }

    fn monitor_selected_sql(&self) -> Option<String> {
        let s = self.session.borrow();
        let id = s.monitor.selected_log.as_ref()?;
        s.monitor.logs.iter().find(|l| &l.id == id).map(|l| l.sql.clone())
    }

    fn monitor_reset_filters(&self) {
        if let Some(w) = self.window.upgrade() {
            let a = w.global::<ActivityState>();
            a.set_search("".into());
            a.set_min_duration("".into());
            a.set_max_duration("".into());
        }
        self.monitor_edit_filters(|m| m.filters = FilterState::default());
    }

    fn monitor_export(self: &Rc<Self>, format: ExportFormat) {
        if self.session.borrow().monitor.exporting {
            return;
        }
        let path = match dbhive_core::storage::app_data_dir() {
            Ok(dir) => dir.join(export_file_name(format, Local::now())),
            Err(e) => {
                self.session.borrow_mut().monitor.error = format!("Could not choose an export path: {e}");
                self.refresh_activity_panel();
                return;
            }
        };
        let query = {
            let mut s = self.session.borrow_mut();
            s.monitor.exporting = true;
            s.monitor.error.clear();
            s.monitor.filters.to_query(Utc::now())
        };
        self.refresh_activity_panel();
        let state = self.state.clone();
        let path = path.to_string_lossy().to_string();
        runtime::spawn(
            async move { mon::export_query_logs(&state, &query, format, path).await },
            |result| {
                with_ctx(|ctx| {
                    ctx.session.borrow_mut().monitor.exporting = false;
                    match result {
                        Ok(out) => ctx.toast(
                            format!(
                                "Exported {} {} ({}) to {}",
                                out.logs,
                                if out.logs == 1 { "query" } else { "queries" },
                                format_bytes(out.bytes),
                                out.path
                            ),
                            Tone::Success,
                        ),
                        Err(e) => {
                            ctx.session.borrow_mut().monitor.error = format!("Export failed: {e}");
                        }
                    }
                    ctx.refresh_activity_panel();
                })
            },
        );
    }

    fn monitor_clear_asked(&self) {
        let count = self.session.borrow().monitor.overall;
        self.session.borrow_mut().pending_action = Some(PendingAction::ClearActivityLog);
        self.show_confirm("Clear the activity log?", &clear_logs_message(count), "Clear log");
    }

    pub(super) fn monitor_clear_confirmed(self: &Rc<Self>) {
        let removed = mon::clear_query_logs(&self.state);
        {
            let mut s = self.session.borrow_mut();
            s.monitor.selected_log = None;
            s.monitor.page = 0;
        }
        self.monitor_reload_logs();
        self.refresh_activity_panel();
        self.toast(
            format!("Cleared {} {}", removed, if removed == 1 { "query" } else { "queries" }),
            Tone::Success,
        );
    }

    fn monitor_cancel_asked(&self) {
        let ask = {
            let s = self.session.borrow();
            s.monitor.process_selected.and_then(|pid| {
                s.monitor.processes.iter().find(|p| p.pid == pid).map(|p| {
                    (pid, kill_message(pid, p.user.as_deref(), p.query_text.as_deref()))
                })
            })
        };
        let Some((pid, message)) = ask else { return };
        self.session.borrow_mut().pending_action = Some(PendingAction::KillProcess(pid));
        self.show_confirm("Cancel this query?", &message, "Cancel query");
    }

    pub(super) fn monitor_kill_confirmed(self: &Rc<Self>, pid: i64) {
        let Some(connection) = self.session.borrow().monitor.live_connection.clone() else {
            return;
        };
        self.session.borrow_mut().monitor.kill_busy = true;
        self.refresh_activity_panel();
        let state = self.state.clone();
        runtime::spawn(
            async move { mon::kill_query(&state, &connection, pid).await },
            move |result| {
                with_ctx(|ctx| {
                    ctx.session.borrow_mut().monitor.kill_busy = false;
                    match result {
                        Ok(()) => ctx.toast(format!("Cancel signal sent to process {pid}"), Tone::Success),
                        Err(e) => ctx.toast(format!("Could not cancel process {pid}: {e}"), Tone::Danger),
                    }
                    // Show the effect without waiting for the next tick.
                    ctx.monitor_poll_live();
                })
            },
        );
    }

    fn monitor_pick_connection(self: &Rc<Self>, index: i32) {
        let open = self.monitor_open_profiles();
        let Some(profile) = usize::try_from(index).ok().and_then(|i| open.get(i)) else { return };
        {
            let mut s = self.session.borrow_mut();
            if s.monitor.live_connection.as_ref() == Some(&profile.id) {
                return;
            }
            // Rates diffed across two servers mean nothing.
            Self::reset_live(&mut s.monitor, Some(profile.id.clone()));
        }
        self.monitor_poll_live();
    }
}

/// Connect every `ActivityState` callback to the controller.
pub(super) fn wire(window: &crate::AppWindow, ctx: &Rc<Ctx>) {
    let a = window.global::<ActivityState>();

    macro_rules! on {
        ($setter:ident, |$c:ident $(, $arg:ident)*| $body:expr) => {{
            let $c = ctx.clone();
            a.$setter(move |$($arg),*| {
                let $c = &$c;
                $body
            });
        }};
    }

    on!(on_open_requested, |c| c.open_activity_monitor());
    on!(on_close, |c| c.hide_activity_monitor());
    on!(on_tab_selected, |c, i| {
        if let Some(w) = c.window.upgrade() {
            w.global::<ActivityState>().set_tab(i);
        }
    });

    on!(on_refresh, |c| c.monitor_refresh());
    on!(on_auto_toggled, |c| {
        {
            let mut s = c.session.borrow_mut();
            s.monitor.auto = !s.monitor.auto;
        }
        c.refresh_activity_panel();
    });
    on!(on_interval_selected, |c, i| {
        c.session.borrow_mut().monitor.interval_index = (i.max(0) as usize).min(REFRESH_SECONDS.len() - 1);
        c.refresh_activity_panel();
    });

    on!(on_search_edited, |c, t| {
        let t = t.to_string();
        c.monitor_edit_filters(|m| m.filters.search_text = t)
    });
    on!(on_filters_toggled, |c| {
        if let Some(w) = c.window.upgrade() {
            let a = w.global::<ActivityState>();
            a.set_filters_open(!a.get_filters_open());
        }
    });
    on!(on_connection_picked, |c, i| {
        c.monitor_edit_filters(|m| {
            m.filters.connection_id = after_all(&m.connections, i).map(|(id, _)| id);
            // The database list follows the connection.
            m.filters.database = None;
        })
    });
    on!(on_database_picked, |c, i| {
        c.monitor_edit_filters(|m| m.filters.database = after_all(&m.databases, i))
    });
    on!(on_range_picked, |c, i| {
        c.monitor_edit_filters(|m| {
            m.filters.time_range = mon::TimeRange::ALL
                .get(i.max(0) as usize)
                .copied()
                .unwrap_or_default()
        })
    });
    on!(on_duration_edited, |c| {
        let (min, max) = match c.window.upgrade() {
            Some(w) => {
                let a = w.global::<ActivityState>();
                (a.get_min_duration().to_string(), a.get_max_duration().to_string())
            }
            None => return,
        };
        c.monitor_edit_filters(|m| {
            m.filters.min_duration = min;
            m.filters.max_duration = max;
        })
    });
    on!(on_type_toggled, |c, i| {
        c.monitor_edit_filters(|m| {
            if let Some(t) = mon::QUERY_TYPES.get(i.max(0) as usize) {
                m.filters.toggle_type(*t);
            }
        })
    });
    on!(on_status_toggled, |c, i| {
        c.monitor_edit_filters(|m| {
            if let Some(st) = mon::QUERY_STATUSES.get(i.max(0) as usize) {
                m.filters.toggle_status(*st);
            }
        })
    });
    on!(on_filters_reset, |c| c.monitor_reset_filters());
    on!(on_sort_clicked, |c, col| c.monitor_sort_clicked(col.max(0) as usize));
    on!(on_row_clicked, |c, r| c.monitor_row_clicked(r.max(0) as usize));
    on!(on_prev_page, |c| c.monitor_go_page(false));
    on!(on_next_page, |c| c.monitor_go_page(true));
    on!(on_page_size_picked, |c, i| {
        {
            let mut s = c.session.borrow_mut();
            s.monitor.page_size_index = (i.max(0) as usize).min(PAGE_SIZES.len() - 1);
            s.monitor.page = 0;
        }
        c.monitor_reload_logs();
        c.refresh_activity_panel();
    });
    on!(on_export_picked, |c, i| {
        c.monitor_export(match i {
            0 => ExportFormat::Csv,
            1 => ExportFormat::Json,
            _ => ExportFormat::Txt,
        })
    });
    on!(on_clear_requested, |c| c.monitor_clear_asked());
    on!(on_copy_sql, |c| {
        if let (Some(sql), Some(w)) = (c.monitor_selected_sql(), c.window.upgrade()) {
            w.invoke_copy_to_clipboard(sql.into());
            c.toast("Copied the SQL", Tone::Success);
        }
    });
    on!(on_open_in_editor, |c| {
        if let Some(sql) = c.monitor_selected_sql() {
            c.open_query_tab_with(&sql);
            c.hide_activity_monitor();
        }
    });
    on!(on_detail_close, |c| {
        c.session.borrow_mut().monitor.selected_log = None;
        c.refresh_activity_panel();
    });

    on!(on_server_connection_picked, |c, i| c.monitor_pick_connection(i));
    on!(on_span_picked, |c, i| {
        c.session.borrow_mut().monitor.span_index = (i.max(0) as usize).min(CHART_SPANS.len() - 1);
        c.refresh_activity_panel();
    });
    on!(on_process_clicked, |c, r| {
        {
            let mut s = c.session.borrow_mut();
            let pid = s.monitor.processes.get(r.max(0) as usize).map(|p| p.pid);
            s.monitor.process_selected = pid;
        }
        c.refresh_activity_panel();
    });
    on!(on_cancel_requested, |c| c.monitor_cancel_asked());
    on!(on_server_retry, |c| c.monitor_poll_live());
}

/// A failure message short enough for the toolbar.
fn short(message: &str) -> String {
    let flat = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= 60 {
        flat
    } else {
        format!("{}…", flat.chars().take(60).collect::<String>())
    }
}
