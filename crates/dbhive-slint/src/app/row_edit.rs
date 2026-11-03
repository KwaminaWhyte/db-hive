//! Editing rows in a table tab's grid: double-click a cell, type, and the
//! change is held as pending; add and delete rows; review the SQL; commit it
//! all in one transaction.
//!
//! The state machine and the SQL live in `dbhive_ui_model::row_edits` (typed
//! coercion, primary-key identity, quoting, unit tested). This file is glue:
//! it routes the grid's intents to that model, overlays the pending changes on
//! the page the grid shows, and runs the commit through the driver's real
//! `execute_transaction`.
//!
//! Nothing reaches the database until the user presses Commit on the review
//! page, and then only as one transaction: either every statement applies or
//! none does.
//!
//! A child module of `app` so it can reach the controller's private state.

use std::rc::Rc;
use std::sync::Arc;

use dbhive_core::drivers::{DatabaseDriver, QueryResult};
use dbhive_core::models::DbDriver;
use dbhive_ui_model::results_grid::cell_text;
use dbhive_ui_model::row_edits::{CellEdit, EditSession, SqlQuoter, Statement, StatementKind};
use dbhive_ui_model::table_inspector::qualified_name;
use dbhive_ui_model::table_view::{rows_json, sql_qualifier, Meta, TableView};
use dbhive_ui_model::workspace_tabs::{Tab, TabKind};
use serde_json::Value;
use slint::{ComponentHandle, ModelRc, VecModel};

use super::{with_ctx, Ctx, Workspace};
use crate::grid_model::RowMark;
use crate::{runtime, AppWindow, InspectorBridge, ReviewItem, Tone};

/// Whether this kind of connection can commit atomically. Mirrors the drivers
/// that implement `execute_transaction`; the commit itself asks the driver
/// again, which is the check that counts.
fn supports_transactions(driver: &DbDriver) -> bool {
    driver.is_postgres_compatible() || matches!(driver, DbDriver::MySql | DbDriver::Sqlite)
}

/// The driver's own quoting, for the model.
struct DriverQuoter(Arc<dyn DatabaseDriver>);

impl SqlQuoter for DriverQuoter {
    fn ident(&self, name: &str) -> String {
        self.0.quote_identifier(name)
    }
    fn literal(&self, text: &str) -> String {
        format!("'{}'", self.0.escape_string_literal(text))
    }
}

/// Build the edit session once the table's structure is known.
fn ensure_session(v: &mut TableView) {
    if v.edit.session.is_some() {
        return;
    }
    if let Meta::Ready(m) = &v.meta {
        v.edit.session = Some(EditSession::new(m.schema.columns.clone()));
    }
}

/// A page row as values in the table's own column order (the page is
/// `SELECT *`, but the model identifies columns by name, not position).
fn meta_row(sess: &EditSession, page: &QueryResult, row: usize) -> Vec<Value> {
    let mut out = vec![Value::Null; sess.columns().len()];
    if let Some(r) = page.rows.get(row) {
        for (c, name) in page.columns.iter().enumerate() {
            if let (Some(i), Some(v)) = (sess.column_index(name), r.get(c)) {
                out[i] = v.clone();
            }
        }
    }
    out
}

/// A value as the text a person would type into the box.
fn value_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The page with every pending change applied, and a mark per row, for the
/// grid to draw (including which rows are ticked). `None` when nothing is
/// pending or ticked, so an untouched table pays nothing.
pub(super) fn overlay(tab: &Tab) -> Option<(Vec<Vec<Option<String>>>, Vec<RowMark>)> {
    let v = tab.inspector.as_ref()?;
    let page = tab.result.as_ref()?;
    let ncols = page.columns.len();
    let text_of = |v: &Value| {
        let (t, is_null) = cell_text(v);
        (!is_null).then_some(t)
    };
    let Some(sess) = v.edit.session.as_ref().filter(|s| !s.is_empty()) else {
        // Nothing pending: the page as it is, with only the ticks marked.
        if v.checked.is_empty() {
            return None;
        }
        let rows = page
            .rows
            .iter()
            .map(|r| r.iter().map(text_of).collect())
            .collect();
        let marks = (0..page.rows.len())
            .map(|i| RowMark {
                checked: v.checked.contains(&i),
                modified: vec![false; ncols],
                placeholder: vec![false; ncols],
                ..RowMark::default()
            })
            .collect();
        return Some((rows, marks));
    };
    let col_idx: Vec<Option<usize>> = page.columns.iter().map(|n| sess.column_index(n)).collect();

    let mut rows = Vec::new();
    let mut marks = Vec::new();
    for r in 0..page.rows.len() {
        let mrow = meta_row(sess, page, r);
        let mut cells = Vec::with_capacity(ncols);
        let mut modified = vec![false; ncols];
        for c in 0..ncols {
            let value = match col_idx[c] {
                Some(i) => sess.effective(&mrow, i),
                None => page.rows[r].get(c).cloned().unwrap_or(Value::Null),
            };
            cells.push(text_of(&value));
            modified[c] = col_idx[c].is_some_and(|i| sess.is_modified(&mrow, i));
        }
        marks.push(RowMark {
            deleted: sess.is_deleted(&mrow),
            added: false,
            checked: v.checked.contains(&r),
            modified,
            placeholder: vec![false; ncols],
        });
        rows.push(cells);
    }
    for (k, nr) in sess.new_rows().iter().enumerate() {
        let mut cells = Vec::with_capacity(ncols);
        let mut placeholder = vec![false; ncols];
        for c in 0..ncols {
            match col_idx[c].and_then(|i| nr.values.get(&i)) {
                Some(v) => cells.push(text_of(v)),
                None => {
                    let hint = col_idx[c]
                        .and_then(|i| sess.columns().get(i))
                        .map(|col| {
                            if col.is_auto_increment {
                                "auto"
                            } else {
                                "default"
                            }
                        })
                        .unwrap_or("default");
                    cells.push(Some(hint.to_string()));
                    placeholder[c] = true;
                }
            }
        }
        marks.push(RowMark {
            deleted: false,
            added: true,
            checked: v.checked.contains(&(page.rows.len() + k)),
            modified: vec![false; ncols],
            placeholder,
        });
        rows.push(cells);
    }
    Some((rows, marks))
}

/// Push the editing state of the active table tab into the bridge.
pub(super) fn project(w: &AppWindow, ws: &Workspace, tab: &Tab) {
    let Some(v) = &tab.inspector else {
        return;
    };
    let b = w.global::<InspectorBridge>();
    let e = &v.edit;
    let can = ws.driver.as_ref().is_some_and(supports_transactions);

    b.set_editable(can);
    b.set_edit_note(
        if can {
            e.session
                .as_ref()
                .and_then(|s| s.refusal())
                .map(|r| r.message())
                .unwrap_or("")
        } else {
            "This kind of connection cannot save edits safely, so the grid is read-only."
        }
        .into(),
    );
    let (er, ec) = e
        .editing
        .map(|(r, c)| (r as i32, c as i32))
        .unwrap_or((-1, -1));
    b.set_editing_row(er);
    b.set_editing_col(ec);
    b.set_edit_initial(e.initial.as_str().into());
    b.set_edit_message(
        e.cell_error
            .clone()
            .or_else(|| e.message.clone())
            .unwrap_or_default()
            .into(),
    );
    b.set_pending_count(e.pending_count() as i32);
    let shown = tab.result.as_ref().map_or(0, |r| r.rows.len())
        + e.session.as_ref().map_or(0, |s| s.new_rows().len());
    b.set_checked_count(v.checked.len() as i32);
    b.set_all_checked(v.all_checked(shown));
    b.set_review_open(e.review_open);
    b.set_committing(e.committing);
    b.set_review_items(ModelRc::new(VecModel::from(
        e.review
            .iter()
            .map(|s| ReviewItem {
                summary: s.summary.as_str().into(),
                sql: s.sql.as_str().into(),
                kind: match s.kind {
                    StatementKind::Update => "update",
                    StatementKind::Delete => "delete",
                    StatementKind::Insert => "insert",
                }
                .into(),
            })
            .collect::<Vec<_>>(),
    )));
    let selected_deleted = match (v.selection, e.session.as_ref(), tab.result.as_ref()) {
        (Some(c), Some(s), Some(page)) if c.row < page.rows.len() => {
            s.is_deleted(&meta_row(s, page, c.row))
        }
        _ => false,
    };
    b.set_selected_deleted(selected_deleted);
}

impl Ctx {
    /// Run `f` on the active table tab's view and the page it shows.
    fn with_edit_view<R>(
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

    /// Project the editing state, and redraw the grid when its rows changed.
    fn edit_changed(&self, rows_changed: bool) {
        if rows_changed {
            self.refresh_workspace();
        } else {
            let Some(w) = self.window.upgrade() else {
                return;
            };
            let s = self.session.borrow();
            if let Some(tab) = s.workspace.tabs.active() {
                project(&w, &s.workspace, tab);
            }
        }
    }

    /// The tab, the connection, and the table's schema qualifier and name.
    fn edit_target(&self) -> Option<(u64, String, String, String)> {
        let s = self.session.borrow();
        let ws = &s.workspace;
        let tab = ws.tabs.active()?;
        let TabKind::Table(table) = &tab.kind else {
            return None;
        };
        let driver = ws.driver.clone()?;
        let schema_arg = tab
            .schema
            .clone()
            .or_else(|| ws.schema.clone())
            .unwrap_or_default();
        Some((
            tab.id,
            ws.connection_id.clone(),
            sql_qualifier(&driver, &schema_arg),
            table.clone(),
        ))
    }

    /// Show a message above the grid and stop any editing in progress.
    fn edit_message(&self, message: impl Into<String>) {
        let message = message.into();
        self.with_edit_view(|v, _| {
            v.edit.editing = None;
            v.edit.cell_error = None;
            v.edit.message = Some(message);
        });
        self.edit_changed(true);
    }

    pub(super) fn row_edit_double_click(self: &Rc<Self>, row: usize, col: usize) {
        let can = self
            .session
            .borrow()
            .workspace
            .driver
            .as_ref()
            .is_some_and(supports_transactions);
        if !can {
            self.edit_message(
                "This kind of connection cannot save edits safely, so the grid is read-only.",
            );
            return;
        }
        let outcome = self.with_edit_view(|v, page| -> Result<(), String> {
            ensure_session(v);
            let Some(page) = page else {
                return Err("The rows are still loading".to_string());
            };
            let Some(sess) = v.edit.session.as_ref() else {
                return Err("The table's structure is still loading".to_string());
            };
            let n = page.rows.len();
            let name = page.columns.get(col).ok_or("No such column")?;
            let ci = sess
                .column_index(name)
                .ok_or("That column is not part of the table")?;
            let initial = if row < n {
                if let Some(r) = sess.refusal() {
                    return Err(r.message().to_string());
                }
                if sess.is_primary_key(ci) {
                    return Err("Primary key cells cannot be edited".to_string());
                }
                let mrow = meta_row(sess, page, row);
                if sess.is_deleted(&mrow) {
                    return Err("This row is marked for deletion".to_string());
                }
                value_text(&sess.effective(&mrow, ci))
            } else {
                sess.new_rows()
                    .get(row - n)
                    .ok_or("No such new row")?
                    .values
                    .get(&ci)
                    .map(value_text)
                    .unwrap_or_default()
            };
            v.edit.editing = Some((row, col));
            v.edit.initial = initial;
            v.edit.cell_error = None;
            v.edit.message = None;
            Ok(())
        });
        match outcome {
            Some((_, Ok(()))) => self.edit_changed(false),
            Some((_, Err(msg))) => self.edit_message(msg),
            None => {}
        }
    }

    pub(super) fn row_edit_accept(self: &Rc<Self>, text: &str) {
        let outcome = self.with_edit_view(|v, page| -> Result<(), String> {
            let Some((row, col)) = v.edit.editing.take() else {
                // Pressing Enter and then leaving the box both report; the
                // second one finds nothing to do.
                return Ok(());
            };
            let (Some(page), Some(sess)) = (page, v.edit.session.as_mut()) else {
                return Ok(());
            };
            let name = page.columns.get(col).ok_or("No such column")?;
            let ci = sess
                .column_index(name)
                .ok_or("That column is not part of the table")?;
            let n = page.rows.len();
            if row < n {
                let mrow = meta_row(sess, page, row);
                match sess.set_cell(&mrow, ci, text) {
                    Ok(CellEdit::Changed | CellEdit::Reverted | CellEdit::Unchanged) => Ok(()),
                    Err(e) => Err(e),
                }
            } else {
                sess.set_new_cell(row - n, ci, text)
            }
        });
        match outcome {
            Some((_, Ok(()))) => {
                self.with_edit_view(|v, _| {
                    v.edit.cell_error = None;
                    v.edit.message = None;
                });
                self.edit_changed(true);
            }
            // The text was not accepted: say why, and drop it.
            Some((_, Err(msg))) => self.edit_message(format!("Not saved: {msg}")),
            None => {}
        }
    }

    pub(super) fn row_edit_cancel(&self) {
        self.with_edit_view(|v, _| v.edit.editing = None);
        self.edit_changed(true);
    }

    pub(super) fn row_edit_add_row(self: &Rc<Self>) {
        let outcome = self.with_edit_view(|v, page| -> Result<(), String> {
            ensure_session(v);
            let Some(sess) = v.edit.session.as_mut() else {
                return Err("The table's structure is still loading".to_string());
            };
            let n = page.map(|p| p.rows.len()).unwrap_or(0);
            let idx = sess.add_row();
            // Select the new row so Delete row can take it back.
            v.select(n + idx, 0);
            v.edit.message = None;
            Ok(())
        });
        match outcome {
            Some((_, Ok(()))) => self.edit_changed(true),
            Some((_, Err(msg))) => self.edit_message(msg),
            None => {}
        }
    }

    pub(super) fn row_edit_delete_selected(self: &Rc<Self>) {
        let outcome = self.with_edit_view(|v, page| -> Result<(), String> {
            let Some(cell) = v.selection else {
                return Err("Select a row first".to_string());
            };
            let (Some(page), Some(sess)) = (page, v.edit.session.as_mut()) else {
                return Err("The table's structure is still loading".to_string());
            };
            let n = page.rows.len();
            if cell.row < n {
                let mrow = meta_row(sess, page, cell.row);
                sess.toggle_delete(&mrow).map(|_| ())
            } else {
                sess.remove_new_row(cell.row - n);
                v.clear_selection();
                Ok(())
            }
        });
        match outcome {
            Some((_, Ok(()))) => {
                self.with_edit_view(|v, _| v.edit.message = None);
                self.edit_changed(true);
            }
            Some((_, Err(msg))) => self.edit_message(msg),
            None => {}
        }
    }

    pub(super) fn row_check_toggle(&self, row: usize) {
        self.with_edit_view(|v, _| v.toggle_checked(row));
        self.edit_changed(true);
    }

    pub(super) fn row_check_all(&self) {
        self.with_edit_view(|v, page| {
            let new_rows = v.edit.session.as_ref().map_or(0, |s| s.new_rows().len());
            v.toggle_all_checked(page.map_or(0, |p| p.rows.len()) + new_rows);
        });
        self.edit_changed(true);
    }

    pub(super) fn row_check_clear(&self) {
        self.with_edit_view(|v, _| v.checked.clear());
        self.edit_changed(true);
    }

    /// The ticked rows of the page, as one JSON array, on the clipboard.
    pub(super) fn row_check_copy_json(self: &Rc<Self>) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let copied = self
            .with_edit_view(|v, page| {
                let page = page?;
                let n = v.checked.iter().filter(|&&r| r < page.rows.len()).count();
                (n > 0).then(|| (rows_json(page, &v.checked), n))
            })
            .and_then(|(_, c)| c);
        let Some((text, n)) = copied else {
            self.edit_message("Only rows that are already saved can be copied.");
            return;
        };
        w.invoke_copy_to_clipboard(text.as_str().into());
        let noun = if n == 1 { "row" } else { "rows" };
        self.toast(format!("Copied {n} {noun} as JSON"), Tone::Success);
    }

    /// Mark every ticked row for deletion. Nothing is deleted until the
    /// review is committed; a ticked row that is not saved yet is just removed.
    pub(super) fn row_check_delete(self: &Rc<Self>) {
        let outcome = self.with_edit_view(|v, page| -> Result<(), String> {
            ensure_session(v);
            let (Some(page), Some(sess)) = (page, v.edit.session.as_mut()) else {
                return Err("The table's structure is still loading".to_string());
            };
            let n = page.rows.len();
            let picked: Vec<usize> = v.checked.iter().copied().collect();
            if picked.iter().any(|&r| r < n) {
                if let Some(r) = sess.refusal() {
                    return Err(r.message().to_string());
                }
            }
            for &r in picked.iter().filter(|&&r| r < n) {
                let mrow = meta_row(sess, page, r);
                if !sess.is_deleted(&mrow) {
                    sess.toggle_delete(&mrow)?;
                }
            }
            // Highest first, so removing one does not shift the next.
            for &r in picked.iter().rev().filter(|&&r| r >= n) {
                sess.remove_new_row(r - n);
            }
            v.clear_selection();
            v.edit.message = None;
            Ok(())
        });
        match outcome {
            Some((_, Ok(()))) => self.edit_changed(true),
            Some((_, Err(msg))) => self.edit_message(msg),
            None => {}
        }
    }

    pub(super) fn row_edit_discard(&self) {
        self.with_edit_view(|v, _| {
            if let Some(s) = v.edit.session.as_mut() {
                s.discard();
            }
            v.edit.editing = None;
            v.edit.review_open = false;
            v.edit.review.clear();
            v.edit.message = None;
            v.edit.cell_error = None;
            v.clear_selection();
        });
        self.edit_changed(true);
    }

    /// The statements for the active tab's pending changes.
    fn edit_statements(&self, d: &Arc<dyn DatabaseDriver>) -> Result<Vec<Statement>, String> {
        let (id, _, qualifier, table) = self
            .edit_target()
            .ok_or_else(|| "Open a table first".to_string())?;
        let quote = |x: &str| d.quote_identifier(x);
        let table_sql = qualified_name(&quote, &qualifier, &table);
        let s = self.session.borrow();
        let tab = s
            .workspace
            .tabs
            .get(id)
            .ok_or_else(|| "The tab is gone".to_string())?;
        let sess = tab
            .inspector
            .as_ref()
            .and_then(|v| v.edit.session.as_ref())
            .ok_or_else(|| "Nothing to save".to_string())?;
        sess.statements(&table_sql, &DriverQuoter(d.clone()))
    }

    pub(super) fn row_edit_open_review(self: &Rc<Self>) {
        let Some((_, connection_id, _, _)) = self.edit_target() else {
            return;
        };
        let Some(d) = self.open_driver(&connection_id) else {
            self.edit_message("This connection is no longer open.");
            return;
        };
        match self.edit_statements(&d) {
            Ok(statements) if statements.is_empty() => {
                self.edit_message("There is nothing to save.");
            }
            Ok(statements) => {
                self.with_edit_view(|v, _| {
                    v.edit.editing = None;
                    v.edit.review = statements;
                    v.edit.review_open = true;
                    v.edit.message = None;
                });
                self.edit_changed(true);
            }
            // A new row that is missing a required value, for example.
            Err(msg) => self.edit_message(msg),
        }
    }

    pub(super) fn row_edit_close_review(&self) {
        self.with_edit_view(|v, _| {
            v.edit.review_open = false;
            v.edit.review.clear();
            v.edit.message = None;
        });
        self.edit_changed(true);
    }

    pub(super) fn row_edit_commit(self: &Rc<Self>) {
        let Some((id, connection_id, _, _)) = self.edit_target() else {
            return;
        };
        let already = self
            .with_edit_view(|v, _| v.edit.committing)
            .map(|(_, c)| c)
            .unwrap_or(true);
        if already {
            return;
        }
        let Some(d) = self.open_driver(&connection_id) else {
            self.row_edit_commit_failed("This connection is no longer open.".to_string());
            return;
        };
        // The driver, not a guess, decides whether this can be atomic.
        if !d.supports_transactions() {
            self.row_edit_commit_failed(
                "This connection cannot run the changes as one transaction, so nothing was saved."
                    .to_string(),
            );
            return;
        }
        let statements = match self.edit_statements(&d) {
            Ok(s) => s,
            Err(msg) => {
                self.row_edit_commit_failed(msg);
                return;
            }
        };
        let sqls: Vec<String> = statements.iter().map(|s| s.sql.clone()).collect();
        self.with_edit_view(|v, _| {
            v.edit.committing = true;
            v.edit.message = None;
        });
        self.edit_changed(false);

        runtime::spawn(
            async move { d.execute_transaction(&sqls).await },
            move |result| {
                with_ctx(|ctx| ctx.row_edit_finish(id, statements, result));
            },
        );
    }

    fn row_edit_commit_failed(&self, message: String) {
        self.with_edit_view(|v, _| {
            v.edit.committing = false;
            v.edit.message = Some(message);
        });
        self.edit_changed(false);
    }

    fn row_edit_finish(
        self: &Rc<Self>,
        id: u64,
        statements: Vec<Statement>,
        result: Result<Vec<u64>, dbhive_core::drivers::TransactionFailure>,
    ) {
        match result {
            Ok(affected) => {
                // A statement that touched a different number of rows than
                // expected means the table changed under the user (someone
                // else edited or deleted that row). The transaction has
                // committed, so say so rather than hide it.
                let off = statements
                    .iter()
                    .zip(&affected)
                    .filter(|(s, n)| s.expect_rows != **n)
                    .count();
                let total = statements.len();
                {
                    let mut s = self.session.borrow_mut();
                    if let Some(v) = s
                        .workspace
                        .tabs
                        .get_mut(id)
                        .and_then(|t| t.inspector.as_mut())
                    {
                        if let Some(sess) = v.edit.session.as_mut() {
                            sess.discard();
                        }
                        v.edit.review_open = false;
                        v.edit.review.clear();
                        v.edit.committing = false;
                        v.edit.message = None;
                        v.clear_selection();
                    }
                }
                let noun = if total == 1 { "change" } else { "changes" };
                if off == 0 {
                    self.toast(format!("Saved {total} {noun}"), Tone::Success);
                } else {
                    self.toast(
                        format!(
                            "Saved {total} {noun}, but {off} did not match a row: the data was changed elsewhere."
                        ),
                        Tone::Warning,
                    );
                }
                // Show what the database has now.
                self.inspector_open(id);
            }
            Err(failure) => {
                let summary = statements
                    .get(failure.index)
                    .map(|s| s.summary.clone())
                    .unwrap_or_default();
                self.row_edit_commit_failed(format!(
                    "Nothing was changed. Change {} of {} failed ({}): {}",
                    failure.index + 1,
                    statements.len(),
                    summary,
                    failure.error
                ));
            }
        }
    }
}

pub(super) fn wire(window: &AppWindow, ctx: &Rc<Ctx>) {
    let b = window.global::<InspectorBridge>();

    macro_rules! on {
        ($setter:ident, |$ctx:ident $(, $arg:ident)*| $body:expr) => {{
            let $ctx = ctx.clone();
            b.$setter(move |$($arg),*| $body);
        }};
    }

    on!(on_cell_double_clicked, |ctx, r, c| ctx
        .row_edit_double_click(r.max(0) as usize, c.max(0) as usize));
    on!(on_edit_accepted, |ctx, t| ctx.row_edit_accept(&t));
    on!(on_edit_cancelled, |ctx| ctx.row_edit_cancel());
    on!(on_add_row, |ctx| ctx.row_edit_add_row());
    on!(on_delete_selected, |ctx| ctx.row_edit_delete_selected());
    on!(on_discard_edits, |ctx| ctx.row_edit_discard());
    on!(on_row_toggled, |ctx, r| ctx
        .row_check_toggle(r.max(0) as usize));
    on!(on_toggle_all, |ctx| ctx.row_check_all());
    on!(on_clear_checked, |ctx| ctx.row_check_clear());
    on!(on_copy_checked_json, |ctx| ctx.row_check_copy_json());
    on!(on_delete_checked, |ctx| ctx.row_check_delete());
    on!(on_open_review, |ctx| ctx.row_edit_open_review());
    on!(on_close_review, |ctx| ctx.row_edit_close_review());
    on!(on_commit_edits, |ctx| ctx.row_edit_commit());
}
