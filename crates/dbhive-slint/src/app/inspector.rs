//! The table inspector's controller: a table tab's Data and Structure views.
//!
//! The state machine lives in `dbhive_ui_model::table_view` (paging, filters,
//! sort, selection, stale-load refusal, what a copy produces) and the SQL in
//! `dbhive_ui_model::table_inspector`. This file is glue: start the async
//! loads, land their results by sequence number, and project the view into the
//! `InspectorBridge` global the Slint side draws from.
//!
//! A child module of `app` so it can reach the controller's private state.

use std::rc::Rc;

use dbhive_core::drivers::QueryResult;
use dbhive_core::models::DbDriver;
use dbhive_ui_model::table_inspector::{page_range_label, rules_for, TableMeta};
use dbhive_ui_model::table_view::{
    column_rows, copy_text, filter_op_labels, index_rows, page_size_at, page_size_label,
    page_size_labels, pager_state, relation_rows, row_json, selection_label, sql_qualifier,
    total_label, CopyKind, Meta, TableView, View,
};
use dbhive_ui_model::workspace_tabs::{Tab, TabKind};
use slint::{ComponentHandle, ModelRc, VecModel};

use super::{with_ctx, Ctx, Workspace};
use crate::{
    runtime, AppWindow, ColumnRowItem, FilterRowItem, IndexRowItem, InspectorBridge,
    RelationRowItem, Tone,
};

/// Everything a load needs to know about the table behind a tab.
struct Target {
    connection_id: String,
    driver: DbDriver,
    /// What the driver's metadata calls take: a schema, or the database.
    schema_arg: String,
    /// What generated SQL puts in front of the table name.
    qualifier: String,
    table: String,
}

impl Ctx {
    fn inspector_target(&self, id: u64) -> Option<Target> {
        let s = self.session.borrow();
        let ws = &s.workspace;
        let tab = ws.tabs.get(id)?;
        let TabKind::Table(table) = &tab.kind else {
            return None;
        };
        let driver = ws.driver.clone()?;
        let schema_arg = ws.schema.clone().unwrap_or_default();
        Some(Target {
            connection_id: ws.connection_id.clone(),
            qualifier: sql_qualifier(&driver, &schema_arg),
            driver,
            schema_arg,
            table: table.clone(),
        })
    }

    /// Put a table tab into a failed state without running anything.
    fn inspector_fail(&self, id: u64, message: &str) {
        if let Some(tab) = self.session.borrow_mut().workspace.tabs.get_mut(id) {
            tab.running = false;
            tab.error = Some(message.to_string());
        }
        self.refresh_workspace();
    }

    /// A freshly opened table tab: load its schema, then its first page.
    pub(super) fn inspector_open(self: &Rc<Self>, id: u64) {
        self.inspector_load_meta(id, true);
    }

    /// Reload the schema, then (with `then_data`) the first-page data and the
    /// count. Paging needs the primary key for a stable order, so the data
    /// waits for the schema, and goes ahead unordered if the schema fails.
    fn inspector_load_meta(self: &Rc<Self>, id: u64, then_data: bool) {
        let Some(t) = self.inspector_target(id) else {
            return;
        };
        let Some(d) = self.open_driver(&t.connection_id) else {
            self.inspector_fail(id, "This connection is no longer open.");
            return;
        };
        let seq = {
            let mut s = self.session.borrow_mut();
            let tabs = &mut s.workspace.tabs;
            if then_data {
                // Shows "loading" while the schema is on its way.
                tabs.restart_run(id);
            }
            let Some(v) = tabs.get_mut(id).and_then(|t| t.inspector.as_mut()) else {
                return;
            };
            v.begin_meta()
        };
        self.refresh_workspace();

        let (schema_arg, table) = (t.schema_arg, t.table);
        runtime::spawn(
            async move {
                let schema = d
                    .get_table_schema(&schema_arg, &table)
                    .await
                    .map_err(|e| e.to_string())?;
                // Relations are a nice-to-have; a driver that cannot report
                // them must not cost the user the columns and indexes.
                let foreign_keys = d.get_foreign_keys(&schema_arg).await.unwrap_or_default();
                Ok::<TableMeta, String>(TableMeta {
                    schema,
                    foreign_keys,
                })
            },
            move |out| {
                with_ctx(|ctx| {
                    let landed = ctx
                        .session
                        .borrow_mut()
                        .workspace
                        .tabs
                        .get_mut(id)
                        .and_then(|t| t.inspector.as_mut())
                        .map(|v| v.finish_meta(seq, out))
                        .unwrap_or(false);
                    if !landed {
                        return;
                    }
                    if then_data {
                        ctx.inspector_fetch(id, true);
                    } else {
                        ctx.refresh_workspace();
                    }
                })
            },
        );
    }

    /// Fetch the current page (and, with `recount`, the row count for the
    /// applied filters). A newer fetch supersedes one still in flight.
    fn inspector_fetch(self: &Rc<Self>, id: u64, recount: bool) {
        let Some(t) = self.inspector_target(id) else {
            return;
        };
        let Some(d) = self.open_driver(&t.connection_id) else {
            self.inspector_fail(id, "This connection is no longer open.");
            return;
        };
        let rules = rules_for(&t.driver);

        let (data_sql, count) = {
            let mut s = self.session.borrow_mut();
            let Some(v) = s
                .workspace
                .tabs
                .get_mut(id)
                .and_then(|t| t.inspector.as_mut())
            else {
                return;
            };
            let quote = |x: &str| d.quote_identifier(x);
            let escape = |x: &str| d.escape_string_literal(x);
            let data_sql = v.data_sql(&quote, &escape, rules, &t.qualifier, &t.table);
            let count = recount.then(|| {
                let sql = v.count_sql(&quote, &escape, rules, &t.qualifier, &t.table);
                (sql, v.begin_count())
            });
            (data_sql, count)
        };
        let Some(seq) = self.session.borrow_mut().workspace.tabs.restart_run(id) else {
            return;
        };
        self.refresh_workspace();

        let page_driver = d.clone();
        runtime::spawn(
            async move {
                let started = std::time::Instant::now();
                let out = page_driver.execute_query(&data_sql).await;
                (out, started.elapsed().as_millis() as u64)
            },
            move |(out, elapsed)| {
                with_ctx(|ctx| {
                    let landed = ctx.session.borrow_mut().workspace.tabs.finish_run(
                        id,
                        seq,
                        out.map_err(|e| e.to_string()),
                        elapsed,
                    );
                    if landed {
                        ctx.refresh_workspace();
                    }
                })
            },
        );

        if let Some((count_sql, count_seq)) = count {
            runtime::spawn(
                async move {
                    let r = d
                        .execute_query(&count_sql)
                        .await
                        .map_err(|e| e.to_string())?;
                    Ok::<Option<u64>, String>(dbhive_ui_model::table_inspector::parse_count(&r))
                },
                move |out| {
                    with_ctx(|ctx| {
                        let landed = ctx
                            .session
                            .borrow_mut()
                            .workspace
                            .tabs
                            .get_mut(id)
                            .and_then(|t| t.inspector.as_mut())
                            .map(|v| v.finish_count(count_seq, out))
                            .unwrap_or(false);
                        if landed {
                            ctx.refresh_workspace();
                        }
                    })
                },
            );
        }
    }

    /// Run `f` on the active tab's view and the page it is showing.
    fn with_active_view<R>(
        &self,
        f: impl FnOnce(&mut TableView, Option<&QueryResult>) -> R,
    ) -> Option<(u64, R)> {
        let mut s = self.session.borrow_mut();
        let tab = s.workspace.tabs.active_mut()?;
        let id = tab.id;
        let Tab {
            inspector, result, ..
        } = tab;
        let v = inspector.as_mut()?;
        Some((id, f(v, result.as_ref())))
    }

    /// Re-project just the inspector, for edits that must not rebuild the
    /// grid (the grid's scroll position would reset).
    fn refresh_inspector(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let s = self.session.borrow();
        if let Some(tab) = s.workspace.tabs.active() {
            if tab.inspector.is_some() {
                project(&w, &s.workspace, tab);
            }
        }
    }

    fn inspector_set_view(&self, index: i32) {
        self.with_active_view(|v, _| {
            v.view = if index == 1 {
                View::Structure
            } else {
                View::Data
            };
        });
        self.refresh_inspector();
    }

    fn inspector_reload(self: &Rc<Self>) {
        if let Some((id, ())) = self.with_active_view(|_, _| ()) {
            self.inspector_load_meta(id, true);
        }
    }

    fn inspector_retry_structure(self: &Rc<Self>) {
        if let Some((id, ())) = self.with_active_view(|_, _| ()) {
            self.inspector_load_meta(id, false);
        }
    }

    /// Run a state change on the view, and if it says the rows changed, fetch
    /// again (with a recount when the filters changed).
    fn inspector_change(
        self: &Rc<Self>,
        recount: bool,
        f: impl FnOnce(&mut TableView, Option<&QueryResult>) -> bool,
    ) {
        match self.with_active_view(f) {
            Some((id, true)) => self.inspector_fetch(id, recount),
            Some((_, false)) => self.refresh_inspector(),
            None => {}
        }
    }

    fn inspector_add_filter(&self) {
        self.with_active_view(|v, page| {
            let cols = v.filter_column_names(page);
            v.add_filter(cols.first().map(String::as_str).unwrap_or(""));
        });
        self.refresh_inspector();
    }

    fn inspector_pick_filter_column(&self, row: usize, pick: usize) {
        self.with_active_view(|v, page| {
            if let Some(name) = v.filter_column_names(page).get(pick) {
                v.set_filter_column(row, name);
            }
        });
        self.refresh_inspector();
    }

    /// Typing in a filter value only marks the filters dirty. Re-projecting
    /// the rows would rebuild the text box under the user's cursor.
    fn inspector_edit_filter_value(&self, row: usize, value: &str) {
        let dirty = self.with_active_view(|v, _| {
            v.set_filter_value(row, value);
            v.filters_dirty()
        });
        if let (Some(w), Some((_, dirty))) = (self.window.upgrade(), dirty) {
            w.global::<InspectorBridge>().set_filters_dirty(dirty);
        }
    }

    fn inspector_sort(self: &Rc<Self>, column: usize) {
        self.inspector_change(false, |v, page| {
            let Some(name) = page.and_then(|r| r.columns.get(column)) else {
                return false;
            };
            v.toggle_sort(name);
            true
        });
    }

    fn inspector_copy(self: &Rc<Self>, kind: CopyKind) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let copied = self
            .with_active_view(|v, page| {
                let cell = v.selection?;
                let text = copy_text(page?, kind, cell.row, cell.col)?;
                let values = page.map(|r| r.rows.len()).unwrap_or(0);
                Some((text, values))
            })
            .and_then(|(_, r)| r);
        let Some((text, values)) = copied else {
            return;
        };
        w.invoke_copy_to_clipboard(text.as_str().into());
        let what = match kind {
            CopyKind::Cell => "Copied cell".to_string(),
            CopyKind::Row => "Copied row".to_string(),
            CopyKind::Column => format!("Copied column ({values} values on this page)"),
            CopyKind::RowJson => "Copied row as JSON".to_string(),
        };
        self.toast(what, Tone::Success);
    }

    /// A foreign key's target, opened in its own tab if it is in this schema.
    fn inspector_open_table(self: &Rc<Self>, name: &str) {
        let known = self
            .session
            .borrow()
            .workspace
            .tables
            .iter()
            .any(|t| t.name == name);
        if known {
            self.select_table(name);
        } else {
            self.toast(
                format!("\"{name}\" is not in this schema, so it cannot be opened here."),
                Tone::Warning,
            );
        }
    }
}

/// Connect the inspector's intents to the controller.
pub(super) fn wire(window: &AppWindow, ctx: &Rc<Ctx>) {
    let b = window.global::<InspectorBridge>();

    macro_rules! on {
        ($setter:ident, |$ctx:ident $(, $arg:ident)*| $body:expr) => {{
            let $ctx = ctx.clone();
            b.$setter(move |$($arg),*| $body);
        }};
    }

    on!(on_set_view, |ctx, i| ctx.inspector_set_view(i));
    on!(on_refresh, |ctx| ctx.inspector_reload());
    on!(on_retry_structure, |ctx| ctx.inspector_retry_structure());
    on!(on_toggle_filters, |ctx| {
        ctx.with_active_view(|v, _| v.filters_open = !v.filters_open);
        ctx.refresh_inspector();
    });
    on!(on_add_filter, |ctx| ctx.inspector_add_filter());
    on!(on_remove_filter, |ctx, i| {
        ctx.with_active_view(|v, _| v.remove_filter(i as usize));
        ctx.refresh_inspector();
    });
    on!(on_filter_column_picked, |ctx, i, k| {
        ctx.inspector_pick_filter_column(i as usize, k as usize)
    });
    on!(on_filter_op_picked, |ctx, i, k| {
        ctx.with_active_view(|v, _| v.set_filter_op(i as usize, k as usize));
        ctx.refresh_inspector();
    });
    on!(on_filter_value_edited, |ctx, i, t| {
        ctx.inspector_edit_filter_value(i as usize, &t)
    });
    on!(on_apply_filters, |ctx| ctx
        .inspector_change(true, |v, _| v.apply_filters()));
    on!(on_clear_filters, |ctx| ctx
        .inspector_change(true, |v, _| v.clear_filters()));
    on!(on_sort_by, |ctx, i| ctx.inspector_sort(i as usize));
    on!(on_go_first, |ctx| ctx
        .inspector_change(false, |v, _| v.first()));
    on!(on_go_prev, |ctx| ctx
        .inspector_change(false, |v, _| v.prev()));
    on!(on_go_next, |ctx| {
        ctx.inspector_change(false, |v, page| {
            v.next(page.map(|r| r.rows.len()).unwrap_or(0))
        })
    });
    on!(on_jump_safe, |ctx| ctx
        .inspector_change(false, |v, _| v.jump_to_safe_page()));
    on!(on_page_size_picked, |ctx, k| {
        ctx.inspector_change(false, |v, _| {
            page_size_at(k as usize)
                .map(|s| v.set_page_size(s))
                .unwrap_or(false)
        })
    });
    on!(on_cell_clicked, |ctx, r, c| {
        ctx.with_active_view(|v, _| v.select(r as usize, c as usize));
        ctx.refresh_inspector();
    });
    on!(on_close_panel, |ctx| {
        ctx.with_active_view(|v, _| {
            v.panel_open = false;
            v.clear_selection();
        });
        ctx.refresh_inspector();
    });
    on!(on_copy_cell, |ctx| ctx.inspector_copy(CopyKind::Cell));
    on!(on_copy_row, |ctx| ctx.inspector_copy(CopyKind::Row));
    on!(on_copy_column, |ctx| ctx.inspector_copy(CopyKind::Column));
    on!(on_copy_json, |ctx| ctx.inspector_copy(CopyKind::RowJson));
    on!(on_open_table, |ctx, name| ctx.inspector_open_table(&name));
}

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

/// Push a table tab's inspector state into the bridge.
pub(super) fn project(w: &AppWindow, ws: &Workspace, tab: &Tab) {
    let Some(v) = &tab.inspector else {
        return;
    };
    let b = w.global::<InspectorBridge>();
    let page = tab.result.as_ref();
    let shown = page.map(|r| r.rows.len()).unwrap_or(0);

    b.set_view(if v.view == View::Structure { 1 } else { 0 });
    b.set_total_label(total_label(v.total, v.counting, v.applied_count() > 0).into());
    b.set_elapsed_label(
        tab.elapsed_ms
            .map(|ms| format!("{ms}ms"))
            .unwrap_or_default()
            .into(),
    );

    // Filters.
    b.set_filters_open(v.filters_open);
    b.set_applied_filter_count(v.applied_count() as i32);
    b.set_filters_dirty(v.filters_dirty());
    b.set_filter_rows(model(
        v.filter_rows()
            .into_iter()
            .map(|r| FilterRowItem {
                column: r.column.into(),
                op: r.op.into(),
                value: r.value.into(),
                takes_value: r.takes_value,
            })
            .collect(),
    ));
    b.set_filter_columns(model(
        v.filter_column_names(page)
            .into_iter()
            .map(Into::into)
            .collect(),
    ));
    b.set_filter_ops(model(
        filter_op_labels().into_iter().map(Into::into).collect(),
    ));

    // Sort and pager.
    let columns: Vec<String> = page.map(|r| r.columns.clone()).unwrap_or_default();
    b.set_sort_column(v.sort_column_index(&columns).map_or(-1, |i| i as i32));
    b.set_sort_desc(v.sort.as_ref().is_some_and(|s| s.descending));
    let p = pager_state(v.page, v.page_size, shown, v.total);
    b.set_page_label(p.page_label.into());
    b.set_range_label(page_range_label(v.page, v.page_size, shown, v.total).into());
    b.set_page_size_label(page_size_label(v.page_size).into());
    b.set_page_size_options(model(
        page_size_labels().into_iter().map(Into::into).collect(),
    ));
    b.set_can_first(p.can_first);
    b.set_can_prev(p.can_prev);
    b.set_can_next(p.can_next);
    b.set_show_jump(p.show_jump);
    b.set_deep_blocked(v.deep_offset_blocked);

    // Selection and the row panel. A selection that no longer points at a
    // row (the page changed under it) shows nothing.
    let new_rows = v.edit.session.as_ref().map_or(0, |s| s.new_rows().len());
    let selected = v
        .selection
        .filter(|c| page.is_some_and(|r| c.row < r.rows.len() + new_rows && c.col < r.columns.len()));
    b.set_selected_row(selected.map_or(-1, |c| c.row as i32));
    b.set_selected_col(selected.map_or(-1, |c| c.col as i32));
    // A new, unsaved row has no stored JSON to show.
    let on_page = selected.filter(|c| page.is_some_and(|r| c.row < r.rows.len()));
    b.set_panel_open(v.panel_open && on_page.is_some());
    match (on_page, page) {
        (Some(c), Some(r)) => {
            b.set_panel_title(selection_label(r, c).into());
            b.set_panel_json(row_json(r, c.row).unwrap_or_default().into());
        }
        _ => {
            b.set_panel_title("".into());
            b.set_panel_json("".into());
        }
    }

    // Structure.
    b.set_meta_loading(v.meta_loading());
    b.set_meta_error(v.meta_error().unwrap_or("").into());
    match &v.meta {
        Meta::Ready(m) => {
            let schema_name = ws.schema.as_deref().unwrap_or("");
            let table = &m.schema.table.name;
            b.set_struct_columns(model(
                column_rows(&m.schema, &m.foreign_keys, schema_name)
                    .into_iter()
                    .map(|c| ColumnRowItem {
                        name: c.name.into(),
                        data_type: c.data_type.into(),
                        nullable: c.nullable,
                        default_value: c.default.into(),
                        primary: c.primary,
                        auto: c.auto,
                        foreign: c.foreign.into(),
                        foreign_table: c.foreign_table.into(),
                    })
                    .collect(),
            ));
            b.set_struct_indexes(model(
                index_rows(&m.schema)
                    .into_iter()
                    .map(|i| IndexRowItem {
                        name: i.name.into(),
                        columns: i.columns.into(),
                        kind: i.kind.into(),
                    })
                    .collect(),
            ));
            b.set_struct_relations(model(
                relation_rows(&m.foreign_keys, schema_name, table)
                    .into_iter()
                    .map(|r| RelationRowItem {
                        constraint: r.constraint.into(),
                        outgoing: r.outgoing,
                        from: r.from.into(),
                        to: r.to.into(),
                        actions: format!("{} / {}", r.on_delete, r.on_update).into(),
                        other_table: r.other_table.into(),
                    })
                    .collect(),
            ));
        }
        _ => {
            b.set_struct_columns(model(Vec::new()));
            b.set_struct_indexes(model(Vec::new()));
            b.set_struct_relations(model(Vec::new()));
        }
    }

    super::row_edit::project(w, ws, tab);
}
