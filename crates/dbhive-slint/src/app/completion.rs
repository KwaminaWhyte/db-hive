//! SQL autocomplete: the glue between the editor and
//! `dbhive_ui_model::sql_suggest`.
//!
//! The editor reports where its caret is; this reads the text from the window,
//! asks `suggest` what to offer, and projects the answer into the editor's
//! popup. When a statement refers to a table whose columns are not known yet,
//! the columns are fetched from the driver in the background and the
//! suggestions recomputed when they land. The metadata is the open
//! connection's own table list, so it follows a refresh of the schema.
//!
//! A child module of `app` so it can reach the controller's private state.

use std::collections::HashSet;
use std::rc::Rc;

use dbhive_core::models::DbDriver;
use dbhive_ui_model::sql_suggest::{apply, suggest, ColumnMeta, Metadata, Suggestions, TableMeta};
use slint::{ModelRc, VecModel};

use super::{with_ctx, Ctx};
use crate::{runtime, AppWindow, CompletionItem};

#[derive(Default)]
pub(super) struct CompletionSession {
    /// The connection's tables, with columns filled in as they are fetched.
    meta: Metadata,
    /// Which connection, schema and table list `meta` was built for.
    meta_key: String,
    /// Tables whose columns are being fetched right now, by schema (`None`
    /// is the current one).
    fetching: HashSet<(Option<String>, String)>,
    /// Schemas (or table-holding databases) whose tables are being fetched.
    fetching_schemas: HashSet<String>,
    /// What the popup is showing, if anything.
    shown: Option<Suggestions>,
    selected: usize,
    /// Where the caret was when completion last ran, so a late column fetch
    /// can recompute for the same spot.
    last_cursor: usize,
    /// Byte offset of a word the user dismissed or just completed; it stays
    /// quiet until the caret moves to another word.
    quiet_at: Option<usize>,
}

impl Ctx {
    /// The popup's rows and selection, projected into the window.
    fn project_completion(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let s = self.session.borrow();
        let c = &s.completion;
        let items: Vec<CompletionItem> = c
            .shown
            .as_ref()
            .map(|sg| {
                sg.items
                    .iter()
                    .map(|i| CompletionItem {
                        label: i.label.as_str().into(),
                        detail: i.detail.as_str().into(),
                        kind: i.kind.code().into(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        w.set_workspace_completions(ModelRc::new(VecModel::from(items)));
        w.set_workspace_completion_selected(c.selected as i32);
    }

    /// Forget the connection's metadata, so columns are fetched afresh. Called
    /// when the schema is reloaded and when a workspace opens or closes.
    pub(super) fn completion_reset(&self) {
        self.session.borrow_mut().completion = CompletionSession::default();
        self.project_completion();
    }

    /// Close the popup without touching the metadata.
    pub(super) fn completion_clear(&self) {
        {
            let mut s = self.session.borrow_mut();
            s.completion.shown = None;
            s.completion.selected = 0;
        }
        self.project_completion();
    }

    /// Rebuild the metadata if the connection's table list changed.
    fn completion_sync_metadata(&self) {
        let mut s = self.session.borrow_mut();
        let ws = &s.workspace;
        let key = format!(
            "{}|{}|{}|{}|{}",
            ws.connection_id,
            ws.schema.as_deref().unwrap_or(""),
            ws.tables.len(),
            ws.databases.len(),
            ws.schemas.len()
        );
        if s.completion.meta_key == key {
            return;
        }
        let current_schema = ws.schema.clone();
        let tables: Vec<TableMeta> = ws
            .tables
            .iter()
            .map(|t| TableMeta {
                schema: Some(t.schema.clone()).filter(|x| !x.is_empty()),
                name: t.name.clone(),
                columns: None,
            })
            .collect();
        let meta = Metadata {
            schemas: ws.schemas.clone(),
            tables,
            databases: ws.databases.clone(),
            // MySQL names a table's database in front of it (`shop.orders`);
            // the Postgres family qualifies by schema instead.
            databases_hold_tables: matches!(ws.driver.as_ref(), Some(DbDriver::MySql)),
            current_schema: current_schema.clone(),
            loaded: current_schema.into_iter().collect(),
        };
        s.completion.meta = meta;
        s.completion.meta_key = key;
        s.completion.fetching.clear();
        s.completion.fetching_schemas.clear();
    }

    /// The caret moved to byte offset `cursor` (typing moves it too): work out
    /// what to offer.
    pub(super) fn completion_update(self: &Rc<Self>, cursor: usize) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let is_query = self
            .session
            .borrow()
            .workspace
            .tabs
            .active()
            .map(|t| t.is_query())
            .unwrap_or(false);
        if !is_query {
            self.completion_clear();
            return;
        }
        self.completion_sync_metadata();
        let text = w.get_workspace_editor_text().to_string();
        let found = {
            let mut s = self.session.borrow_mut();
            s.completion.last_cursor = cursor;
            let found = suggest(&text, cursor, &s.completion.meta);
            // A word the user dismissed or just finished stays quiet; moving
            // to a different word lifts that.
            match found.as_ref() {
                Some(f) if s.completion.quiet_at == Some(f.start) => None,
                other => {
                    if other.map(|f| f.start) != s.completion.quiet_at {
                        s.completion.quiet_at = None;
                    }
                    found
                }
            }
        };

        // Columns the statement needs that are not loaded yet, and schemas it
        // names whose tables are not.
        let missing = found
            .as_ref()
            .map(|f| f.missing_columns.clone())
            .unwrap_or_default();
        let missing_schemas = found
            .as_ref()
            .map(|f| f.missing_schemas.clone())
            .unwrap_or_default();

        {
            let mut s = self.session.borrow_mut();
            let same = s
                .completion
                .shown
                .as_ref()
                .zip(found.as_ref())
                .map(|(a, b)| a.items == b.items)
                .unwrap_or(false);
            if !same {
                s.completion.selected = 0;
            }
            // A result with nothing to show (only missing columns) is no popup.
            s.completion.shown = found.filter(|f| !f.items.is_empty());
        }
        self.project_completion();

        for (schema, table) in missing {
            self.completion_fetch_columns(schema, table);
        }
        for schema in missing_schemas {
            self.completion_fetch_schema(&schema);
        }
    }

    /// Fetch one table's columns in the background, then recompute. `schema`
    /// is the qualifier the statement used; `None` is the current schema.
    fn completion_fetch_columns(self: &Rc<Self>, schema: Option<String>, table: String) {
        let key = (schema.clone(), table.clone());
        let (connection_id, schema_arg, meta_key) = {
            let mut s = self.session.borrow_mut();
            if !s.completion.fetching.insert(key.clone()) {
                return;
            }
            (
                s.workspace.connection_id.clone(),
                schema
                    .clone()
                    .or_else(|| s.workspace.schema.clone())
                    .unwrap_or_default(),
                s.completion.meta_key.clone(),
            )
        };
        let Some(d) = self.open_driver(&connection_id) else {
            self.session.borrow_mut().completion.fetching.remove(&key);
            return;
        };
        let worker_table = table.clone();
        runtime::spawn(
            async move {
                d.get_table_schema(&schema_arg, &worker_table)
                    .await
                    .map(|schema| {
                        schema
                            .columns
                            .into_iter()
                            .map(|c| ColumnMeta {
                                name: c.name,
                                data_type: c.data_type,
                            })
                            .collect::<Vec<_>>()
                    })
                    .map_err(|e| e.to_string())
            },
            move |result| {
                with_ctx(|ctx| {
                    let cursor = {
                        let mut s = ctx.session.borrow_mut();
                        // The metadata was rebuilt meanwhile: this answer is for
                        // a connection or schema that is gone.
                        if s.completion.meta_key != meta_key {
                            return;
                        }
                        s.completion.fetching.remove(&key);
                        match result {
                            Ok(cols) => {
                                s.completion
                                    .meta
                                    .set_columns_in(schema.as_deref(), &table, cols)
                            }
                            // Leave the columns unknown. `fetching` no longer
                            // holds the table, so a later keystroke may retry,
                            // but a failure cannot loop on its own.
                            Err(_) => return,
                        }
                        s.completion.last_cursor
                    };
                    ctx.completion_update(cursor);
                })
            },
        );
    }

    /// Fetch a schema's (or table-holding database's) table list, then
    /// recompute. A statement like `FROM audit.` can be completed once it
    /// lands.
    fn completion_fetch_schema(self: &Rc<Self>, schema: &str) {
        let (connection_id, meta_key) = {
            let mut s = self.session.borrow_mut();
            if !s.completion.fetching_schemas.insert(schema.to_string()) {
                return;
            }
            (
                s.workspace.connection_id.clone(),
                s.completion.meta_key.clone(),
            )
        };
        let Some(d) = self.open_driver(&connection_id) else {
            self.session
                .borrow_mut()
                .completion
                .fetching_schemas
                .remove(schema);
            return;
        };
        let name = schema.to_string();
        let worker = name.clone();
        runtime::spawn(
            async move {
                d.get_tables(&worker)
                    .await
                    .map(|tables| tables.into_iter().map(|t| t.name).collect::<Vec<_>>())
                    .map_err(|e| e.to_string())
            },
            move |result| {
                with_ctx(|ctx| {
                    let cursor = {
                        let mut s = ctx.session.borrow_mut();
                        if s.completion.meta_key != meta_key {
                            return;
                        }
                        s.completion.fetching_schemas.remove(&name);
                        match result {
                            Ok(names) => s.completion.meta.add_schema_tables(&name, names),
                            Err(_) => return,
                        }
                        s.completion.last_cursor
                    };
                    ctx.completion_update(cursor);
                })
            },
        );
    }

    pub(super) fn completion_move(&self, delta: i32) {
        {
            let mut s = self.session.borrow_mut();
            let n = s
                .completion
                .shown
                .as_ref()
                .map(|x| x.items.len())
                .unwrap_or(0);
            if n == 0 {
                return;
            }
            let cur = s.completion.selected as i32;
            s.completion.selected = (cur + delta).rem_euclid(n as i32) as usize;
        }
        self.project_completion();
    }

    pub(super) fn completion_accept(self: &Rc<Self>) {
        let choice = self.session.borrow().completion.selected;
        self.completion_pick(choice);
    }

    /// Apply a row of the list: replace the word, move the caret after it, and
    /// keep the popup quiet for that word.
    pub(super) fn completion_pick(self: &Rc<Self>, choice: usize) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let (new_text, at, start) = {
            let s = self.session.borrow();
            let Some(sg) = s.completion.shown.as_ref() else {
                return;
            };
            let text = w.get_workspace_editor_text().to_string();
            let Some((new_text, at)) = apply(&text, sg, choice) else {
                return;
            };
            (new_text, at, sg.start)
        };
        {
            let mut s = self.session.borrow_mut();
            s.completion.shown = None;
            s.completion.selected = 0;
            s.completion.quiet_at = Some(start);
        }
        // Updates the tab's SQL and, through the refresh, the editor's text.
        self.edit_active_sql(new_text);
        w.set_workspace_editor_pending_cursor(at as i32);
        self.project_completion();
    }

    pub(super) fn completion_dismiss(&self) {
        {
            let mut s = self.session.borrow_mut();
            if let Some(sg) = s.completion.shown.take() {
                s.completion.quiet_at = Some(sg.start);
            }
            s.completion.selected = 0;
        }
        self.project_completion();
    }
}

pub(super) fn wire(window: &AppWindow, ctx: &Rc<Ctx>) {
    {
        let ctx = ctx.clone();
        window
            .on_workspace_cursor_moved(move |offset| ctx.completion_update(offset.max(0) as usize));
    }
    {
        let ctx = ctx.clone();
        window.on_workspace_completion_move(move |d| ctx.completion_move(d));
    }
    {
        let ctx = ctx.clone();
        window.on_workspace_completion_accept(move || ctx.completion_accept());
    }
    {
        let ctx = ctx.clone();
        window.on_workspace_completion_dismiss(move || ctx.completion_dismiss());
    }
    {
        let ctx = ctx.clone();
        window.on_workspace_completion_picked(move |i| ctx.completion_pick(i.max(0) as usize));
    }
}
