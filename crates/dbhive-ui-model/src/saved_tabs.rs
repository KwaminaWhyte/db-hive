//! What survives a restart of the workspace's tabs: the SQL in each query tab
//! and which tables were open, in order, and which tab was active.
//!
//! Results are never saved (they can be 50,000 rows and are stale on return);
//! table tabs are reopened and re-queried, query tabs come back with their
//! text and are not run. Pure data and rules, no IO and no toolkit: the shell
//! decides when to write and where.

use serde::{Deserialize, Serialize};

use crate::workspace_tabs::{TabKind, Tabs};

/// A query tab's text past this many bytes is not saved, so one pasted dump
/// cannot make the store file huge or slow every autosave.
pub const MAX_SAVED_SQL_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SavedTab {
    Query { sql: String },
    Table { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SavedWorkspace {
    pub tabs: Vec<SavedTab>,
    /// Index into `tabs` of the active tab.
    pub active: Option<usize>,
}

/// The store key for one connection's workspace on one database.
pub fn key(connection_id: &str, database: Option<&str>) -> String {
    format!("{}::{}", connection_id, database.unwrap_or(""))
}

impl SavedWorkspace {
    /// The tabs as they are now.
    pub fn capture(tabs: &Tabs) -> Self {
        let mut saved = Vec::new();
        let mut active = None;
        let active_id = tabs.active().map(|t| t.id);
        for t in tabs.tabs() {
            let entry = match &t.kind {
                TabKind::Query if t.sql.len() <= MAX_SAVED_SQL_BYTES => {
                    SavedTab::Query { sql: t.sql.clone() }
                }
                TabKind::Query => SavedTab::Query { sql: String::new() },
                TabKind::Table(name) => SavedTab::Table { name: name.clone() },
            };
            if Some(t.id) == active_id {
                active = Some(saved.len());
            }
            saved.push(entry);
        }
        Self {
            tabs: saved,
            active,
        }
    }

    /// Whether there is anything a person would miss: a table tab, or a
    /// query tab with text. A workspace of blank editors is the default state
    /// and is not worth a restore.
    pub fn is_worth_saving(&self) -> bool {
        self.tabs.iter().any(|t| match t {
            SavedTab::Query { sql } => !sql.trim().is_empty(),
            SavedTab::Table { .. } => true,
        })
    }

    /// Read a saved value back, tolerating a file from another version or one
    /// that was edited by hand: anything unreadable is simply "nothing saved".
    pub fn from_value(v: &serde_json::Value) -> Option<Self> {
        let mut saved: Self = serde_json::from_value(v.clone()).ok()?;
        if let Some(a) = saved.active {
            if a >= saved.tabs.len() {
                saved.active = None;
            }
        }
        Some(saved)
    }

    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    /// Drop table tabs whose table no longer exists, keeping `active` on the
    /// same tab (or none, if that tab was dropped).
    pub fn without_missing_tables(&self, exists: impl Fn(&str) -> bool) -> Self {
        let mut tabs = Vec::new();
        let mut active = None;
        for (i, t) in self.tabs.iter().enumerate() {
            if let SavedTab::Table { name } = t {
                if !exists(name) {
                    continue;
                }
            }
            if self.active == Some(i) {
                active = Some(tabs.len());
            }
            tabs.push(t.clone());
        }
        Self { tabs, active }
    }
}

/// Whether `tabs` is still the untouched start state (one blank query tab
/// that has not run), the only state a restore may replace.
pub fn is_pristine(tabs: &Tabs) -> bool {
    match tabs.tabs() {
        [t] => {
            t.kind == TabKind::Query && t.sql.trim().is_empty() && t.result.is_none() && !t.running
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tabs_with(queries: &[&str], tables: &[&str]) -> Tabs {
        let mut t = Tabs::default();
        for q in queries {
            t.open_query(*q);
        }
        for n in tables {
            t.open_table(n, String::new());
        }
        t
    }

    #[test]
    fn key_is_per_connection_and_database() {
        assert_eq!(key("c1", Some("shop")), "c1::shop");
        assert_eq!(key("c1", None), "c1::");
        assert_ne!(key("c1", Some("a")), key("c1", Some("b")));
    }

    #[test]
    fn capture_keeps_order_sql_and_the_active_tab() {
        let mut t = tabs_with(&["SELECT 1", "SELECT 2"], &["orders"]);
        // The table tab was opened last, so it is active.
        let s = SavedWorkspace::capture(&t);
        assert_eq!(
            s.tabs,
            vec![
                SavedTab::Query {
                    sql: "SELECT 1".into()
                },
                SavedTab::Query {
                    sql: "SELECT 2".into()
                },
                SavedTab::Table {
                    name: "orders".into()
                },
            ]
        );
        assert_eq!(s.active, Some(2));
        let first = t.id_at(0).unwrap();
        t.select(first);
        assert_eq!(SavedWorkspace::capture(&t).active, Some(0));
    }

    #[test]
    fn blank_editors_alone_are_not_worth_saving() {
        assert!(!SavedWorkspace::capture(&tabs_with(&["", "  \n"], &[])).is_worth_saving());
        assert!(!SavedWorkspace::default().is_worth_saving());
        assert!(SavedWorkspace::capture(&tabs_with(&["", "SELECT 1"], &[])).is_worth_saving());
        assert!(SavedWorkspace::capture(&tabs_with(&[""], &["t"])).is_worth_saving());
    }

    #[test]
    fn a_huge_query_is_saved_empty_not_whole() {
        let big = "x".repeat(MAX_SAVED_SQL_BYTES + 1);
        let s = SavedWorkspace::capture(&tabs_with(&[big.as_str()], &[]));
        assert_eq!(s.tabs, vec![SavedTab::Query { sql: String::new() }]);
    }

    #[test]
    fn it_round_trips_through_json_and_survives_garbage() {
        let s = SavedWorkspace::capture(&tabs_with(&["SELECT 'it''s'"], &["we\"ird"]));
        assert_eq!(SavedWorkspace::from_value(&s.to_value()), Some(s));
        assert_eq!(SavedWorkspace::from_value(&json!("nonsense")), None);
        assert_eq!(
            SavedWorkspace::from_value(&json!({"tabs": [{"kind": "mystery"}]})),
            None
        );
        // An active index past the end is dropped, not trusted.
        let v = json!({"tabs": [{"kind": "query", "sql": "a"}], "active": 7});
        assert_eq!(SavedWorkspace::from_value(&v).unwrap().active, None);
    }

    #[test]
    fn missing_tables_are_dropped_and_active_follows_its_tab() {
        let s = SavedWorkspace {
            tabs: vec![
                SavedTab::Table {
                    name: "gone".into(),
                },
                SavedTab::Query {
                    sql: "SELECT 1".into(),
                },
                SavedTab::Table {
                    name: "kept".into(),
                },
            ],
            active: Some(2),
        };
        let r = s.without_missing_tables(|n| n == "kept");
        assert_eq!(r.tabs.len(), 2);
        assert_eq!(r.active, Some(1));
        // If the active tab itself is gone there is no active tab.
        let s2 = SavedWorkspace {
            active: Some(0),
            ..s
        };
        assert_eq!(s2.without_missing_tables(|n| n == "kept").active, None);
    }

    #[test]
    fn only_a_single_blank_untouched_tab_is_pristine() {
        assert!(is_pristine(&tabs_with(&[""], &[])));
        assert!(!is_pristine(&tabs_with(&["SELECT 1"], &[])));
        assert!(!is_pristine(&tabs_with(&["", ""], &[])));
        assert!(!is_pristine(&tabs_with(&[""], &["t"])));
        assert!(!is_pristine(&Tabs::default()));
    }
}
