//! Controller glue for query tabs' extras: Stop and Clear, the query grid's
//! cell selection and copy, the tab bar's context menu and keyboard, and the
//! editor settings (font family, word wrap, minimap).
//!
//! The decisions are `dbhive_ui_model::{query_tabs, editor_prefs}` and the
//! cancel sentinel in `format` (unit tested). This file keeps the abort
//! handles of running queries, projects the `QueryState` and `EditorPrefs`
//! globals, and acts on their callbacks. A child module of `app` so it can
//! reach the controller's private session.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use dbhive_core::drivers::{DatabaseDriver, QueryResult};
use dbhive_ui_model::editor_prefs::font_choice;
use dbhive_ui_model::format::{cancelled_summary, is_cancelled};
use dbhive_ui_model::query_tabs::{
    bar_key, bulk_close_prompt, cycle, tab_menu, tabs_to_close, BarKey, GridSelection, TabMenuEntry,
};
use dbhive_ui_model::results_grid::visible_rows;
use dbhive_ui_model::table_view::{copy_text, selection_label, CellRef, CopyKind};
use slint::{ComponentHandle, ModelRc, VecModel};
use tokio::task::AbortHandle;

use super::{with_ctx, Ctx, PendingAction};
use crate::{runtime, CtxItem, EditorPrefs, QueryState, Tone};

/// A query in flight, kept so Stop can abandon it.
struct Running {
    seq: u64,
    abort: AbortHandle,
    started: Instant,
    log_id: String,
    driver: Arc<dyn DatabaseDriver>,
}

#[derive(Default)]
pub(super) struct QuerySession {
    runs: HashMap<u64, Running>,
    /// Tabs whose last run was stopped, and whether the database itself was
    /// told to stop (the driver could interrupt) or only the wait ended.
    stopped_on_server: HashMap<u64, bool>,
    selection: Option<GridSelection>,
    /// Columns a wrapped editor row holds, as the editor reported; 0 when
    /// not wrapping.
    wrap_columns: usize,
    /// The tab the open context menu is for, and its entries.
    menu: Option<(usize, Vec<TabMenuEntry>)>,
    bar_focus_tick: i32,
}

impl QuerySession {
    pub(super) fn wrap_columns(&self) -> usize {
        self.wrap_columns
    }
}

/// Like `runtime::spawn`, but hands back the task's abort handle. Aborting
/// drops the future (and with it the wait on the driver) and the completion
/// never runs.
pub(super) fn spawn_abortable<T, F, D>(work: F, done: D) -> AbortHandle
where
    T: Send + 'static,
    F: std::future::Future<Output = T> + Send + 'static,
    D: FnOnce(T) + Send + 'static,
{
    runtime::handle()
        .spawn(async move {
            let out = work.await;
            let _ = slint::invoke_from_event_loop(move || done(out));
        })
        .abort_handle()
}

impl Ctx {
    // -- Running and stopping ---------------------------------------------------

    /// A query tab's run started: remember how to stop it.
    pub(super) fn query_run_started(
        &self,
        tab: u64,
        seq: u64,
        abort: AbortHandle,
        log_id: String,
        driver: Arc<dyn DatabaseDriver>,
    ) {
        let mut s = self.session.borrow_mut();
        s.query_tools.stopped_on_server.remove(&tab);
        s.query_tools.runs.insert(
            tab,
            Running {
                seq,
                abort,
                started: Instant::now(),
                log_id,
                driver,
            },
        );
    }

    /// A run's result arrived. `false` when the user stopped that run, so the
    /// completion must not record it or land it.
    pub(super) fn query_run_finished(&self, tab: u64, seq: u64) -> bool {
        let mut s = self.session.borrow_mut();
        match s.query_tools.runs.get(&tab) {
            Some(r) if r.seq == seq => {
                s.query_tools.runs.remove(&tab);
                true
            }
            // A newer run of the tab is in flight; this one is stale and
            // `finish_run` refuses it, but it did run and is recorded.
            Some(_) => true,
            // Stopped (or never tracked): the cancel already recorded it.
            None => !s
                .workspace
                .tabs
                .get(tab)
                .and_then(|t| t.error.as_deref())
                .is_some_and(is_cancelled),
        }
    }

    /// Stop the active tab's query: abandon the wait, ask the driver to
    /// interrupt the statement if it can, and show the tab as cancelled.
    fn query_stop(&self) {
        let (tab, running) = {
            let mut s = self.session.borrow_mut();
            let Some(tab) = s
                .workspace
                .tabs
                .active()
                .filter(|t| t.running)
                .map(|t| t.id)
            else {
                return;
            };
            (tab, s.query_tools.runs.remove(&tab))
        };
        let Some(run) = running else {
            return;
        };
        run.abort.abort();
        // SQLite stops the statement; a pooled driver cannot, and the server
        // finishes it unobserved.
        let on_server = run.driver.cancel_running();
        let elapsed = run.started.elapsed().as_millis() as u64;
        dbhive_core::service_monitoring::record_query_cancel(&self.state, &run.log_id, elapsed);
        {
            let mut s = self.session.borrow_mut();
            s.workspace.tabs.cancel_run(tab, elapsed);
            s.query_tools.stopped_on_server.insert(tab, on_server);
        }
        self.refresh_workspace();
    }

    // -- Projection -------------------------------------------------------------

    /// Project the active tab's cancelled state and grid selection.
    pub(super) fn query_tools_refresh(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let g = w.global::<QueryState>();
        let mut guard = self.session.borrow_mut();
        let s = &mut *guard;
        let tab = s.workspace.tabs.active();

        let cancelled = tab
            .filter(|t| !t.running)
            .and_then(|t| t.error.as_deref())
            .is_some_and(is_cancelled);
        g.set_cancelled(cancelled);
        g.set_cancelled_summary(
            if cancelled {
                cancelled_summary(tab.and_then(|t| t.elapsed_ms))
            } else {
                String::new()
            }
            .into(),
        );
        g.set_server_stops(
            tab.and_then(|t| s.query_tools.stopped_on_server.get(&t.id))
                .copied()
                .unwrap_or(false),
        );

        // The selection holds only while it names the same visible cell.
        let held = match (tab, &s.query_tools.selection) {
            (Some(t), Some(sel)) if t.is_query() && !t.running => t.result.as_ref().and_then(|r| {
                let shown = visible_rows(r, &t.grid).len();
                sel.holds(
                    t.id,
                    t.run_seq,
                    &t.grid.filter,
                    t.grid.sort,
                    shown,
                    r.columns.len(),
                )
                .then(|| {
                    let head = QueryResult {
                        columns: r.columns.clone(),
                        rows: Vec::new(),
                        rows_affected: None,
                    };
                    (
                        sel.row,
                        sel.col,
                        selection_label(
                            &head,
                            CellRef {
                                row: sel.row,
                                col: sel.col,
                            },
                        ),
                    )
                })
            }),
            _ => None,
        };
        match held {
            Some((row, col, label)) => {
                g.set_selected_row(row as i32);
                g.set_selected_col(col as i32);
                g.set_selection_label(label.into());
            }
            None => {
                // Stale for this tab: forget it, so coming back does not
                // revive a selection on rows that have changed.
                if tab.is_some_and(|t| {
                    s.query_tools
                        .selection
                        .as_ref()
                        .is_some_and(|sel| sel.tab == t.id)
                }) {
                    s.query_tools.selection = None;
                }
                g.set_selected_row(-1);
                g.set_selected_col(-1);
                g.set_selection_label("".into());
            }
        }
    }

    // -- Selection and copy -----------------------------------------------------

    fn query_cell_clicked(&self, row: i32, col: i32) {
        {
            let mut guard = self.session.borrow_mut();
            let s = &mut *guard;
            let Some(t) = s.workspace.tabs.active().filter(|t| t.is_query()) else {
                return;
            };
            s.query_tools.selection = (row >= 0 && col >= 0).then(|| GridSelection {
                tab: t.id,
                run_seq: t.run_seq,
                filter: t.grid.filter.clone(),
                sort: t.grid.sort,
                row: row as usize,
                col: col as usize,
            });
        }
        self.query_tools_refresh();
    }

    fn query_clear_selection(&self) {
        self.session.borrow_mut().query_tools.selection = None;
        self.query_tools_refresh();
    }

    fn query_copy(self: &Rc<Self>, kind: CopyKind) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let copied = {
            let s = self.session.borrow();
            let (Some(t), Some(sel)) = (s.workspace.tabs.active(), &s.query_tools.selection) else {
                return;
            };
            let Some(r) = &t.result else { return };
            let order = visible_rows(r, &t.grid);
            if !sel.holds(
                t.id,
                t.run_seq,
                &t.grid.filter,
                t.grid.sort,
                order.len(),
                r.columns.len(),
            ) {
                return;
            }
            // The rows as the grid shows them, so "row 3" is the third row
            // on screen and a column copy follows the filter and sort.
            let shown = QueryResult {
                columns: r.columns.clone(),
                rows: order.iter().map(|&i| r.rows[i].clone()).collect(),
                rows_affected: None,
            };
            copy_text(&shown, kind, sel.row, sel.col).map(|text| (text, shown.rows.len()))
        };
        let Some((text, count)) = copied else {
            return;
        };
        w.invoke_copy_to_clipboard(text.as_str().into());
        let what = match kind {
            CopyKind::Cell => "Copied cell".to_string(),
            CopyKind::Row => "Copied row".to_string(),
            CopyKind::Column => format!(
                "Copied column ({} {})",
                dbhive_ui_model::format::thousands(count),
                if count == 1 { "value" } else { "values" }
            ),
            CopyKind::RowJson => "Copied row as JSON".to_string(),
        };
        self.toast(what, Tone::Success);
    }

    // -- Editor -----------------------------------------------------------------

    fn query_clear_editor(&self) {
        let running = self
            .session
            .borrow()
            .workspace
            .tabs
            .active()
            .is_some_and(|t| t.running || !t.is_query());
        if running {
            return;
        }
        self.completion_clear();
        self.edit_active_sql(String::new());
    }

    /// Settings > Appearance: font family, word wrap and minimap, from the
    /// draft so the editor previews them as they are changed.
    pub(super) fn editor_prefs_apply(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let theme = self.session.borrow().settings.editor.draft().theme.clone();
        let font = font_choice(&theme.editor_font_family, std::env::consts::OS);
        let p = w.global::<EditorPrefs>();
        p.set_family(font.family.into());
        p.set_fallback(font.fallback.into());
        p.set_word_wrap(theme.editor_word_wrap);
        p.set_minimap(theme.editor_minimap);
    }

    fn editor_wrap_columns(&self, columns: i32) {
        let columns = columns.max(0) as usize;
        {
            let mut s = self.session.borrow_mut();
            if s.query_tools.wrap_columns == columns {
                return;
            }
            s.query_tools.wrap_columns = columns;
        }
        // This arrives while Slint is laying the editor out; refreshing the
        // window from inside that evaluation would re-enter the layout. Let
        // the current pass finish, then re-wrap the colour layer.
        let _ = slint::invoke_from_event_loop(|| with_ctx(|ctx| ctx.refresh_workspace()));
    }

    // -- Tab bar ----------------------------------------------------------------

    fn tab_context(&self, index: i32, x: f32, y: f32) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let count = self.session.borrow().workspace.tabs.tabs().len();
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        if index >= count {
            return;
        }
        let entries = tab_menu(index, count);
        let items: Vec<CtxItem> = entries
            .iter()
            .map(|e| CtxItem {
                label: e.label.into(),
                icon: e.icon.into(),
                enabled: e.enabled,
                danger: false,
                divider: e.divider_above,
            })
            .collect();
        self.session.borrow_mut().query_tools.menu = Some((index, entries));
        let g = w.global::<QueryState>();
        g.set_menu_items(ModelRc::new(VecModel::from(items)));
        g.set_menu_x(x);
        g.set_menu_y(y);
        g.set_menu_visible(true);
    }

    fn tab_menu_chosen(&self, item: i32) {
        let menu = self.session.borrow_mut().query_tools.menu.take();
        let Some((index, entries)) = menu else {
            return;
        };
        let Some(entry) = usize::try_from(item).ok().and_then(|i| entries.get(i)) else {
            return;
        };
        if !entry.enabled {
            return;
        }
        let ids: Vec<u64> = self
            .session
            .borrow()
            .workspace
            .tabs
            .tabs()
            .iter()
            .map(|t| t.id)
            .collect();
        self.close_tabs(tabs_to_close(entry.action, index, &ids));
    }

    /// Close several tabs, asking once first when any of them holds SQL or
    /// row edits that would be lost. One tab goes through `close_tab`, which
    /// words its own question.
    fn close_tabs(&self, ids: Vec<u64>) {
        if ids.is_empty() {
            return;
        }
        if ids.len() == 1 {
            let index = self
                .session
                .borrow()
                .workspace
                .tabs
                .tabs()
                .iter()
                .position(|t| t.id == ids[0]);
            if let Some(index) = index {
                self.close_tab(index);
            }
            return;
        }
        self.completion_clear();
        let unsaved: Vec<String> = {
            let s = self.session.borrow();
            let tabs = &s.workspace.tabs;
            ids.iter()
                .filter(|id| tabs.needs_close_confirmation(**id))
                .filter_map(|id| tabs.get(*id).map(|t| t.title.clone()))
                .collect()
        };
        match bulk_close_prompt(ids.len(), &unsaved) {
            None => self.close_tabs_now(&ids),
            Some((title, message, label)) => {
                self.session.borrow_mut().pending_action = Some(PendingAction::CloseTabs(ids));
                self.show_confirm(&title, &message, &label);
            }
        }
    }

    /// Close these tabs without asking (the confirmation was accepted). A
    /// query still in flight is abandoned and its log entry closed as
    /// cancelled: the tab is gone, so its result has nowhere to land.
    pub(super) fn close_tabs_now(&self, ids: &[u64]) {
        let mut stopped = Vec::new();
        {
            let mut s = self.session.borrow_mut();
            for id in ids {
                if let Some(run) = s.query_tools.runs.remove(id) {
                    stopped.push(run);
                }
                s.query_tools.stopped_on_server.remove(id);
                s.workspace.tabs.close(*id);
            }
        }
        for run in stopped {
            run.abort.abort();
            run.driver.cancel_running();
            dbhive_core::service_monitoring::record_query_cancel(
                &self.state,
                &run.log_id,
                run.started.elapsed().as_millis() as u64,
            );
        }
        self.refresh_workspace();
    }

    /// Arrows, Home and End on the focused bar; "next" and "prev" from
    /// Ctrl+Tab and Ctrl+Shift+Tab.
    fn tab_navigate(&self, key: &str) {
        let (active, count) = {
            let s = self.session.borrow();
            (
                s.workspace.tabs.active_index(),
                s.workspace.tabs.tabs().len(),
            )
        };
        let target = match key {
            "left" => bar_key(active, count, BarKey::Left),
            "right" => bar_key(active, count, BarKey::Right),
            "home" => bar_key(active, count, BarKey::Home),
            "end" => bar_key(active, count, BarKey::End),
            "next" => cycle(active, count, 1),
            "prev" => cycle(active, count, -1),
            _ => None,
        };
        if let Some(i) = target.filter(|i| Some(*i) != active) {
            self.select_tab(i);
        }
    }

    /// Ctrl+Tab / Ctrl+Shift+Tab that reached the window (nothing focused
    /// used it). On macOS Slint reports the Control key as `meta`.
    pub(super) fn tabs_key_chord(&self, text: &str, ctrl: bool, shift: bool, meta: bool) -> bool {
        if !(ctrl || meta) {
            return false;
        }
        let in_workspace = self
            .window
            .upgrade()
            .is_some_and(|w| w.get_screen() == crate::Screen::Workspace);
        if !in_workspace {
            return false;
        }
        match text {
            "\t" if shift => self.tab_navigate("prev"),
            "\t" => self.tab_navigate("next"),
            // Shift+Tab arrives as Backtab on some platforms.
            "\u{19}" => self.tab_navigate("prev"),
            _ => return false,
        }
        true
    }

    /// Hand the keyboard to the tab bar (used by tests and after a menu).
    #[allow(dead_code)]
    pub(super) fn focus_tab_bar(&self) {
        let tick = {
            let mut s = self.session.borrow_mut();
            s.query_tools.bar_focus_tick += 1;
            s.query_tools.bar_focus_tick
        };
        if let Some(w) = self.window.upgrade() {
            w.global::<QueryState>().set_bar_focus_tick(tick);
        }
    }
}

/// Connect every `QueryState` and `EditorPrefs` callback to the controller.
pub(super) fn wire(window: &crate::AppWindow, ctx: &Rc<Ctx>) {
    let g = window.global::<QueryState>();

    macro_rules! on {
        ($setter:ident, |$c:ident $(, $a:ident)*| $body:expr) => {{
            let $c = ctx.clone();
            g.$setter(move |$($a),*| {
                let $c = &$c;
                $body
            });
        }};
    }

    on!(on_stop, |c| c.query_stop());
    on!(on_clear_editor, |c| c.query_clear_editor());
    on!(on_cell_clicked, |c, r, col| c.query_cell_clicked(r, col));
    on!(on_clear_selection, |c| c.query_clear_selection());
    on!(on_copy_cell, |c| c.query_copy(CopyKind::Cell));
    on!(on_copy_row, |c| c.query_copy(CopyKind::Row));
    on!(on_copy_column, |c| c.query_copy(CopyKind::Column));
    on!(on_tab_context, |c, i, x, y| c.tab_context(i, x, y));
    on!(on_tab_navigate, |c, key| c.tab_navigate(&key));
    on!(on_menu_chosen, |c, i| c.tab_menu_chosen(i));
    on!(on_menu_dismissed, |c| {
        c.session.borrow_mut().query_tools.menu = None;
    });

    let ctx = ctx.clone();
    window
        .global::<EditorPrefs>()
        .on_wrap_columns_changed(move |n| ctx.editor_wrap_columns(n));
}
