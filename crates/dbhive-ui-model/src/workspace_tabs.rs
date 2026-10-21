//! The workspace's tabs: query tabs with an editor, and table tabs that
//! preview one table.
//!
//! Pure state, no toolkit and no async. The shell starts a run with
//! `begin_run`, gets a sequence number, and hands the result back through
//! `finish_run` with that number. A result whose number is no longer current
//! (the tab was re-run, or closed) is refused, so a slow query can never
//! overwrite a newer one or resurrect a closed tab.

use dbhive_core::drivers::QueryResult;

use crate::format::thousands;
use crate::results_grid::GridState;
use crate::table_view::TableView;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabKind {
    Query,
    /// A read-only preview of one table.
    Table(String),
}

pub struct Tab {
    pub id: u64,
    pub kind: TabKind,
    pub title: String,
    pub sql: String,
    pub result: Option<QueryResult>,
    pub grid: GridState,
    pub error: Option<String>,
    pub running: bool,
    pub elapsed_ms: Option<u64>,
    /// Incremented by every `begin_run`; see the module docs.
    pub run_seq: u64,
    /// Paging, filters, sort and structure for a table tab; `None` on a query tab.
    pub inspector: Option<TableView>,
}

impl Tab {
    pub fn is_query(&self) -> bool {
        self.kind == TabKind::Query
    }
}

#[derive(Default)]
pub struct Tabs {
    tabs: Vec<Tab>,
    active: Option<u64>,
    next_id: u64,
    /// Only ever grows, so closing "Query 2" never makes the next one "Query 2" again.
    query_count: u32,
}

impl Tabs {
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    fn push(&mut self, kind: TabKind, title: String, sql: String) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.tabs.push(Tab {
            id,
            kind,
            title,
            sql,
            result: None,
            grid: GridState::default(),
            error: None,
            running: false,
            elapsed_ms: None,
            run_seq: 0,
            inspector: None,
        });
        self.active = Some(id);
        id
    }

    /// A new query tab, made active.
    pub fn open_query(&mut self, sql: impl Into<String>) -> u64 {
        self.query_count += 1;
        let title = format!("Query {}", self.query_count);
        self.push(TabKind::Query, title, sql.into())
    }

    /// The preview tab for `name`, made active. Returns its id and whether it
    /// was newly created (so the caller knows to run its query).
    pub fn open_table(&mut self, name: &str, sql: String) -> (u64, bool) {
        if let Some(t) = self
            .tabs
            .iter()
            .find(|t| matches!(&t.kind, TabKind::Table(n) if n == name))
        {
            let id = t.id;
            self.active = Some(id);
            return (id, false);
        }
        let id = self.push(TabKind::Table(name.to_string()), name.to_string(), sql);
        if let Some(t) = self.get_mut(id) {
            t.inspector = Some(TableView::default());
        }
        (id, true)
    }

    pub fn select(&mut self, id: u64) -> bool {
        if self.tabs.iter().any(|t| t.id == id) {
            self.active = Some(id);
            true
        } else {
            false
        }
    }

    pub fn id_at(&self, index: usize) -> Option<u64> {
        self.tabs.get(index).map(|t| t.id)
    }

    pub fn active_index(&self) -> Option<usize> {
        let id = self.active?;
        self.tabs.iter().position(|t| t.id == id)
    }

    pub fn active(&self) -> Option<&Tab> {
        self.get(self.active?)
    }

    pub fn active_mut(&mut self) -> Option<&mut Tab> {
        let id = self.active?;
        self.get_mut(id)
    }

    pub fn get(&self, id: u64) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    /// The table whose preview tab is active, for highlighting it in a sidebar.
    pub fn selected_table(&self) -> Option<&str> {
        match &self.active()?.kind {
            TabKind::Table(n) => Some(n.as_str()),
            TabKind::Query => None,
        }
    }

    /// Whether closing this tab would throw away SQL the user typed.
    pub fn needs_close_confirmation(&self, id: u64) -> bool {
        self.get(id)
            .map(|t| {
                (t.is_query() && !t.sql.trim().is_empty())
                    // Row edits that were never committed would be lost.
                    || t.inspector.as_ref().is_some_and(|v| v.edit.has_pending())
            })
            .unwrap_or(false)
    }

    /// Close a tab. If it was active, the one to its right takes over, else
    /// the one to its left.
    pub fn close(&mut self, id: u64) -> bool {
        let Some(pos) = self.tabs.iter().position(|t| t.id == id) else {
            return false;
        };
        self.tabs.remove(pos);
        if self.active == Some(id) {
            self.active = self
                .tabs
                .get(pos)
                .or_else(|| pos.checked_sub(1).and_then(|p| self.tabs.get(p)))
                .map(|t| t.id);
        }
        true
    }

    /// Mark a tab as running and return the sequence number the result must
    /// come back with. `None` if the tab is gone or already running.
    pub fn begin_run(&mut self, id: u64) -> Option<u64> {
        let t = self.get_mut(id)?;
        if t.running {
            return None;
        }
        t.running = true;
        t.error = None;
        t.run_seq += 1;
        Some(t.run_seq)
    }

    /// Start a run that supersedes any run in flight, returning its sequence
    /// number. A table tab uses this: turning a page or changing a filter
    /// while the last page is still loading must win, not be refused. The
    /// superseded run's result is then refused by `finish_run`.
    pub fn restart_run(&mut self, id: u64) -> Option<u64> {
        let t = self.get_mut(id)?;
        t.running = true;
        t.error = None;
        t.run_seq += 1;
        Some(t.run_seq)
    }

    /// Land a result. `false` means it was stale or the tab is gone, and
    /// nothing changed.
    pub fn finish_run(
        &mut self,
        id: u64,
        seq: u64,
        outcome: Result<QueryResult, String>,
        elapsed_ms: u64,
    ) -> bool {
        let Some(t) = self.get_mut(id) else {
            return false;
        };
        if t.run_seq != seq {
            return false;
        }
        t.running = false;
        t.elapsed_ms = Some(elapsed_ms);
        t.grid = GridState::default();
        match outcome {
            Ok(r) => {
                t.result = Some(r);
                t.error = None;
            }
            Err(e) => {
                t.result = None;
                t.error = Some(e);
            }
        }
        true
    }

    /// Forget every tab's results, e.g. after switching database. SQL stays.
    pub fn clear_results(&mut self) {
        for t in &mut self.tabs {
            t.result = None;
            t.error = None;
            t.elapsed_ms = None;
            t.run_seq += 1; // an in-flight run against the old database must not land
            t.running = false;
        }
    }
}

/// One line under the editor: "8 rows · 12 ms", or "3 rows affected · 4 ms"
/// for a statement that changed data instead of returning rows.
pub fn result_summary(result: &QueryResult, elapsed_ms: Option<u64>) -> String {
    let time = elapsed_ms.map(elapsed_label);
    let head = match (result.columns.is_empty(), result.rows_affected) {
        (true, Some(n)) => format!("{} {} affected", thousands(n as usize), plural(n as usize, "row")),
        _ => format!(
            "{} {}",
            thousands(result.rows.len()),
            plural(result.rows.len(), "row")
        ),
    };
    match time {
        Some(t) => format!("{head} · {t}"),
        None => head,
    }
}

/// "0ms", "12ms", "1.25s". Unlike the history formatter, a fast query shows
/// "0ms" rather than "N/A".
fn elapsed_label(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.2}s", ms as f64 / 1000.0)
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        word.to_string()
    } else {
        format!("{word}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(n: usize) -> QueryResult {
        QueryResult {
            columns: vec!["id".into()],
            rows: (0..n).map(|i| vec![serde_json::json!(i)]).collect(),
            rows_affected: None,
        }
    }

    #[test]
    fn new_query_tabs_are_numbered_and_never_reuse_a_number() {
        let mut t = Tabs::default();
        let a = t.open_query("");
        let b = t.open_query("");
        assert_eq!(t.get(a).unwrap().title, "Query 1");
        assert_eq!(t.get(b).unwrap().title, "Query 2");
        t.close(b);
        let c = t.open_query("");
        assert_eq!(t.get(c).unwrap().title, "Query 3");
    }

    #[test]
    fn opening_a_table_twice_focuses_the_existing_tab() {
        let mut t = Tabs::default();
        let (a, created) = t.open_table("users", "SELECT 1".into());
        assert!(created);
        let q = t.open_query("");
        let (again, created) = t.open_table("users", "SELECT 1".into());
        assert!(!created);
        assert_eq!(again, a);
        assert_eq!(t.active().unwrap().id, a);
        assert_eq!(t.tabs().len(), 2);
        let _ = q;
    }

    #[test]
    fn closing_the_active_tab_activates_the_right_neighbour_else_the_left() {
        let mut t = Tabs::default();
        let a = t.open_query("");
        let b = t.open_query("");
        let c = t.open_query("");
        t.select(b);
        t.close(b);
        assert_eq!(t.active().unwrap().id, c);
        t.close(c);
        assert_eq!(t.active().unwrap().id, a);
        t.close(a);
        assert!(t.active().is_none() && t.is_empty());
    }

    #[test]
    fn closing_an_inactive_tab_keeps_the_active_one() {
        let mut t = Tabs::default();
        let a = t.open_query("");
        let b = t.open_query("");
        t.close(a);
        assert_eq!(t.active().unwrap().id, b);
        assert!(!t.close(a), "closing twice is a no-op");
    }

    #[test]
    fn only_query_tabs_with_text_ask_before_closing() {
        let mut t = Tabs::default();
        let empty = t.open_query("   ");
        let typed = t.open_query("SELECT 1");
        let (table, _) = t.open_table("users", "SELECT * FROM users".into());
        assert!(!t.needs_close_confirmation(empty));
        assert!(t.needs_close_confirmation(typed));
        assert!(!t.needs_close_confirmation(table));
        assert!(!t.needs_close_confirmation(9999));
    }

    #[test]
    fn a_result_lands_only_for_the_current_run() {
        let mut t = Tabs::default();
        let id = t.open_query("SELECT 1");
        let first = t.begin_run(id).unwrap();
        assert!(t.begin_run(id).is_none(), "already running");
        assert!(t.finish_run(id, first, Ok(rows(1)), 5));
        assert!(!t.get(id).unwrap().running);

        let second = t.begin_run(id).unwrap();
        // The first run's late duplicate must not overwrite anything.
        assert!(!t.finish_run(id, first, Ok(rows(99)), 1));
        assert!(t.get(id).unwrap().running);
        assert!(t.finish_run(id, second, Ok(rows(3)), 7));
        assert_eq!(t.get(id).unwrap().result.as_ref().unwrap().rows.len(), 3);
        assert_eq!(t.get(id).unwrap().elapsed_ms, Some(7));
    }

    #[test]
    fn a_result_for_a_closed_tab_is_refused() {
        let mut t = Tabs::default();
        let id = t.open_query("SELECT 1");
        let seq = t.begin_run(id).unwrap();
        t.close(id);
        assert!(!t.finish_run(id, seq, Ok(rows(1)), 1));
        assert!(t.is_empty());
    }

    #[test]
    fn an_error_replaces_the_result_and_a_success_clears_the_error() {
        let mut t = Tabs::default();
        let id = t.open_query("x");
        let s = t.begin_run(id).unwrap();
        t.finish_run(id, s, Ok(rows(2)), 1);
        let s = t.begin_run(id).unwrap();
        t.finish_run(id, s, Err("syntax error".into()), 1);
        let tab = t.get(id).unwrap();
        assert!(tab.result.is_none());
        assert_eq!(tab.error.as_deref(), Some("syntax error"));
        let s = t.begin_run(id).unwrap();
        t.finish_run(id, s, Ok(rows(1)), 1);
        assert!(t.get(id).unwrap().error.is_none());
    }

    #[test]
    fn clearing_results_keeps_sql_and_drops_in_flight_runs() {
        let mut t = Tabs::default();
        let id = t.open_query("SELECT 1");
        let seq = t.begin_run(id).unwrap();
        t.clear_results();
        assert!(!t.finish_run(id, seq, Ok(rows(1)), 1));
        assert_eq!(t.get(id).unwrap().sql, "SELECT 1");
        assert!(!t.get(id).unwrap().running);
    }

    #[test]
    fn only_table_tabs_carry_an_inspector() {
        let mut t = Tabs::default();
        let q = t.open_query("");
        let (tbl, _) = t.open_table("users", String::new());
        assert!(t.get(q).unwrap().inspector.is_none());
        assert!(t.get(tbl).unwrap().inspector.is_some());
    }

    #[test]
    fn restarting_a_run_supersedes_the_one_in_flight() {
        let mut t = Tabs::default();
        let (id, _) = t.open_table("users", String::new());
        let first = t.restart_run(id).unwrap();
        // begin_run refuses while a run is in flight; restart does not.
        assert!(t.begin_run(id).is_none());
        let second = t.restart_run(id).unwrap();
        assert!(!t.finish_run(id, first, Ok(rows(1)), 1), "the superseded page must not land");
        assert!(t.get(id).unwrap().running);
        assert!(t.finish_run(id, second, Ok(rows(2)), 1));
        assert_eq!(t.get(id).unwrap().result.as_ref().unwrap().rows.len(), 2);
        assert!(t.restart_run(999).is_none());
    }

    #[test]
    fn selected_table_follows_the_active_tab() {
        let mut t = Tabs::default();
        assert_eq!(t.selected_table(), None);
        t.open_table("users", String::new());
        assert_eq!(t.selected_table(), Some("users"));
        t.open_query("");
        assert_eq!(t.selected_table(), None);
    }

    #[test]
    fn summaries_read_naturally() {
        assert_eq!(result_summary(&rows(1), Some(12)), "1 row · 12ms");
        assert_eq!(result_summary(&rows(1), Some(0)), "1 row · 0ms");
        assert_eq!(result_summary(&rows(2), Some(1250)), "2 rows · 1.25s");
        assert_eq!(result_summary(&rows(1234), None), "1,234 rows");
        let dml = QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected: Some(3),
        };
        assert_eq!(result_summary(&dml, Some(4)), "3 rows affected · 4ms");
    }
}
