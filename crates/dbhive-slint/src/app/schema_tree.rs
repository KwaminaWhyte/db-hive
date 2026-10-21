//! The schema browser's controller: the tree down the side of the workspace.
//!
//! The tree itself (rows, filter, keyboard rules, menus, the drop-table
//! confirmation) is `dbhive_ui_model::schema_tree`; the SQL it generates and
//! the dependency check come from `dbhive_core::service_schema`. This file is
//! glue: start the async loads, land their results, project the model into
//! the `TreeState` global the Slint side draws from, and run what a menu
//! entry or a double click means.
//!
//! One rule keeps the rest of the workspace honest: it still has exactly one
//! "current schema" (`Workspace::schema`, with its `tables`), which the
//! builder, the diagram, autocomplete and the table dialogs all use. The tree
//! can browse any schema, but anything that acts on a schema first makes it
//! the current one (`tree_activate_schema`), and a table tab opened before
//! that is pinned to the schema it was opened in, so it never silently
//! changes meaning.
//!
//! A child module of `app` so it can reach the controller's private state.

use std::collections::HashMap;
use std::rc::Rc;

use dbhive_core::models::{DbDriver, TableInfo};
use dbhive_core::service_procedures::qualify;
use dbhive_core::service_schema::{self, Overview};
use dbhive_ui_model::schema_tree::{
    Action, Caps, Dependents, DropDialog, Load, MenuAction, MenuEntry, NavKey, NodeId, SchemaTree,
};
use dbhive_ui_model::table_view::sql_qualifier;
use dbhive_ui_model::workspace_tabs::TabKind;
use slint::ComponentHandle;

use super::{with_ctx, Ctx};
use crate::{runtime, schema_tree_view, AppWindow, Tone, TreeState};

/// Work waiting for a schema's tables to arrive.
type After = Box<dyn FnOnce(&Rc<Ctx>)>;

#[derive(Default)]
pub(super) struct TreeSession {
    model: SchemaTree,
    caps: Caps,
    driver: Option<DbDriver>,
    /// Bumped whenever the tree is rebuilt, so a load that was in flight for
    /// the old tree is dropped when it lands.
    generation: u64,
    loads: HashMap<String, runtime::Latest>,
    waiting: HashMap<String, Vec<After>>,
    /// The row the open menu was raised on, and its entries.
    menu: Option<(NodeId, Vec<MenuEntry>)>,
    drop: Option<DropDialog>,
    drop_latest: runtime::Latest,
    switching: bool,
    focus_tick: i32,
}

fn schema_of(id: &NodeId) -> Option<&str> {
    match id {
        NodeId::Table { schema, .. } | NodeId::Schema(schema) | NodeId::Note(schema, _) => {
            Some(schema)
        }
        NodeId::Database(_) => None,
    }
}

impl Ctx {
    // -- Lifecycle ---------------------------------------------------------------

    /// A workspace opened: start from an empty tree for this driver.
    pub(super) fn tree_open(&self, driver: &DbDriver) {
        self.tree_reset(Some(driver.clone()));
        self.session.borrow_mut().tree.model.begin_listing();
        self.tree_project();
    }

    /// The workspace closed.
    pub(super) fn tree_close(&self) {
        self.tree_reset(None);
        self.tree_project();
    }

    fn tree_reset(&self, driver: Option<DbDriver>) {
        {
            let mut s = self.session.borrow_mut();
            let generation = s.tree.generation + 1;
            s.tree = TreeSession {
                caps: driver.as_ref().map(Caps::for_driver).unwrap_or_default(),
                driver,
                generation,
                ..TreeSession::default()
            };
        }
        if let Some(w) = self.window.upgrade() {
            let state = w.global::<TreeState>();
            state.set_menu_visible(false);
            state.set_drop_visible(false);
            state.set_filter("".into());
        }
    }

    /// `load_tables` started: keep the rows, show that they are being refreshed.
    pub(super) fn tree_listing_begin(&self) {
        self.session.borrow_mut().tree.model.begin_listing();
        self.tree_project();
    }

    /// `load_tables` could not even start.
    pub(super) fn tree_listing_failed(&self, message: &str) {
        self.session.borrow_mut().tree.model.fail_listing(message);
        self.tree_project();
    }

    /// `load_tables` finished: take the overview and the current schema's tables.
    pub(super) fn tree_seed(
        self: &Rc<Self>,
        overview: Overview,
        schema: &str,
        tables: Result<Vec<TableInfo>, String>,
    ) {
        let reload = {
            let mut s = self.session.borrow_mut();
            s.tree.switching = false;
            let m = &mut s.tree.model;
            m.apply_overview(&overview, schema);
            m.set_tables(schema, tables);
            m.expanded_unloaded()
        };
        self.tree_project();
        for name in reload {
            self.tree_load_schema(&name);
        }
        self.tree_search_loads();
    }

    pub(super) fn tree_project(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let (view, can_create) = {
            let s = self.session.borrow();
            (s.tree.model.view(), s.tree.caps.ddl)
        };
        schema_tree_view::apply(&w, &view, can_create);
    }

    // -- Loading a schema -----------------------------------------------------------

    /// Fetch one schema's tables. A newer fetch of the same schema supersedes
    /// this one.
    fn tree_load_schema(self: &Rc<Self>, schema: &str) {
        let (connection_id, generation, ticket) = {
            let mut s = self.session.borrow_mut();
            let ticket = s.tree.loads.entry(schema.to_string()).or_default().begin();
            s.tree.model.begin_schema(schema);
            (s.workspace.connection_id.clone(), s.tree.generation, ticket)
        };
        self.tree_project();
        let Some(d) = self.open_driver(&connection_id) else {
            self.tree_schema_loaded(
                generation,
                ticket,
                schema,
                Err("This connection is no longer open.".to_string()),
            );
            return;
        };
        let name = schema.to_string();
        runtime::spawn(
            {
                let name = name.clone();
                async move { d.get_tables(&name).await.map_err(|e| e.to_string()) }
            },
            move |result| {
                with_ctx(|ctx| ctx.tree_schema_loaded(generation, ticket, &name, result))
            },
        );
    }

    fn tree_schema_loaded(
        self: &Rc<Self>,
        generation: u64,
        ticket: runtime::Ticket,
        schema: &str,
        result: Result<Vec<TableInfo>, String>,
    ) {
        if !ticket.is_current() {
            return;
        }
        let (waiting, is_current_schema) = {
            let mut s = self.session.borrow_mut();
            if s.tree.generation != generation {
                return;
            }
            let is_current = s.workspace.schema.as_deref() == Some(schema);
            // The current schema's list also feeds the rest of the workspace.
            if let (true, Ok(tables)) = (is_current, &result) {
                s.workspace.tables = tables.clone();
            }
            s.tree.model.set_tables(schema, result.clone());
            (s.tree.waiting.remove(schema).unwrap_or_default(), is_current)
        };
        if is_current_schema {
            self.completion_reset();
            self.refresh_workspace();
        }
        self.tree_project();
        match result {
            Ok(_) => {
                for after in waiting {
                    after(self);
                }
            }
            // The row only says it failed; the reason goes here.
            Err(e) => self.toast(format!("Could not list tables in {schema}: {e}"), Tone::Danger),
        }
    }

    /// Run `then` once `schema`'s tables are known, fetching them if need be.
    fn tree_with_schema(self: &Rc<Self>, schema: &str, then: After) {
        let load = self
            .session
            .borrow()
            .tree
            .model
            .schema(schema)
            .map(|n| n.load.clone());
        match load {
            Some(Load::Ready) => then(self),
            Some(Load::Loading) => {
                self.session
                    .borrow_mut()
                    .tree
                    .waiting
                    .entry(schema.to_string())
                    .or_default()
                    .push(then);
            }
            _ => {
                self.session
                    .borrow_mut()
                    .tree
                    .waiting
                    .entry(schema.to_string())
                    .or_default()
                    .push(then);
                self.tree_load_schema(schema);
            }
        }
    }

    /// Make `schema` the workspace's current one, which the builder, the
    /// diagram, autocomplete and the table dialogs act on. Table tabs already
    /// open are pinned to the schema they came from first.
    fn tree_activate_schema(self: &Rc<Self>, schema: &str) {
        let changed = {
            let mut s = self.session.borrow_mut();
            let previous = s.workspace.schema.clone();
            if previous.as_deref() == Some(schema) {
                false
            } else {
                if let Some(old) = previous {
                    let ids: Vec<u64> = s
                        .workspace
                        .tabs
                        .tabs()
                        .iter()
                        .filter(|t| !t.is_query() && t.schema.is_none())
                        .map(|t| t.id)
                        .collect();
                    for id in ids {
                        if let Some(t) = s.workspace.tabs.get_mut(id) {
                            t.schema = Some(old.clone());
                        }
                    }
                }
                let tables = s
                    .tree
                    .model
                    .schema(schema)
                    .map(|n| n.tables.clone())
                    .unwrap_or_default();
                s.workspace.schema = Some(schema.to_string());
                s.workspace.tables = tables;
                s.tree.model.set_active_schema(schema);
                true
            }
        };
        if changed {
            self.completion_reset();
            self.invalidate_erd();
            self.refresh_workspace();
            self.tree_project();
        }
    }

    // -- Opening a table --------------------------------------------------------------

    fn tree_open_table(self: &Rc<Self>, schema: &str, table: &str) {
        let (connection_id, pinned) = {
            let s = self.session.borrow();
            let ws = &s.workspace;
            // A tab in the current schema (or in a driver with no schema
            // level) is the plain kind; any other is pinned to its schema.
            let plain = ws.schema.as_deref() == Some(schema) || !s.tree.model.schema_level();
            (ws.connection_id.clone(), (!plain).then(|| schema.to_string()))
        };
        if self.open_driver(&connection_id).is_none() {
            self.toast("This connection is no longer open.", Tone::Danger);
            return;
        }
        let (id, created) = self.session.borrow_mut().workspace.tabs.open_table_in(
            pinned.as_deref(),
            table,
            String::new(),
        );
        self.refresh_workspace();
        if created {
            self.inspector_open(id);
        }
    }

    // -- Interaction -------------------------------------------------------------------

    fn tree_act(self: &Rc<Self>, action: Action) {
        match action {
            Action::None => {}
            Action::Redraw => self.tree_project(),
            Action::OpenTable { schema, table } => {
                self.tree_project();
                self.tree_open_table(&schema, &table);
            }
            Action::SwitchDatabase(db) => self.tree_switch_database(db),
            Action::LoadSchema(schema) => self.tree_load_schema(&schema),
            Action::CreateTable(schema) => self.tree_create_table_in(&schema),
        }
    }

    fn tree_id_at(&self, row: i32) -> Option<NodeId> {
        usize::try_from(row)
            .ok()
            .and_then(|i| self.session.borrow().tree.model.id_at(i))
    }

    fn tree_click(self: &Rc<Self>, row: i32) {
        let Some(id) = self.tree_id_at(row) else {
            return;
        };
        let action = self.session.borrow_mut().tree.model.click(&id);
        self.tree_act(action);
    }

    fn tree_double_click(self: &Rc<Self>, row: i32) {
        let Some(id) = self.tree_id_at(row) else {
            return;
        };
        let action = self.session.borrow_mut().tree.model.open(&id);
        self.tree_act(action);
    }

    fn tree_toggle(self: &Rc<Self>, row: i32) {
        let Some(id) = self.tree_id_at(row) else {
            return;
        };
        let action = {
            let mut s = self.session.borrow_mut();
            s.tree.model.set_cursor(Some(id.clone()));
            s.tree.model.toggle(&id)
        };
        self.tree_project();
        self.tree_act(action);
    }

    fn tree_nav(self: &Rc<Self>, code: i32) {
        let key = match code {
            0 => NavKey::Up,
            1 => NavKey::Down,
            2 => NavKey::Home,
            3 => NavKey::End,
            4 => NavKey::Left,
            5 => NavKey::Right,
            _ => NavKey::Enter,
        };
        let action = self.session.borrow_mut().tree.model.key(key);
        self.tree_act(action);
    }

    fn tree_filter(self: &Rc<Self>, text: &str) {
        self.session.borrow_mut().tree.model.set_filter(text);
        self.tree_project();
        self.tree_search_loads();
    }

    fn tree_clear_filter(self: &Rc<Self>) {
        if let Some(w) = self.window.upgrade() {
            w.global::<TreeState>().set_filter("".into());
        }
        self.tree_filter("");
    }

    /// A live search sees only loaded schemas, so it loads the rest.
    fn tree_search_loads(self: &Rc<Self>) {
        let names = self.session.borrow().tree.model.schemas_to_load_for_search();
        for name in names {
            self.tree_load_schema(&name);
        }
    }

    /// The manual refresh button: everything again.
    fn tree_refresh_all(self: &Rc<Self>) {
        self.session.borrow_mut().tree.model.invalidate_all();
        self.load_tables();
    }

    fn tree_refresh_schema(self: &Rc<Self>, schema: &str) {
        let is_current = self.session.borrow().workspace.schema.as_deref() == Some(schema);
        if is_current {
            // Re-reads the overview too, which is cheap.
            self.load_tables();
        } else {
            self.session.borrow_mut().tree.model.invalidate_schema(schema);
            self.tree_load_schema(schema);
        }
    }

    fn tree_retry(self: &Rc<Self>) {
        self.load_tables();
    }

    // -- Context menu --------------------------------------------------------------------

    fn tree_context(self: &Rc<Self>, row: i32, x: f32, y: f32) {
        let Some(id) = self.tree_id_at(row) else {
            return;
        };
        let entries = {
            let mut s = self.session.borrow_mut();
            s.tree.model.set_cursor(Some(id.clone()));
            let caps = s.tree.caps;
            let entries = s.tree.model.menu_for(&id, caps);
            s.tree.menu = Some((id, entries.clone()));
            entries
        };
        self.tree_project();
        if let Some(w) = self.window.upgrade() {
            schema_tree_view::show_menu(&w, &entries, x, y);
        }
    }

    /// The menu key: open the menu on the highlighted row.
    fn tree_context_key(self: &Rc<Self>, x: f32, y: f32) {
        let row = self.session.borrow().tree.model.view().cursor;
        if let Some(i) = row {
            self.tree_context(i as i32, x, y);
        }
    }

    fn tree_menu_dismissed(self: &Rc<Self>) {
        let tick = {
            let mut s = self.session.borrow_mut();
            s.tree.menu = None;
            s.tree.focus_tick += 1;
            s.tree.focus_tick
        };
        if let Some(w) = self.window.upgrade() {
            w.global::<TreeState>().set_focus_tick(tick);
        }
    }

    fn tree_menu_chosen(self: &Rc<Self>, index: i32) {
        let picked = {
            let mut s = self.session.borrow_mut();
            s.tree.menu.take().and_then(|(id, entries)| {
                usize::try_from(index)
                    .ok()
                    .and_then(|i| entries.get(i).cloned())
                    .filter(|e| e.enabled)
                    .map(|e| (id, e.action))
            })
        };
        if let Some((id, action)) = picked {
            self.tree_run(action, id);
        }
    }

    fn tree_run(self: &Rc<Self>, action: MenuAction, id: NodeId) {
        let schema = schema_of(&id).unwrap_or_default().to_string();
        let table = match &id {
            NodeId::Table { name, .. } => Some(name.clone()),
            _ => None,
        };
        match (action, table) {
            (MenuAction::ViewData, Some(t)) => self.tree_open_table(&schema, &t),
            (MenuAction::GenerateSelect, Some(t)) => self.tree_generate(&schema, &t, false),
            (MenuAction::GenerateInsert, Some(t)) => self.tree_generate(&schema, &t, true),
            (MenuAction::CopyName, Some(t)) => self.tree_copy_name(&schema, &t),
            (MenuAction::EditTable, Some(t)) => {
                let s2 = schema.clone();
                self.tree_with_schema(
                    &schema,
                    Box::new(move |ctx| {
                        ctx.tree_activate_schema(&s2);
                        ctx.table_dialog_open_edit(&t);
                    }),
                );
            }
            (MenuAction::DropTable, Some(t)) => self.tree_start_drop(&schema, &t),
            (MenuAction::Refresh, _) => match id {
                NodeId::Database(_) => self.tree_refresh_all(),
                _ => self.tree_refresh_schema(&schema),
            },
            (MenuAction::CreateTable, _) => self.tree_create_table_in(&schema),
            (MenuAction::SwitchDatabase, _) => {
                if let NodeId::Database(db) = id {
                    self.tree_switch_database(db);
                }
            }
            (MenuAction::CreateDatabase, _) => self.tree_create_database_open(),
            _ => {}
        }
    }

    /// Create a table in `schema`, which becomes the current one.
    fn tree_create_table_in(self: &Rc<Self>, schema: &str) {
        let s2 = schema.to_string();
        self.tree_with_schema(
            schema,
            Box::new(move |ctx| {
                ctx.tree_activate_schema(&s2);
                ctx.table_dialog_open_create();
            }),
        );
    }

    /// The header's "new table": in the schema the highlighted row belongs to,
    /// else the current one.
    fn tree_create_table_here(self: &Rc<Self>) {
        let schema = {
            let s = self.session.borrow();
            s.tree
                .model
                .cursor()
                .and_then(schema_of)
                .map(str::to_string)
                .or_else(|| s.workspace.schema.clone())
        };
        match schema {
            Some(schema) => self.tree_create_table_in(&schema),
            None => self.toast("Tables are still loading. Try again in a moment.", Tone::Neutral),
        }
    }

    // -- Generated SQL and copy ------------------------------------------------------------

    /// What `driver` and the live connection need to write SQL for `schema`.
    fn tree_sql_target(
        &self,
        schema: &str,
    ) -> Option<(std::sync::Arc<dyn dbhive_core::drivers::DatabaseDriver>, DbDriver, String)> {
        let (connection_id, driver) = {
            let s = self.session.borrow();
            (s.workspace.connection_id.clone(), s.workspace.driver.clone()?)
        };
        let d = self.open_driver(&connection_id)?;
        let qualifier = sql_qualifier(&driver, schema);
        Some((d, driver, qualifier))
    }

    fn tree_generate(self: &Rc<Self>, schema: &str, table: &str, insert: bool) {
        let Some((d, driver, qualifier)) = self.tree_sql_target(schema) else {
            self.toast("This connection is no longer open.", Tone::Danger);
            return;
        };
        if !insert {
            let sql = service_schema::generate_select(
                &driver,
                &|s: &str| d.quote_identifier(s),
                &qualifier,
                table,
            );
            self.tree_put_sql(sql, "SELECT");
            return;
        }
        let (schema, table) = (schema.to_string(), table.to_string());
        runtime::spawn(
            {
                let table = table.clone();
                async move {
                    let ts = d
                        .get_table_schema(&schema, &table)
                        .await
                        .map_err(|e| e.to_string())?;
                    Ok::<String, String>(service_schema::generate_insert(
                        &driver,
                        &|s: &str| d.quote_identifier(s),
                        &qualifier,
                        &table,
                        &ts.columns,
                    ))
                }
            },
            move |result| {
                with_ctx(|ctx| match result {
                    Ok(sql) => ctx.tree_put_sql(sql, "INSERT"),
                    Err(e) => ctx.toast(format!("Could not read {table}: {e}"), Tone::Danger),
                })
            },
        );
    }

    /// Generated SQL goes into a new query tab and is never run.
    fn tree_put_sql(self: &Rc<Self>, sql: String, what: &str) {
        self.completion_clear();
        self.session.borrow_mut().workspace.tabs.open_query(sql);
        self.refresh_workspace();
        self.toast(
            format!("{what} added to a new query tab. Nothing was run."),
            Tone::Neutral,
        );
    }

    fn tree_copy_name(self: &Rc<Self>, schema: &str, table: &str) {
        let Some((d, _, qualifier)) = self.tree_sql_target(schema) else {
            self.toast("This connection is no longer open.", Tone::Danger);
            return;
        };
        let quoted = qualify(&|s: &str| d.quote_identifier(s), &qualifier, table);
        if let Some(w) = self.window.upgrade() {
            w.invoke_copy_to_clipboard(quoted.as_str().into());
        }
        self.toast(format!("Copied {quoted}"), Tone::Success);
    }

    // -- Switching database -----------------------------------------------------------------------

    fn tree_switch_database(self: &Rc<Self>, db: String) {
        let (connection_id, allowed, busy, dirty) = {
            let s = self.session.borrow();
            let dirty = s
                .workspace
                .tabs
                .tabs()
                .iter()
                .find(|t| t.inspector.as_ref().is_some_and(|v| v.edit.has_pending()))
                .map(|t| t.title.clone());
            (
                s.workspace.connection_id.clone(),
                s.tree.caps.switch_database,
                s.tree.switching,
                dirty,
            )
        };
        if !allowed {
            self.toast("This database cannot switch databases from here.", Tone::Warning);
            return;
        }
        if busy {
            return;
        }
        if let Some(title) = dirty {
            self.toast(
                format!(
                    "\"{title}\" has uncommitted row edits. Commit or discard them before switching database."
                ),
                Tone::Warning,
            );
            return;
        }
        {
            let mut s = self.session.borrow_mut();
            s.tree.switching = true;
            s.tree.model.begin_listing();
        }
        self.tree_project();
        let state = self.state.clone();
        let target = db.clone();
        runtime::spawn(
            async move {
                dbhive_core::service::switch_database(&state, &connection_id, &target)
                    .await
                    .map_err(|e| e.to_string())
            },
            move |result| with_ctx(|ctx| ctx.tree_switched(db, result)),
        );
    }

    fn tree_switched(self: &Rc<Self>, db: String, result: Result<String, String>) {
        match result {
            Ok(_) => {
                {
                    let mut s = self.session.borrow_mut();
                    // Open tables belong to the database we left.
                    let ids: Vec<u64> = s
                        .workspace
                        .tabs
                        .tabs()
                        .iter()
                        .filter(|t| !t.is_query())
                        .map(|t| t.id)
                        .collect();
                    for id in ids {
                        s.workspace.tabs.close(id);
                    }
                    s.workspace.database = Some(db.clone());
                    s.workspace.schema = None;
                    s.workspace.tables.clear();
                    let driver = s.workspace.driver.clone();
                    drop(s);
                    if let Some(driver) = driver {
                        self.tree_reset(Some(driver));
                    }
                }
                self.session.borrow_mut().tree.switching = false;
                self.completion_reset();
                self.invalidate_erd();
                self.refresh_workspace();
                self.load_tables();
                self.toast(format!("Switched to {db}"), Tone::Success);
            }
            Err(e) => {
                {
                    let mut s = self.session.borrow_mut();
                    s.tree.switching = false;
                    s.tree.model.finish_listing();
                }
                self.tree_project();
                self.toast(format!("Could not switch to {db}: {e}"), Tone::Danger);
            }
        }
    }

    // -- Dropping a table ------------------------------------------------------------------------------

    fn tree_start_drop(self: &Rc<Self>, schema: &str, table: &str) {
        let Some((d, driver, qualifier)) = self.tree_sql_target(schema) else {
            self.toast("This connection is no longer open.", Tone::Danger);
            return;
        };
        let postgres = driver.is_postgres_compatible();
        let (ticket, generation, scan) = {
            let mut s = self.session.borrow_mut();
            s.tree.drop = Some(DropDialog::new(schema, &qualifier, table, postgres));
            let scan = if postgres {
                s.tree.model.schema_names()
            } else {
                vec![schema.to_string()]
            };
            (s.tree.drop_latest.begin(), s.tree.generation, scan)
        };
        self.tree_drop_refresh();
        let (schema, table) = (schema.to_string(), table.to_string());
        runtime::spawn(
            async move {
                let mut found = service_schema::find_dependents(&*d, &scan, &schema, &table)
                    .await
                    .map_err(|e| e.to_string())?;
                // Outside Postgres the "schema" is the database (or a file
                // path for SQLite): everything is in the same one, so naming
                // it only clutters the warning.
                if !postgres {
                    for t in &mut found {
                        t.schema.clear();
                    }
                }
                Ok::<_, String>(found)
            },
            move |result| {
                with_ctx(|ctx| {
                    if !ticket.is_current() {
                        return;
                    }
                    {
                        let mut s = ctx.session.borrow_mut();
                        if s.tree.generation != generation {
                            return;
                        }
                        let Some(dialog) = s.tree.drop.as_mut() else {
                            return;
                        };
                        dialog.dependents = match result {
                            Ok(list) => Dependents::Ready(list),
                            Err(e) => Dependents::Failed(e),
                        };
                    }
                    ctx.tree_drop_refresh();
                })
            },
        );
    }

    fn tree_drop_refresh(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let s = self.session.borrow();
        let Some(dialog) = s.tree.drop.as_ref() else {
            w.global::<TreeState>().set_drop_visible(false);
            return;
        };
        let sql = s
            .tree
            .driver
            .as_ref()
            .and_then(|driver| {
                service_schema::drop_table_sql(
                    driver,
                    &dialog.qualifier,
                    &dialog.table,
                    dialog.cascade && dialog.cascade_offered(),
                )
                .ok()
            })
            .map(|v| v.join("\n"))
            .unwrap_or_default();
        schema_tree_view::apply_drop(&w, dialog, &sql);
    }

    fn tree_drop_typed(&self, text: &str) {
        if let Some(d) = self.session.borrow_mut().tree.drop.as_mut() {
            d.typed = text.to_string();
            d.error.clear();
        }
        self.tree_drop_refresh();
    }

    fn tree_drop_toggle_cascade(&self) {
        if let Some(d) = self.session.borrow_mut().tree.drop.as_mut() {
            if d.cascade_offered() && !d.running {
                d.cascade = !d.cascade;
            }
        }
        self.tree_drop_refresh();
    }

    fn tree_drop_cancel(&self) {
        {
            let mut s = self.session.borrow_mut();
            s.tree.drop_latest.begin();
            s.tree.drop = None;
        }
        if let Some(w) = self.window.upgrade() {
            w.global::<TreeState>().set_drop_visible(false);
        }
    }

    fn tree_drop_confirm(self: &Rc<Self>) {
        let (table, qualifier, cascade, driver, connection_id) = {
            let mut s = self.session.borrow_mut();
            let driver = s.tree.driver.clone();
            let connection_id = s.workspace.connection_id.clone();
            let Some(d) = s.tree.drop.as_mut() else {
                return;
            };
            if !d.can_drop() {
                return;
            }
            d.running = true;
            d.error.clear();
            (
                d.table.clone(),
                d.qualifier.clone(),
                d.cascade && d.cascade_offered(),
                driver,
                connection_id,
            )
        };
        self.tree_drop_refresh();
        let (Some(driver), Some(conn)) = (driver, self.open_driver(&connection_id)) else {
            self.tree_drop_failed("This connection is no longer open.".to_string());
            return;
        };
        let (ticket, schema) = {
            let s = self.session.borrow();
            let schema = s.tree.drop.as_ref().map(|d| d.schema.clone()).unwrap_or_default();
            (s.tree.drop_latest.begin(), schema)
        };
        let dropped = table.clone();
        runtime::spawn(
            async move {
                service_schema::drop_table(&*conn, &driver, &qualifier, &table, cascade)
                    .await
                    .map_err(|e| e.to_string())
            },
            move |result| {
                with_ctx(|ctx| {
                    if !ticket.is_current() {
                        return;
                    }
                    match result {
                        Ok(_) => ctx.tree_dropped(&schema, &dropped),
                        Err(e) => ctx.tree_drop_failed(e),
                    }
                })
            },
        );
    }

    fn tree_drop_failed(&self, message: String) {
        if let Some(d) = self.session.borrow_mut().tree.drop.as_mut() {
            d.running = false;
            d.error = message;
        }
        self.tree_drop_refresh();
    }

    fn tree_dropped(self: &Rc<Self>, schema: &str, table: &str) {
        {
            let mut s = self.session.borrow_mut();
            s.tree.drop = None;
            // The table's tabs have nothing left to show.
            let current = s.workspace.schema.clone();
            let ids: Vec<u64> = s
                .workspace
                .tabs
                .tabs()
                .iter()
                .filter(|t| {
                    matches!(&t.kind, TabKind::Table(n) if n == table)
                        && t.schema.as_deref().or(current.as_deref()).unwrap_or("") == schema
                })
                .map(|t| t.id)
                .collect();
            for id in ids {
                s.workspace.tabs.close(id);
            }
        }
        if let Some(w) = self.window.upgrade() {
            w.global::<TreeState>().set_drop_visible(false);
        }
        self.refresh_workspace();
        self.tree_refresh_schema(schema);
        self.toast(format!("Dropped table {table}"), Tone::Success);
    }

    // -- Create database (stretch) -----------------------------------------------------------------------

    fn tree_create_database_open(self: &Rc<Self>) {
        self.toast("Create Database is not available yet.", Tone::Neutral);
    }
}

/// Connect the tree's intents to the controller.
pub(super) fn wire(window: &AppWindow, ctx: &Rc<Ctx>) {
    let s = window.global::<TreeState>();

    macro_rules! on {
        ($setter:ident, |$ctx:ident $(, $arg:ident)*| $body:expr) => {{
            let $ctx = ctx.clone();
            s.$setter(move |$($arg),*| $body);
        }};
    }

    on!(on_clicked, |ctx, i| ctx.tree_click(i));
    on!(on_double_clicked, |ctx, i| ctx.tree_double_click(i));
    on!(on_toggled, |ctx, i| ctx.tree_toggle(i));
    on!(on_context, |ctx, i, x, y| ctx.tree_context(i, x, y));
    on!(on_context_key, |ctx, x, y| ctx.tree_context_key(x, y));
    on!(on_nav, |ctx, code| ctx.tree_nav(code));
    on!(on_filter_edited, |ctx, t| ctx.tree_filter(&t));
    on!(on_clear_filter, |ctx| ctx.tree_clear_filter());
    on!(on_refresh, |ctx| ctx.tree_refresh_all());
    on!(on_retry, |ctx| ctx.tree_retry());
    on!(on_create_table, |ctx| ctx.tree_create_table_here());
    on!(on_menu_chosen, |ctx, i| ctx.tree_menu_chosen(i));
    on!(on_menu_dismissed, |ctx| ctx.tree_menu_dismissed());
    on!(on_drop_typed_edited, |ctx, t| ctx.tree_drop_typed(&t));
    on!(on_drop_cascade_toggled, |ctx| ctx.tree_drop_toggle_cascade());
    on!(on_drop_confirm, |ctx| ctx.tree_drop_confirm());
    on!(on_drop_cancel, |ctx| ctx.tree_drop_cancel());
}
