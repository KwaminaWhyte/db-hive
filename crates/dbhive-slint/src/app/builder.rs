//! Controller glue for the visual query builder.
//!
//! A child module of `app` so it can use the controller's private pieces
//! (`Ctx`, `Session`, `run_tab`, `with_ctx`). The builder's logic lives in
//! `dbhive_ui_model::builder_editor`; this file moves values between that
//! logic, the open connection's driver, and the `BuilderState` global.
//!
//! Two things come from the driver and nothing else does: the columns of a
//! table when it is added, and the schema's foreign keys (for join
//! suggestions). Every identifier in the generated statement goes through the
//! driver's `quote_identifier` and every literal through `escape_string_literal`.

use std::rc::Rc;

use dbhive_core::models::ForeignKeyInfo;
use dbhive_ui_model::builder_editor::{BuilderEditor, Change};
use dbhive_ui_model::query_builder::{rules_for, supports_builder};
use dbhive_ui_model::table_view::sql_qualifier;
use slint::ComponentHandle;

use super::{with_ctx, Ctx};
use crate::{builder_view, runtime, AppWindow, BuilderState, Theme, Tone};

/// The builder's own state. The editor is the statement; the rest is what
/// the controller needs around it.
#[derive(Default)]
pub(super) struct BuilderSession {
    pub(super) editor: BuilderEditor,
    /// The connection the editor's tables belong to. A different connection
    /// starts a fresh builder.
    connection_id: String,
    keys: Vec<ForeignKeyInfo>,
    keys_loading: bool,
    keys_loaded: bool,
    /// Tables whose columns are being read.
    pending: Vec<String>,
    error: String,
    /// What Retry does: read this table's columns again.
    retry_table: Option<String>,
}

impl Ctx {
    /// Open the builder on the current connection.
    pub(super) fn builder_open(self: &Rc<Self>) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let (connection_id, driver) = {
            let s = self.session.borrow();
            (
                s.workspace.connection_id.clone(),
                s.workspace.driver.clone(),
            )
        };
        let Some(driver) = driver.filter(|_| !connection_id.is_empty()) else {
            self.toast("Open a connection first.", Tone::Neutral);
            return;
        };
        if !supports_builder(&driver) {
            self.toast(
                "The visual query builder works with SQL databases only.",
                Tone::Warning,
            );
            return;
        }

        {
            let mut s = self.session.borrow_mut();
            if s.builder.connection_id != connection_id {
                s.builder = BuilderSession {
                    connection_id: connection_id.clone(),
                    ..BuilderSession::default()
                };
            }
            s.builder.error.clear();
        }
        w.global::<BuilderState>().set_visible(true);
        self.builder_refresh(Change::Structure, true);
        self.builder_load_keys();
    }

    pub(super) fn builder_close(&self) {
        if let Some(w) = self.window.upgrade() {
            w.global::<BuilderState>().set_visible(false);
        }
    }

    /// Whether the builder or a table dialog is covering the workspace.
    pub(super) fn builder_open_now(&self) -> bool {
        self.window
            .upgrade()
            .is_some_and(|w| w.global::<BuilderState>().get_visible())
    }

    /// Read the schema's foreign keys once per builder, for join suggestions.
    /// A driver that cannot report them just offers none.
    fn builder_load_keys(self: &Rc<Self>) {
        let (connection_id, schema_arg) = {
            let s = self.session.borrow();
            if s.builder.keys_loaded || s.builder.keys_loading {
                return;
            }
            (
                s.workspace.connection_id.clone(),
                s.workspace.schema.clone().unwrap_or_default(),
            )
        };
        let Some(d) = self.open_driver(&connection_id) else {
            return;
        };
        self.session.borrow_mut().builder.keys_loading = true;
        runtime::spawn(
            async move { d.get_foreign_keys(&schema_arg).await.unwrap_or_default() },
            move |keys| {
                with_ctx(|ctx| {
                    {
                        let mut s = ctx.session.borrow_mut();
                        if s.builder.connection_id != connection_id {
                            return;
                        }
                        s.builder.keys = keys;
                        s.builder.keys_loading = false;
                        s.builder.keys_loaded = true;
                    }
                    ctx.builder_refresh(Change::Structure, false);
                })
            },
        );
    }

    /// Add a table: read its columns, then put it in the statement.
    pub(super) fn builder_add_table(self: &Rc<Self>, name: &str) {
        let (connection_id, schema_arg, driver) = {
            let s = self.session.borrow();
            (
                s.workspace.connection_id.clone(),
                s.workspace.schema.clone().unwrap_or_default(),
                s.workspace.driver.clone(),
            )
        };
        let Some(driver_kind) = driver else { return };
        let Some(d) = self.open_driver(&connection_id) else {
            self.session.borrow_mut().builder.error =
                "This connection is no longer open.".to_string();
            self.builder_refresh(Change::Structure, false);
            return;
        };
        {
            let mut s = self.session.borrow_mut();
            s.builder.error.clear();
            s.builder.retry_table = None;
            s.builder.pending.push(name.to_string());
        }
        self.builder_refresh(Change::Structure, false);

        let qualifier = sql_qualifier(&driver_kind, &schema_arg);
        let table = name.to_string();
        runtime::spawn(
            {
                let table = table.clone();
                async move {
                    d.get_table_schema(&schema_arg, &table)
                        .await
                        .map_err(|e| e.to_string())
                }
            },
            move |result| {
                with_ctx(|ctx| {
                    {
                        let mut s = ctx.session.borrow_mut();
                        if s.builder.connection_id != connection_id {
                            return;
                        }
                        if let Some(p) = s.builder.pending.iter().position(|n| *n == table) {
                            s.builder.pending.remove(p);
                        }
                        match result {
                            Ok(schema) => {
                                let columns = schema
                                    .columns
                                    .iter()
                                    .map(|c| (c.name.clone(), c.data_type.clone()))
                                    .collect();
                                s.builder.editor.add_table(&qualifier, &table, columns);
                            }
                            Err(e) => {
                                s.builder.error =
                                    format!("Could not read the columns of {table}: {e}");
                                s.builder.retry_table = Some(table.clone());
                            }
                        }
                    }
                    ctx.builder_refresh(Change::Structure, false);
                })
            },
        );
    }

    fn builder_retry(self: &Rc<Self>) {
        let table = self.session.borrow_mut().builder.retry_table.take();
        match table {
            Some(t) => self.builder_add_table(&t),
            None => {
                self.session.borrow_mut().builder.error.clear();
                self.builder_refresh(Change::Structure, false);
            }
        }
    }

    /// One edit from the window.
    fn builder_act(self: &Rc<Self>, action: &str, i: i32, j: i32, text: &str) {
        let change = {
            let mut s = self.session.borrow_mut();
            let keys = s.builder.keys.clone();
            s.builder.editor.apply(action, i, j, text, &keys)
        };
        if change == Change::None {
            return;
        }
        // A reset also clears the typed boxes, which are two-way bound.
        self.builder_refresh(change, action == "reset");
    }

    /// Project the builder into the window. `fields` also rewrites the three
    /// typed boxes (open, reset); while typing they belong to the user.
    pub(super) fn builder_refresh(&self, change: Change, fields: bool) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let dark = w.global::<Theme>().get_dark();

        // Copy what the projection needs, then drop the borrow.
        let (connection_id, driver, scope_label, catalog, loading, schema_note) = {
            let s = self.session.borrow();
            let ws = &s.workspace;
            let scope = match &ws.database {
                Some(db) if !db.is_empty() => format!("{} / {}", ws.connection_name, db),
                _ => ws.connection_name.clone(),
            };
            (
                ws.connection_id.clone(),
                ws.driver.clone(),
                scope,
                ws.tables.iter().map(|t| t.name.clone()).collect::<Vec<_>>(),
                ws.loading_tables,
                ws.tables_error.clone(),
            )
        };
        let rules = rules_for(&driver.unwrap_or(dbhive_core::models::DbDriver::Postgres));
        let live = self.open_driver(&connection_id);

        // Everything the window needs, computed under one short borrow so no
        // property setter below can re-enter the controller while it is held.
        let (view, can_use, error, retry, pending_note) = {
            let s = self.session.borrow();
            let b = &s.builder;
            let mut view = match &live {
                Some(d) => b.editor.view(
                    &catalog,
                    &b.keys,
                    &|x: &str| d.quote_identifier(x),
                    &|x: &str| d.escape_string_literal(x),
                    rules,
                ),
                // No driver means no trustworthy quoting: show no statement.
                None => {
                    let mut v = b.editor.view(
                        &catalog,
                        &b.keys,
                        &|x: &str| x.to_string(),
                        &|x: &str| x.to_string(),
                        rules,
                    );
                    v.sql.clear();
                    v
                }
            };
            if live.is_none() && b.error.is_empty() {
                view.warnings
                    .insert(0, "This connection is no longer open.".to_string());
            }
            let can_use =
                !view.sql.is_empty() && view.limit_error.is_empty() && view.offset_error.is_empty();
            let error = if !b.error.is_empty() {
                b.error.clone()
            } else {
                schema_note
            };
            let pending_note = match b.pending.as_slice() {
                [] => String::new(),
                [one] => format!("Reading the columns of {one}…"),
                many => format!("Reading the columns of {} tables…", many.len()),
            };
            (view, can_use, error, b.retry_table.is_some(), pending_note)
        };

        let state = w.global::<BuilderState>();
        state.set_scope_label(scope_label.into());
        state.set_loading(loading);
        state.set_error(error.into());
        state.set_error_retry(retry);
        state.set_pending_note(pending_note.into());

        builder_view::apply(&w, &view, change, can_use, dark);
        if fields {
            builder_view::reset_fields(&w, &view);
        }
    }

    /// The statement as it stands, or `None` when it cannot be used.
    fn builder_statement(&self) -> Option<String> {
        let (connection_id, driver) = {
            let s = self.session.borrow();
            (
                s.workspace.connection_id.clone(),
                s.workspace.driver.clone()?,
            )
        };
        let d = self.open_driver(&connection_id)?;
        let s = self.session.borrow();
        let e = &s.builder.editor;
        let sql = e.sql(
            &|x: &str| d.quote_identifier(x),
            &|x: &str| d.escape_string_literal(x),
            rules_for(&driver),
        );
        e.can_use(&sql).then_some(sql).flatten()
    }

    fn builder_copy(self: &Rc<Self>) {
        let Some(sql) = self.builder_statement() else {
            return;
        };
        if let Some(w) = self.window.upgrade() {
            w.invoke_copy_to_clipboard(sql.as_str().into());
        }
        self.toast("Copied the SQL", Tone::Success);
    }

    /// Hand the statement to a query tab. An empty active query tab is reused;
    /// anything else gets a new one. With `run`, it goes through the normal
    /// run path, so the destructive-statement guard still applies.
    fn builder_send(self: &Rc<Self>, run: bool) {
        let Some(sql) = self.builder_statement() else {
            self.toast("The query is not ready yet.", Tone::Warning);
            return;
        };
        self.builder_close();

        let reuse = {
            let s = self.session.borrow();
            s.workspace
                .tabs
                .active()
                .filter(|t| t.is_query() && t.sql.trim().is_empty())
                .map(|t| t.id)
        };
        let id = match reuse {
            Some(id) => {
                if let Some(tab) = self.session.borrow_mut().workspace.tabs.get_mut(id) {
                    tab.sql = sql;
                }
                self.refresh_workspace();
                id
            }
            None => self.open_query_tab_with(&sql),
        };
        if run {
            self.run_tab(id, true);
        }
    }
}

/// Connect every `BuilderState` callback to the controller.
pub(super) fn wire(window: &AppWindow, ctx: &Rc<Ctx>) {
    let b = window.global::<BuilderState>();

    macro_rules! on {
        ($setter:ident, |$c:ident $(, $a:ident)*| $body:expr) => {{
            let $c = ctx.clone();
            b.$setter(move |$($a),*| {
                let $c = &$c;
                $body
            });
        }};
    }

    on!(on_open_requested, |c| {
        c.dispatch(dbhive_ui_model::command_palette::CommandId::GoVisualQueryBuilder);
    });
    on!(on_close, |c| c.builder_close());
    on!(on_retry, |c| c.builder_retry());
    on!(on_dismiss_error, |c| {
        {
            let mut s = c.session.borrow_mut();
            s.builder.error.clear();
            s.builder.retry_table = None;
        }
        c.builder_refresh(Change::Structure, false);
    });
    on!(on_add_table, |c, name| c.builder_add_table(&name));
    on!(on_act, |c, action, i, j, text| c
        .builder_act(&action, i, j, &text));
    on!(on_copy_sql, |c| c.builder_copy());
    on!(on_use_sql, |c| c.builder_send(false));
    on!(on_run_sql, |c| c.builder_send(true));
}
