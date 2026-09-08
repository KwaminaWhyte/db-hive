//! The connected workspace: schema sidebar, SQL editor, results grid.
//!
//! Parity target: the React `/query` route plus the schema explorer sidebar.

use std::sync::Arc;

use dbhive_core::drivers::{DatabaseDriver, QueryResult};
use dbhive_core::models::{DatabaseInfo, DbError, TableInfo};

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::widgets::{results_grid, sql_editor};

type Metadata = (Vec<DatabaseInfo>, Vec<TableInfo>);

pub struct Workspace {
    pub connection_id: String,
    pub connection_name: String,

    pub databases: Vec<DatabaseInfo>,
    pub current_database: Option<String>,
    pub tables: Vec<TableInfo>,
    pub table_filter: String,

    pub sql: String,
    pub editor: sql_editor::EditorState,
    pub schema_index: sql_editor::SchemaIndex,

    pub grid: results_grid::GridState,
    pub result: Option<QueryResult>,
    pub last_error: Option<String>,
    pub elapsed_ms: Option<u128>,

    pub query_job: Job<(Result<QueryResult, DbError>, u128)>,
    pub meta_job: Job<Result<Metadata, DbError>>,
    pub switch_job: Job<Result<String, DbError>>,
    pub pending_database: Option<String>,
    meta_requested: bool,
}

impl Workspace {
    pub fn new(connection_id: String, connection_name: String) -> Self {
        Self {
            connection_id,
            connection_name,
            databases: Vec::new(),
            current_database: None,
            tables: Vec::new(),
            table_filter: String::new(),
            sql: String::new(),
            editor: sql_editor::EditorState::default(),
            schema_index: sql_editor::SchemaIndex::default(),
            grid: results_grid::GridState::default(),
            result: None,
            last_error: None,
            elapsed_ms: None,
            query_job: Job::default(),
            meta_job: Job::default(),
            switch_job: Job::default(),
            pending_database: None,
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

    // Kick off the metadata load once per connection.
    if needs_meta {
        app.workspace.as_mut().unwrap().meta_requested = true;
        load_metadata(app, ui.ctx(), None);
        // Development aid, paired with DBHIVE_AUTOCONNECT: run a statement as
        // soon as the workspace opens. Unset in normal use.
        if let Ok(sql) = std::env::var("DBHIVE_AUTOQUERY") {
            app.workspace.as_mut().unwrap().sql = sql;
            run_now = true;
        }
    }

    let ws = app.workspace.as_mut().unwrap();
    if let Some(result) = ws.meta_job.take_result() {
        match result {
            Ok((databases, tables)) => {
                if ws.current_database.is_none() {
                    ws.current_database = databases.first().map(|d| d.name.clone());
                }
                ws.databases = databases;
                ws.tables = tables;
                rebuild_index(ws);
            }
            Err(e) => ws.last_error = Some(e.to_string()),
        }
    }
    if let Some(result) = ws.switch_job.take_result() {
        match result {
            Ok(_) => {
                let db = ws.pending_database.take();
                let ctx = ui.ctx().clone();
                load_metadata(app, &ctx, db);
                let ws = app.workspace.as_mut().unwrap();
                ws.result = None;
                ws.last_error = None;
            }
            Err(e) => {
                ws.pending_database = None;
                ws.last_error = Some(e.to_string());
            }
        }
    }

    let ws = app.workspace.as_mut().unwrap();
    if let Some((result, elapsed)) = ws.query_job.take_result() {
        ws.elapsed_ms = Some(elapsed);
        match result {
            Ok(r) => {
                ws.grid = results_grid::GridState::default();
                ws.result = Some(r);
                ws.last_error = None;
            }
            Err(e) => {
                ws.result = None;
                ws.last_error = Some(e.to_string());
            }
        }
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

            ui.add(
                egui::TextEdit::singleline(&mut ws.table_filter)
                    .hint_text("Search tables...")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(6.0);

            if ws.meta_job.is_running() {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label(crate::theme::text::muted(palette, "Loading schema"));
                });
            }

            let needle = ws.table_filter.to_lowercase();
            let mut insert: Option<String> = None;
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
                    if ui
                        .add(egui::Label::new(label).sense(egui::Sense::click()))
                        .on_hover_text("Click to query this table")
                        .clicked()
                    {
                        insert = Some(t.name.clone());
                    }
                }
            });
            if let Some(name) = insert {
                ws.sql = format!("SELECT * FROM {} LIMIT 100;", name);
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
            ui.horizontal(|ui| {
                match (&ws.result, ws.elapsed_ms) {
                    (Some(r), Some(ms)) => ui.label(crate::theme::text::muted(
                        palette,
                        format!("{} rows in {} ms", r.rows.len(), ms),
                    )),
                    _ => ui.label(crate::theme::text::muted(palette, "Ready")),
                };
            });
        });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        let ws = app.workspace.as_mut().unwrap();

        ui.horizontal(|ui| {
            let running = ws.query_job.is_running();
            if ui
                .add_enabled(!running && !ws.sql.trim().is_empty(), egui::Button::new("Execute"))
                .on_hover_text("Cmd/Ctrl + Enter")
                .clicked()
            {
                run_now = true;
            }
            if running {
                ui.add(egui::Spinner::new());
            }
            if ui.button("Clear").clicked() {
                ws.sql.clear();
            }
        });
        ui.add_space(6.0);

        let action = sql_editor::show(ui, palette, &mut ws.editor, &ws.schema_index, &mut ws.sql);
        if action == sql_editor::EditorAction::Execute {
            run_now = true;
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        if let Some(err) = ws.last_error.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(palette.destructive, "Query failed:");
                ui.label(err);
            });
        } else if let Some(result) = &ws.result {
            let mut grid = std::mem::take(&mut ws.grid);
            results_grid::show(ui, palette, result, &mut grid);
            ws.grid = grid;
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

    if let Some(db) = switch_to {
        switch_database(app, ui.ctx(), db);
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
    let target = database
        .or_else(|| ws.current_database.clone())
        .or_else(|| ws.databases.first().map(|d| d.name.clone()));

    let Some(d) = driver(app, &connection_id) else {
        return;
    };
    let handle = app.rt.handle().clone();
    let ctx = ctx.clone();
    app.workspace
        .as_mut()
        .unwrap()
        .meta_job
        .start(&handle, &ctx, async move {
            let databases = d.get_databases().await?;
            let schema = target
                .or_else(|| databases.first().map(|db| db.name.clone()))
                .unwrap_or_else(|| "public".to_string());
            // An empty table list is not fatal; the sidebar just stays empty.
            let tables = d.get_tables(&schema).await.unwrap_or_default();
            Ok((databases, tables))
        });
}

/// Refresh the completion index from the loaded tables.
fn rebuild_index(ws: &mut Workspace) {
    ws.schema_index.tables = ws.tables.iter().map(|t| t.name.clone()).collect();
}

fn execute(app: &mut DbHiveApp, ctx: &egui::Context) {
    let Some(ws) = app.workspace.as_mut() else {
        return;
    };
    if ws.query_job.is_running() || ws.sql.trim().is_empty() {
        return;
    }
    let sql = ws.sql.clone();
    let connection_id = ws.connection_id.clone();
    let Some(d) = driver(app, &connection_id) else {
        app.workspace.as_mut().unwrap().last_error =
            Some("Connection is no longer open".to_string());
        return;
    };
    let handle = app.rt.handle().clone();
    let ws = app.workspace.as_mut().unwrap();
    ws.query_job.start(&handle, ctx, async move {
        let started = std::time::Instant::now();
        let out = d.execute_query(&sql).await;
        (out, started.elapsed().as_millis())
    });
}
