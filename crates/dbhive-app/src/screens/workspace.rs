//! The connected workspace: schema sidebar, SQL editor, results grid.
//!
//! Parity target: the React `/query` route plus the schema explorer sidebar.

use std::sync::Arc;

use dbhive_core::drivers::{DatabaseDriver, QueryResult};
use dbhive_core::models::{DatabaseInfo, DbError, QueryHistory, SchemaInfo, TableInfo};
use dbhive_core::state::AppState;

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::widgets::{results_grid, sql_editor};

/// databases, schemas, tables, and the database actually in use.
type Metadata = (Vec<DatabaseInfo>, Vec<SchemaInfo>, Vec<TableInfo>, String);

/// One query tab. Each carries its own statement, results and in-flight
/// query, so running a slow query in one tab does not block editing in
/// another.
pub struct QueryTab {
    pub title: String,
    pub sql: String,
    pub editor: sql_editor::EditorState,
    pub grid: results_grid::GridState,
    pub result: Option<QueryResult>,
    pub last_error: Option<String>,
    pub elapsed_ms: Option<u128>,
    pub query_job: Job<(Result<QueryResult, DbError>, u128)>,
}

impl QueryTab {
    pub fn new(index: usize) -> Self {
        Self {
            title: format!("Query {}", index),
            sql: String::new(),
            editor: sql_editor::EditorState::default(),
            grid: results_grid::GridState::default(),
            result: None,
            last_error: None,
            elapsed_ms: None,
            query_job: Job::default(),
        }
    }
}

pub struct Workspace {
    pub connection_id: String,
    pub connection_name: String,

    pub databases: Vec<DatabaseInfo>,
    pub current_database: Option<String>,
    pub schemas: Vec<SchemaInfo>,
    pub current_schema: Option<String>,
    pub tables: Vec<TableInfo>,
    pub table_filter: String,

    pub schema_index: sql_editor::SchemaIndex,

    pub tabs: Vec<QueryTab>,
    pub active_tab: usize,
    /// Monotonic counter so closing tab 2 does not make the next new tab
    /// also "Query 2".
    next_tab_number: usize,

    pub meta_job: Job<Result<Metadata, DbError>>,
    pub switch_job: Job<Result<String, DbError>>,
    pub pending_database: Option<String>,
    /// Connection-level failure (metadata load, database switch), as opposed
    /// to a per-tab query error.
    pub last_error: Option<String>,
    meta_requested: bool,
}

impl Workspace {
    pub fn active(&mut self) -> &mut QueryTab {
        let i = self.active_tab.min(self.tabs.len().saturating_sub(1));
        &mut self.tabs[i]
    }

    pub fn add_tab(&mut self) {
        self.next_tab_number += 1;
        self.tabs.push(QueryTab::new(self.next_tab_number));
        self.active_tab = self.tabs.len() - 1;
    }

    /// Close a tab, keeping at least one open.
    pub fn close_tab(&mut self, index: usize) {
        if self.tabs.len() <= 1 || index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if index < self.active_tab {
            self.active_tab -= 1;
        }
    }
}

impl Workspace {
    pub fn new(connection_id: String, connection_name: String) -> Self {
        Self {
            connection_id,
            connection_name,
            databases: Vec::new(),
            current_database: None,
            schemas: Vec::new(),
            current_schema: None,
            tables: Vec::new(),
            table_filter: String::new(),
            schema_index: sql_editor::SchemaIndex::default(),
            tabs: vec![QueryTab::new(1)],
            active_tab: 0,
            next_tab_number: 1,
            meta_job: Job::default(),
            switch_job: Job::default(),
            pending_database: None,
            last_error: None,
            meta_requested: false,
        }
    }
}

/// Pull the driver out of state, cloning the Arc so the lock is released
/// before any await. Holding it across a query would freeze the UI.
fn driver(app: &DbHiveApp, connection_id: &str) -> Option<Arc<dyn DatabaseDriver>> {
    app.state.lock().unwrap().get_connection(connection_id).cloned()
}

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    // Read what is needed up front so no borrow of `app.workspace` is held
    // across the `driver()` lookup, which borrows `app` immutably.
    let Some((connection_id, needs_meta)) = app
        .workspace
        .as_ref()
        .map(|w| (w.connection_id.clone(), !w.meta_requested))
    else {
        return;
    };
    let palette = app.mode.palette();
    let mut run_now = false;
    let mut disconnect = false;
    let mut switch_to: Option<String> = None;
    let mut reload_tables = false;
    let mut open_inspector: Option<(String, String, String)> = None;
    let mut pending_switch: Option<String> = None;

    // Kick off the metadata load once per connection.
    if needs_meta {
        app.workspace.as_mut().unwrap().meta_requested = true;
        load_metadata(app, ui.ctx(), None);
        // Development aid, paired with DBHIVE_AUTOCONNECT: run a statement as
        // soon as the workspace opens. Unset in normal use.
        if let Ok(sql) = std::env::var("DBHIVE_AUTOQUERY") {
            app.workspace.as_mut().unwrap().active().sql = sql;
            run_now = true;
        }

    }

    let ws = app.workspace.as_mut().unwrap();
    if let Some(result) = ws.meta_job.take_result() {
        match result {
            Ok((databases, schemas, tables, database)) => {
                if ws.current_database.is_none() {
                    ws.current_database = databases.first().map(|d| d.name.clone());
                }
                if ws.current_schema.is_none() {
                    ws.current_schema = pick_default_schema(&schemas);
                }
                ws.databases = databases;
                ws.schemas = schemas;
                ws.tables = tables;
                rebuild_index(ws);

                // Dev aid: DBHIVE_AUTODB switches to a named database once
                // the list is known, before anything else keys off it.
                if let Ok(want) = std::env::var("DBHIVE_AUTODB") {
                    if ws.current_database.as_deref() != Some(want.as_str())
                        && ws.databases.iter().any(|d| d.name == want)
                    {
                        pending_switch = Some(want);
                    }
                }

                // Dev aid: opening this before the metadata landed passed an
                // empty schema, which MySQL silently ignored and Postgres
                // would have got wrong.
                let want_db = std::env::var("DBHIVE_AUTODB").ok();
                let on_target = match (&want_db, &ws.current_database) {
                    (Some(w), Some(c)) => w == c,
                    (Some(_), None) => false,
                    (None, _) => true,
                };
                if pending_switch.is_none()
                    && on_target
                    && std::env::var("DBHIVE_AUTOSCREEN").as_deref() == Ok("er")
                {
                    let conn = ws.connection_id.clone();
                    let schema = current_schema_or_database(ws);
                    app.screens.er_diagram.open_for(conn, schema);
                }
            }
            Err(e) => ws.last_error = Some(e.to_string()),
        }
    }
    if let Some(result) = ws.switch_job.take_result() {
        match result {
            Ok(_) => {
                let db = ws.pending_database.take();
                // The session is on this database now. The profile still
                // names the original one, so anything re-deriving the label
                // from the profile would put the picker out of step with
                // what the connection is actually pointed at.
                if let Some(name) = db.clone() {
                    ws.current_database = Some(name);
                }
                let ctx = ui.ctx().clone();
                load_metadata(app, &ctx, db);
                let ws = app.workspace.as_mut().unwrap();
                for tab in &mut ws.tabs {
                    tab.result = None;
                    tab.last_error = None;
                }
            }
            Err(e) => {
                ws.pending_database = None;
                ws.last_error = Some(e.to_string());
            }
        }
    }

    // Every tab is polled, not just the active one: a query started in one
    // tab must still land if the user switches away while it runs.
    let ws = app.workspace.as_mut().unwrap();
    let conn_name = ws.connection_name.clone();
    let conn_id = ws.connection_id.clone();
    let database = ws.current_database.clone().unwrap_or_default();
    let mut landed: Vec<QueryHistory> = Vec::new();

    for tab in &mut ws.tabs {
        if let Some((result, elapsed)) = tab.query_job.take_result() {
            tab.elapsed_ms = Some(elapsed);
            let entry = QueryHistory::new(
                conn_id.clone(),
                conn_name.clone(),
                database.clone(),
                tab.sql.clone(),
                chrono::Utc::now().to_rfc3339(),
            );
            match result {
                Ok(r) => {
                    landed.push(entry.with_success(elapsed as u64, Some(r.rows.len() as u64)));
                    tab.grid = results_grid::GridState::default();
                    tab.result = Some(r);
                    tab.last_error = None;
                }
                Err(e) => {
                    landed.push(entry.with_error(e.to_string(), Some(elapsed as u64)));
                    tab.result = None;
                    tab.last_error = Some(e.to_string());
                }
            }
        }
    }

    // Persisting history is fire-and-forget: it must never delay showing the
    // result, and a failed write is not worth interrupting the user for.
    if !landed.is_empty() {
        let snapshot = {
            let mut guard = app.state.lock().unwrap();
            for entry in landed {
                guard.add_history(entry);
            }
            guard.get_all_history()
        };
        app.screens.history.invalidate_history();
        app.rt.spawn(async move {
            if let Err(e) = AppState::save_history_to_store(&snapshot) {
                eprintln!("Warning: failed to persist query history: {}", e);
            }
        });
    }

    egui::SidePanel::left("schema_sidebar")
        .resizable(true)
        .default_width(240.0)
        .frame(
            egui::Frame::none()
                .fill(palette.sidebar)
                .inner_margin(egui::Margin::same(10.0)),
        )
        .show_inside(ui, |ui| {
            let ws = app.workspace.as_mut().unwrap();
            ui.horizontal(|ui| {
                ui.label(crate::theme::text::strong(palette, ws.connection_name.clone()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Disconnect").clicked() {
                        disconnect = true;
                    }
                });
            });
            ui.add_space(6.0);

            if !ws.databases.is_empty() {
                let current = ws
                    .current_database
                    .clone()
                    .unwrap_or_else(|| ws.databases[0].name.clone());
                egui::ComboBox::from_id_salt("database_picker")
                    .selected_text(current.clone())
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        for db in &ws.databases {
                            let selected = current == db.name;
                            if ui.selectable_label(selected, &db.name).clicked() && !selected {
                                ws.current_database = Some(db.name.clone());
                                switch_to = Some(db.name.clone());
                            }
                        }
                    });
                ui.add_space(6.0);
            }

            if !ws.schemas.is_empty() {
                let current = ws
                    .current_schema
                    .clone()
                    .unwrap_or_else(|| ws.schemas[0].name.clone());
                egui::ComboBox::from_id_salt("schema_picker")
                    .selected_text(current.clone())
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        for sc in &ws.schemas {
                            let selected = current == sc.name;
                            if ui.selectable_label(selected, &sc.name).clicked() && !selected {
                                ws.current_schema = Some(sc.name.clone());
                                reload_tables = true;
                            }
                        }
                    });
                ui.add_space(6.0);
            }

            ui.add(
                egui::TextEdit::singleline(&mut ws.table_filter)
                    .hint_text("Search tables...")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(6.0);

            if let Some(err) = ws.last_error.clone() {
                ui.colored_label(palette.destructive, err);
                if ui.small_button("Dismiss").clicked() {
                    ws.last_error = None;
                }
                ui.add_space(4.0);
            }

            if ws.meta_job.is_running() {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label(crate::theme::text::muted(palette, "Loading schema"));
                });
            }

            let needle = ws.table_filter.to_lowercase();
            let mut insert: Option<String> = None;
            let mut inspect: Option<(String, String)> = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for t in ws
                    .tables
                    .iter()
                    .filter(|t| needle.is_empty() || t.name.to_lowercase().contains(&needle))
                {
                    let label = match t.row_count {
                        Some(n) => format!("{}  ({} rows)", t.name, n),
                        None => t.name.clone(),
                    };
                    let response = ui
                        .add(egui::Label::new(label).sense(egui::Sense::click()))
                        .on_hover_text("Click to query, right-click for more");
                    if response.clicked() {
                        insert = Some(t.name.clone());
                    }
                    response.context_menu(|ui| {
                        if ui.button("Query first 100 rows").clicked() {
                            insert = Some(t.name.clone());
                            ui.close_menu();
                        }
                        if ui.button("Inspect table").clicked() {
                            inspect = Some((t.schema.clone(), t.name.clone()));
                            ui.close_menu();
                        }
                    });
                }
            });
            if let Some((schema, table)) = inspect {
                open_inspector = Some((ws.connection_id.clone(), schema, table));
            }
            if let Some(name) = insert {
                let tab = ws.active();
                tab.sql = format!("SELECT * FROM {} LIMIT 100;", name);
                tab.title = name;
                run_now = true;
            }
        });

    egui::TopBottomPanel::bottom("status_bar")
        .exact_height(26.0)
        .frame(
            egui::Frame::none()
                .fill(palette.sidebar)
                .inner_margin(egui::Margin::symmetric(10.0, 4.0)),
        )
        .show_inside(ui, |ui| {
            let ws = app.workspace.as_ref().unwrap();
            let tab = ws.tabs.get(ws.active_tab);
            ui.horizontal(|ui| {
                match tab.map(|t| (&t.result, t.elapsed_ms)) {
                    Some((Some(r), Some(ms))) => ui.label(crate::theme::text::muted(
                        palette,
                        format!("{} rows in {} ms", r.rows.len(), ms),
                    )),
                    _ => ui.label(crate::theme::text::muted(palette, "Ready")),
                };
                if let Some(db) = &ws.current_database {
                    ui.separator();
                    ui.label(crate::theme::text::muted(palette, db.clone()));
                }
            });
        });

    egui::SidePanel::right("history_panel")
        .resizable(true)
        .default_width(320.0)
        .frame(
            egui::Frame::none()
                .fill(palette.sidebar)
                .inner_margin(egui::Margin::same(10.0)),
        )
        .show_inside(ui, |ui| {
            if let crate::screens::history::HistoryAction::UseSql(sql) =
                crate::screens::history::show(app, ui)
            {
                app.workspace.as_mut().unwrap().active().sql = sql;
            }
        });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        let ws = app.workspace.as_mut().unwrap();

        // Tab strip. Recorded then applied, since the buttons borrow `ws`.
        let mut select: Option<usize> = None;
        let mut close: Option<usize> = None;
        let mut new_tab = false;
        egui::ScrollArea::horizontal()
            .id_salt("query_tabs")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let closable = ws.tabs.len() > 1;
                    for (i, tab) in ws.tabs.iter().enumerate() {
                        let active = i == ws.active_tab;
                        let mut label = tab.title.clone();
                        if tab.query_job.is_running() {
                            label.push_str(" …");
                        }
                        if ui.selectable_label(active, label).clicked() {
                            select = Some(i);
                        }
                        if closable && ui.small_button("✕").on_hover_text("Close tab").clicked() {
                            close = Some(i);
                        }
                        ui.separator();
                    }
                    if ui.small_button("+").on_hover_text("New query tab").clicked() {
                        new_tab = true;
                    }
                });
            });
        if let Some(i) = select {
            ws.active_tab = i;
        }
        if let Some(i) = close {
            ws.close_tab(i);
        }
        if new_tab {
            ws.add_tab();
        }
        ui.add_space(6.0);

        let schema_index = ws.schema_index.clone();
        let tab = ws.active();

        ui.horizontal(|ui| {
            let running = tab.query_job.is_running();
            if ui
                .add_enabled(
                    !running && !tab.sql.trim().is_empty(),
                    egui::Button::new("Execute"),
                )
                .on_hover_text("Cmd/Ctrl + Enter")
                .clicked()
            {
                run_now = true;
            }
            if running {
                ui.add(egui::Spinner::new());
            }
            if ui.button("Clear").clicked() {
                tab.sql.clear();
            }
        });
        ui.add_space(6.0);

        let action = sql_editor::show(ui, palette, &mut tab.editor, &schema_index, &mut tab.sql);
        if action == sql_editor::EditorAction::Execute {
            run_now = true;
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        if let Some(err) = tab.last_error.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(palette.destructive, "Query failed:");
                ui.label(err);
            });
        } else if tab.result.is_some() {
            let result = tab.result.take().unwrap();
            results_grid::show(ui, palette, &result, &mut tab.grid);
            tab.result = Some(result);
        } else {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.label(crate::theme::text::muted(
                    palette,
                    "Execute a query to see results",
                ));
            });
        }
    });

    if let Some(db) = pending_switch {
        app.workspace.as_mut().unwrap().current_schema = None;
        switch_database(app, ui.ctx(), db);
    }
    if let Some(db) = switch_to {
        // A different database means different schemas; let them be
        // rediscovered rather than carrying the old one across.
        app.workspace.as_mut().unwrap().current_schema = None;
        switch_database(app, ui.ctx(), db);
    }
    if reload_tables {
        let ctx = ui.ctx().clone();
        load_metadata(app, &ctx, None);
    }
    if let Some((conn, schema, table)) = open_inspector {
        app.screens.table_inspector.open_table(conn, schema, table);
    }
    if run_now {
        execute(app, ui.ctx());
    }
    if disconnect {
        let id = app.workspace.as_ref().unwrap().connection_id.clone();
        let state = app.state.clone();
        app.connections.connected.retain(|c| c != &id);
        app.connections
            .disconnect_job
            .start(app.rt.handle(), ui.ctx(), async move {
                dbhive_core::service::disconnect(&state, &id).await
            });
        app.workspace = None;
        app.screen = super::Screen::Connections;
    }
}

/// Point the connection at another database, then reload the sidebar.
fn switch_database(app: &mut DbHiveApp, ctx: &egui::Context, database: String) {
    let Some(ws) = app.workspace.as_ref() else {
        return;
    };
    let connection_id = ws.connection_id.clone();
    let state = app.state.clone();
    let handle = app.rt.handle().clone();
    let ctx2 = ctx.clone();
    let target = database.clone();

    app.workspace
        .as_mut()
        .unwrap()
        .switch_job
        .start(&handle, &ctx2, async move {
            dbhive_core::service::switch_database(&state, &connection_id, &target).await
        });
    app.workspace.as_mut().unwrap().pending_database = Some(database);
}

/// Load databases and the table list for `database`.
///
/// The schema argument matters per driver: Postgres wants a schema name
/// ("public"), MySQL wants the database. Passing a hardcoded "public" listed
/// MySQL's internal tables instead of the user's, so the selected database is
/// used and "public" is only the Postgres fallback.
fn load_metadata(app: &mut DbHiveApp, ctx: &egui::Context, database: Option<String>) {
    let Some(ws) = app.workspace.as_ref() else {
        return;
    };
    let connection_id = ws.connection_id.clone();
    let target = database.or_else(|| ws.current_database.clone());

    let wanted_schema = ws.current_schema.clone();
    // What the session is on, which outranks the profile after a switch.
    let session_database = ws.current_database.clone();
    let Some(d) = driver(app, &connection_id) else {
        return;
    };
    // The profile decides both what `get_tables` expects and which database
    // the connection is actually on.
    let (is_postgres_family, connected_database) = {
        let guard = app.state.lock().unwrap();
        match guard.get_profile(&connection_id) {
            Some(p) => {
                let pg = p.driver.is_postgres_compatible();
                // service::connect falls back to "postgres" when a
                // Postgres profile names no database; mirror that, or the
                // picker will claim a database the session is not on.
                let db = match p.database.as_deref() {
                    Some(d) if !d.is_empty() => Some(d.to_string()),
                    _ if pg => Some("postgres".to_string()),
                    _ => None,
                };
                (pg, db)
            }
            None => (false, None),
        }
    };

    let handle = app.rt.handle().clone();
    let ctx = ctx.clone();
    app.workspace
        .as_mut()
        .unwrap()
        .meta_job
        .start(&handle, &ctx, async move {
            let databases = d.get_databases().await?;
            // Default to the database the session is actually on, not
            // simply the first one the server reports.
            let database = target
                .or(session_database)
                .or(connected_database)
                .or_else(|| databases.first().map(|db| db.name.clone()))
                .unwrap_or_default();

            // `get_tables` takes a *schema* on Postgres and is driven by the
            // connection's own database on MySQL. Passing the database name to
            // Postgres returned nothing; passing "public" to MySQL listed its
            // internal tables. Both bugs were visible in the sidebar.
            let (schemas, schema_arg) = if is_postgres_family {
                let schemas = d.get_schemas(&database).await.unwrap_or_default();
                let chosen = wanted_schema
                    .or_else(|| pick_default_schema(&schemas))
                    .unwrap_or_else(|| "public".to_string());
                (schemas, chosen)
            } else {
                (Vec::new(), database.clone())
            };

            // An empty table list is not fatal; the sidebar just stays empty.
            let tables = d.get_tables(&schema_arg).await.unwrap_or_default();
            Ok((databases, schemas, tables, database))
        });
}

/// The schema to browse, falling back to the database on drivers with no
/// schema concept (MySQL, SQLite).
pub fn current_schema_or_database(ws: &Workspace) -> String {
    ws.current_schema
        .clone()
        .or_else(|| ws.current_database.clone())
        .unwrap_or_default()
}

/// Prefer "public" when a Postgres server offers it, else the first schema.
///
/// Postgres lists internal schemas too; landing the user in pg_catalog would
/// be useless.
fn pick_default_schema(schemas: &[SchemaInfo]) -> Option<String> {
    if schemas.iter().any(|s| s.name == "public") {
        return Some("public".to_string());
    }
    schemas
        .iter()
        .find(|s| !s.name.starts_with("pg_") && s.name != "information_schema")
        .or_else(|| schemas.first())
        .map(|s| s.name.clone())
}

/// Refresh the completion index from the loaded tables.
fn rebuild_index(ws: &mut Workspace) {
    ws.schema_index.tables = ws.tables.iter().map(|t| t.name.clone()).collect();
}

pub fn execute(app: &mut DbHiveApp, ctx: &egui::Context) {
    let Some(ws) = app.workspace.as_mut() else {
        return;
    };
    let connection_id = ws.connection_id.clone();
    let tab = ws.active();
    if tab.query_job.is_running() || tab.sql.trim().is_empty() {
        return;
    }
    let sql = tab.sql.clone();

    let Some(d) = driver(app, &connection_id) else {
        app.workspace.as_mut().unwrap().active().last_error =
            Some("Connection is no longer open".to_string());
        return;
    };
    let handle = app.rt.handle().clone();
    app.workspace
        .as_mut()
        .unwrap()
        .active()
        .query_job
        .start(&handle, ctx, async move {
            let started = std::time::Instant::now();
            let out = d.execute_query(&sql).await;
            (out, started.elapsed().as_millis())
        });
}

/// Reload databases, schemas and tables for the open connection.
pub fn refresh_metadata(app: &mut DbHiveApp, ctx: &egui::Context) {
    if app.workspace.is_some() {
        load_metadata(app, ctx, None);
    }
}
