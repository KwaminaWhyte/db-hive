//! Controller glue for the results area's extras: the Table / JSON / Text
//! switch, the structured query error, and exporting a result to CSV or JSON.
//!
//! The logic is `dbhive_ui_model::result_export` (formatting, caps, error
//! headlines, path checks; unit tested). This file picks the rows the grid is
//! showing, hands them over, and projects the state into the `ResultsState`
//! global. A child module of `app` so it can reach the controller's private
//! session.

use std::collections::HashMap;
use std::rc::Rc;

use dbhive_core::drivers::QueryResult;
use dbhive_ui_model::data_forms::expand_home;
use dbhive_ui_model::result_export::{
    self as rx, check_path, describe_error, error_clipboard, json_view, scope_note, text_tsv,
    text_view, truncation_notice, ExportScope, PathCheck, ResultFormat, ViewMode, VIEW_ROW_CAP,
};
use dbhive_ui_model::results_grid::visible_rows;
use dbhive_ui_model::workspace_tabs::{Tab, TabKind};
use serde_json::Value;
use slint::{ComponentHandle, ModelRc, VecModel};

use super::{with_ctx, Ctx};
use crate::results_view::{lines_model, no_lines, LineStyle};
use crate::{runtime, ResultsState, Tone};

/// What the JSON / Text model was last built from. A keystroke in the editor
/// refreshes the workspace without touching the result, and rebuilding 2,000
/// rows of JSON each time would be wasted work.
#[derive(Debug, Clone, PartialEq)]
struct ViewKey {
    tab: u64,
    run_seq: u64,
    mode: ViewMode,
    filter: String,
    sort: Option<(usize, bool)>,
}

/// The export dialog's state.
struct ExportDialog {
    open: bool,
    tab: u64,
    format: ResultFormat,
    path: String,
    /// The user typed in the path box; a format change then keeps their name.
    touched: bool,
    /// Why the last Export click was refused, or why the write failed.
    error: String,
    running: bool,
    /// Set once the file is written: the full path and the row count.
    done: Option<(String, usize)>,
}

impl Default for ExportDialog {
    fn default() -> Self {
        Self {
            open: false,
            tab: 0,
            format: ResultFormat::Csv,
            path: String::new(),
            touched: false,
            error: String::new(),
            running: false,
            done: None,
        }
    }
}

#[derive(Default)]
pub(super) struct ResultsSession {
    /// The view each query tab was left in.
    modes: HashMap<u64, ViewMode>,
    built: Option<ViewKey>,
    /// Which failed run the error panel last showed, so a new failure starts
    /// with the query folded away again.
    error_for: Option<(u64, u64)>,
    error_query_open: bool,
    export: ExportDialog,
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// The rows an export would write for a tab, as the grid shows them: a query
/// tab's rows after the grid filter and sort, a table tab's stored page.
struct Snapshot {
    scope: ExportScope,
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
    /// Rows in the result before any grid filter.
    held: usize,
    pending_edits: usize,
}

fn snapshot(tab: &Tab) -> Option<Snapshot> {
    let result: &QueryResult = tab.result.as_ref()?;
    if result.columns.is_empty() {
        return None;
    }
    let (scope, rows) = match tab.kind {
        TabKind::Query => {
            let order = visible_rows(result, &tab.grid);
            (ExportScope::Query, order.iter().map(|&i| result.rows[i].clone()).collect())
        }
        TabKind::Table(_) => (ExportScope::TablePage, result.rows.clone()),
    };
    Some(Snapshot {
        scope,
        columns: result.columns.clone(),
        rows,
        held: result.rows.len(),
        pending_edits: tab.inspector.as_ref().map_or(0, |v| v.edit.pending_count()),
    })
}

/// A file name stem for the tab: the table's name, or "query_results".
fn stem_for(tab: &Tab) -> String {
    match &tab.kind {
        TabKind::Table(name) => name.clone(),
        TabKind::Query => "query_results".to_string(),
    }
}

impl Ctx {
    // -- Projection -------------------------------------------------------------

    /// Project the active tab's view mode, JSON / Text lines, error and
    /// export availability into `ResultsState`. Cheap when nothing changed.
    pub(super) fn results_refresh(&self) {
        let Some(w) = self.window.upgrade() else { return };
        let g = w.global::<ResultsState>();
        let mut guard = self.session.borrow_mut();
        let s = &mut *guard;
        let Some(tab) = s.workspace.tabs.active() else {
            g.set_mode(0);
            g.set_can_export(false);
            g.set_error_headline("".into());
            g.set_error_detail("".into());
            g.set_error_code("".into());
            g.set_error_query("".into());
            if s.results.built.take().is_some() {
                g.set_view_lines(no_lines());
                g.set_view_notice("".into());
            }
            // The workspace is gone (disconnect); an export dialog over it
            // has nothing left to export.
            if s.results.export.open && !s.results.export.running {
                s.results.export = ExportDialog::default();
                g.set_export_visible(false);
            }
            return;
        };

        // The error panel is for query tabs; a table tab keeps the inspector's.
        match (&tab.error, tab.is_query()) {
            (Some(raw), true) => {
                let view = describe_error(raw);
                if s.results.error_for != Some((tab.id, tab.run_seq)) {
                    s.results.error_for = Some((tab.id, tab.run_seq));
                    s.results.error_query_open = false;
                }
                g.set_error_headline(view.headline.into());
                g.set_error_detail(view.detail.into());
                g.set_error_code(view.code.into());
                g.set_error_query(tab.sql.trim().into());
                g.set_error_query_open(s.results.error_query_open);
            }
            _ => {
                g.set_error_headline("".into());
                g.set_error_detail("".into());
                g.set_error_code("".into());
                g.set_error_query("".into());
            }
        }

        let has_columns = tab.result.as_ref().is_some_and(|r| !r.columns.is_empty());
        g.set_can_export(has_columns && !tab.running);

        let mode = if tab.is_query() && has_columns && tab.error.is_none() {
            s.results.modes.get(&tab.id).copied().unwrap_or_default()
        } else {
            ViewMode::Table
        };
        g.set_mode(mode.index());

        if mode == ViewMode::Table {
            if s.results.built.take().is_some() {
                g.set_view_lines(no_lines());
                g.set_view_notice("".into());
            }
            return;
        }
        let Some(result) = &tab.result else { return };
        let key = ViewKey {
            tab: tab.id,
            run_seq: tab.run_seq,
            mode,
            filter: tab.grid.filter.clone(),
            sort: tab.grid.sort,
        };
        if s.results.built.as_ref() == Some(&key) {
            return;
        }
        let (lines, style, max_columns, notice) = build_view(result, tab, mode);
        g.set_view_lines(lines_model(lines.lines.clone(), style));
        g.set_view_max_columns(max_columns as i32);
        g.set_view_notice(notice.unwrap_or_default().into());
        s.results.built = Some(key);
    }

    /// Project the export dialog.
    fn results_project_export(&self) {
        let Some(w) = self.window.upgrade() else { return };
        let g = w.global::<ResultsState>();
        let s = self.session.borrow();
        let d = &s.results.export;
        g.set_export_visible(d.open);
        if !d.open {
            return;
        }
        g.set_export_formats(ModelRc::new(VecModel::from(
            ResultFormat::labels().into_iter().map(Into::into).collect::<Vec<slint::SharedString>>(),
        )));
        g.set_export_format(d.format.index());
        g.set_export_hint(rx::format_hint(d.format).into());
        g.set_export_path_error(d.error.as_str().into());
        g.set_export_running(d.running);
        let snap = s.workspace.tabs.get(d.tab).and_then(snapshot);
        let note = match &snap {
            Some(sn) => {
                let mut note = scope_note(sn.scope, sn.rows.len(), sn.held, sn.columns.len());
                if sn.pending_edits > 0 {
                    note.push_str(" Pending row edits are not included.");
                }
                note
            }
            None => "There is no result to export any more. Run the query again.".to_string(),
        };
        g.set_export_scope_note(note.into());
        g.set_export_replaces(
            d.done.is_none() && matches!(check_path(&d.path, d.format), PathCheck::Replaces),
        );
        match &d.done {
            Some((path, rows)) => {
                g.set_export_done(true);
                g.set_export_done_message(
                    format!(
                        "Exported {} {} to {}",
                        dbhive_ui_model::format::thousands(*rows),
                        if *rows == 1 { "row" } else { "rows" },
                        path
                    )
                    .into(),
                );
            }
            None => {
                g.set_export_done(false);
                g.set_export_done_message("".into());
            }
        }
    }

    // -- View modes ---------------------------------------------------------------

    fn results_mode_picked(&self, index: i32) {
        {
            let mut s = self.session.borrow_mut();
            let Some(tab) = s.workspace.tabs.active() else { return };
            if !tab.is_query() {
                return;
            }
            let id = tab.id;
            s.results.modes.insert(id, ViewMode::at(index));
        }
        self.results_refresh();
    }

    /// Copy what the JSON or Text view shows (JSON text, or real TSV).
    fn results_view_copy(self: &Rc<Self>) {
        let Some(w) = self.window.upgrade() else { return };
        let copied = {
            let s = self.session.borrow();
            let Some(tab) = s.workspace.tabs.active() else { return };
            let Some(result) = &tab.result else { return };
            let mode = s.results.modes.get(&tab.id).copied().unwrap_or_default();
            let (columns, rows) = capped_rows(result, tab);
            match mode {
                ViewMode::Json => Some((json_view(&columns, &rows, VIEW_ROW_CAP).joined(), "JSON")),
                ViewMode::Text => Some((text_tsv(&columns, &rows, VIEW_ROW_CAP), "text")),
                ViewMode::Table => None,
            }
        };
        if let Some((text, what)) = copied {
            w.invoke_copy_to_clipboard(text.as_str().into());
            self.toast(format!("Copied the {what} view"), Tone::Success);
        }
    }

    // -- Error --------------------------------------------------------------------

    fn results_error_copy(self: &Rc<Self>) {
        let Some(w) = self.window.upgrade() else { return };
        let text = {
            let s = self.session.borrow();
            let Some(tab) = s.workspace.tabs.active() else { return };
            let Some(raw) = &tab.error else { return };
            error_clipboard(&describe_error(raw), &tab.sql)
        };
        w.invoke_copy_to_clipboard(text.as_str().into());
        self.toast("Copied the error and the query", Tone::Success);
    }

    // -- Export -------------------------------------------------------------------

    fn results_export_open(&self) {
        {
            let mut s = self.session.borrow_mut();
            let Some(tab) = s.workspace.tabs.active() else { return };
            if tab.running || snapshot(tab).is_none() {
                return;
            }
            let (id, stem) = (tab.id, stem_for(tab));
            let format = s.results.export.format;
            s.results.export = ExportDialog {
                open: true,
                tab: id,
                format,
                path: rx::suggested_path(
                    &dbhive_core::service_import_export::suggested_directory(),
                    &stem,
                    format,
                    &today(),
                ),
                ..ExportDialog::default()
            };
        }
        self.results_push_export_path();
        self.results_project_export();
    }

    /// Put the dialog's path into the text box (typing already has it there).
    fn results_push_export_path(&self) {
        let Some(w) = self.window.upgrade() else { return };
        let path = self.session.borrow().results.export.path.clone();
        w.global::<ResultsState>().set_export_path(path.into());
    }

    fn results_export_close(&self) {
        {
            let mut s = self.session.borrow_mut();
            if s.results.export.running {
                return;
            }
            s.results.export.open = false;
            s.results.export.done = None;
        }
        self.results_project_export();
    }

    fn results_export_format_picked(&self, index: i32) {
        {
            let mut s = self.session.borrow_mut();
            let d = &mut s.results.export;
            if d.running {
                return;
            }
            d.format = ResultFormat::at(index);
            d.path = rx::retarget_extension(&d.path, d.format);
            d.error.clear();
        }
        self.results_push_export_path();
        self.results_project_export();
    }

    fn results_export_path_edited(&self, text: &str) {
        {
            let mut s = self.session.borrow_mut();
            let d = &mut s.results.export;
            d.path = text.to_string();
            d.touched = true;
            d.error.clear();
        }
        self.results_project_export();
    }

    fn results_export_suggest(&self) {
        {
            let mut s = self.session.borrow_mut();
            let stem = s.workspace.tabs.get(s.results.export.tab).map(stem_for).unwrap_or_default();
            let d = &mut s.results.export;
            if d.running {
                return;
            }
            d.path = rx::suggested_path(
                &dbhive_core::service_import_export::suggested_directory(),
                &stem,
                d.format,
                &today(),
            );
            d.touched = false;
            d.error.clear();
        }
        self.results_push_export_path();
        self.results_project_export();
    }

    fn results_export_run(self: &Rc<Self>) {
        let job = {
            let mut s = self.session.borrow_mut();
            let (tab_id, format, path) = {
                let d = &s.results.export;
                if d.running || d.done.is_some() {
                    return;
                }
                (d.tab, d.format, d.path.clone())
            };
            if let PathCheck::Problem(p) = check_path(&path, format) {
                s.results.export.error = p;
                None
            } else if let Some(snap) = s.workspace.tabs.get(tab_id).and_then(snapshot) {
                let d = &mut s.results.export;
                d.error.clear();
                d.running = true;
                Some((snap.columns, snap.rows, format, expand_home(&path)))
            } else {
                s.results.export.error =
                    "There is no result to export any more. Run the query again.".to_string();
                None
            }
        };
        self.results_project_export();
        let Some((columns, rows, format, path)) = job else { return };
        let written = path.clone();
        runtime::spawn(
            async move {
                // Rendering a large result is megabytes of string building;
                // keep it off the async workers the drivers use.
                tokio::task::spawn_blocking(move || {
                    let count = rows.len();
                    let body = rx::render(format, &columns, &rows);
                    rx::write_export(&path, &body).map(|_| count)
                })
                .await
                .map_err(|e| format!("The export task failed: {e}"))
                .and_then(|r| r)
            },
            move |result| {
                with_ctx(|ctx| {
                    {
                        let mut s = ctx.session.borrow_mut();
                        let d = &mut s.results.export;
                        d.running = false;
                        match result {
                            Ok(count) => d.done = Some((written, count)),
                            Err(e) => d.error = e,
                        }
                    }
                    ctx.results_project_export();
                })
            },
        );
    }

    fn results_export_reveal(self: &Rc<Self>) {
        let path = {
            let s = self.session.borrow();
            match &s.results.export.done {
                Some((path, _)) => path.clone(),
                None => return,
            }
        };
        let (program, args) = rx::reveal_command(&path, std::env::consts::OS);
        if let Err(e) = std::process::Command::new(&program).args(&args).spawn() {
            self.toast(format!("Could not open the folder: {e}"), Tone::Danger);
        }
    }
}

/// The columns and the first [`VIEW_ROW_CAP`] rows of the grid's order.
fn capped_rows(result: &QueryResult, tab: &Tab) -> (Vec<String>, Vec<Vec<Value>>) {
    let order = visible_rows(result, &tab.grid);
    let rows = order.iter().take(VIEW_ROW_CAP).map(|&i| result.rows[i].clone()).collect();
    (result.columns.clone(), rows)
}

/// The lines for a view, how wide the widest is, and the truncation notice.
fn build_view(
    result: &QueryResult,
    tab: &Tab,
    mode: ViewMode,
) -> (rx::ViewText, LineStyle, usize, Option<String>) {
    let (columns, rows) = capped_rows(result, tab);
    let total = visible_rows(result, &tab.grid).len();
    let mut view = match mode {
        ViewMode::Text => text_view(&columns, &rows, VIEW_ROW_CAP),
        _ => json_view(&columns, &rows, VIEW_ROW_CAP),
    };
    // Only the first rows were cloned; the view must report the real total.
    view.total_rows = total;
    let style = if mode == ViewMode::Text { LineStyle::Text } else { LineStyle::Json };
    let max = view.max_columns();
    let notice = truncation_notice(view.shown_rows, view.total_rows);
    (view, style, max, notice)
}

/// Connect every `ResultsState` callback to the controller.
pub(super) fn wire(window: &crate::AppWindow, ctx: &Rc<Ctx>) {
    let g = window.global::<ResultsState>();

    macro_rules! on {
        ($setter:ident, |$c:ident $(, $a:ident)*| $body:expr) => {{
            let $c = ctx.clone();
            g.$setter(move |$($a),*| {
                let $c = &$c;
                $body
            });
        }};
    }

    on!(on_mode_picked, |c, i| c.results_mode_picked(i));
    on!(on_view_copy, |c| c.results_view_copy());
    on!(on_error_retry, |c| c.run_active_tab());
    on!(on_error_copy, |c| c.results_error_copy());
    on!(on_error_toggle_query, |c| {
        {
            let mut s = c.session.borrow_mut();
            s.results.error_query_open = !s.results.error_query_open;
        }
        c.results_refresh();
    });
    on!(on_export_open, |c| c.results_export_open());
    on!(on_export_close, |c| c.results_export_close());
    on!(on_export_format_picked, |c, i| c.results_export_format_picked(i));
    on!(on_export_path_edited, |c, t| c.results_export_path_edited(&t));
    on!(on_export_suggest, |c| c.results_export_suggest());
    on!(on_export_run, |c| c.results_export_run());
    on!(on_export_reveal, |c| c.results_export_reveal());
}
